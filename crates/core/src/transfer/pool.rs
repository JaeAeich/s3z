//! Bounded async work pool — runs up to N tasks in parallel, fail-fast on error.

use std::future::Future;

use tokio::{sync::mpsc, task::JoinSet};

use crate::error::{Error, Result};

/// Run `spawn_fn` for each item, keeping at most `workers` tasks in flight.
///
/// Calls `on_complete` (if provided) after each successful result.
/// On the first error the remaining tasks are aborted and the error is returned.
///
/// Results are returned in completion order, not submission order.
pub(crate) async fn run_pool<I, R, F, Fut>(
    items: impl IntoIterator<Item = I> + Send, workers: usize, spawn_fn: F,
    on_complete: Option<&(dyn Fn(&R) + Send + Sync)>,
) -> Result<Vec<R>>
where
    I: Send + 'static,
    R: Send + 'static,
    F: Fn(I) -> Fut + Send,
    Fut: Future<Output = Result<R>> + Send + 'static,
{
    assert!(workers > 0, "workers must be at least 1");

    let mut iter = items.into_iter();
    let (lower, _) = iter.size_hint();
    let mut results: Vec<R> = Vec::with_capacity(lower);
    let mut set = JoinSet::new();

    // Seed with up to `workers` initial tasks.
    for item in iter.by_ref().take(workers) {
        set.spawn(spawn_fn(item));
    }

    loop {
        let Some(handle) = set.join_next().await else {
            break;
        };

        match handle.map_err(|e| Error::Internal(e.to_string()))? {
            Ok(result) => {
                if let Some(cb) = on_complete {
                    cb(&result);
                }
                results.push(result);
            },
            Err(e) => {
                set.abort_all();
                return Err(e);
            },
        }

        // Refill: spawn one replacement for the slot that just freed up.
        if let Some(item) = iter.next() {
            set.spawn(spawn_fn(item));
        }
    }

    Ok(results)
}

/// Like [`run_pool`] but reads items from an async channel instead of an
/// iterator. This allows the producer (e.g. a paginator) to run concurrently
/// with the download workers.
///
/// The caller should spawn a producer that sends items into the `tx` side
/// and pass the `rx` side here. The pool runs until the channel is closed
/// and all in-flight tasks complete.
pub(crate) async fn run_pool_rx<I, R, F, Fut>(
    mut rx: mpsc::Receiver<I>, workers: usize, spawn_fn: F,
    on_complete: Option<&(dyn Fn(&R) + Send + Sync)>,
) -> Result<Vec<R>>
where
    I: Send + 'static,
    R: Send + 'static,
    F: Fn(I) -> Fut + Send,
    Fut: Future<Output = Result<R>> + Send + 'static,
{
    assert!(workers > 0, "workers must be at least 1");

    let mut results: Vec<R> = Vec::new();
    let mut set = JoinSet::new();
    let mut channel_open = true;

    loop {
        if set.is_empty() && !channel_open {
            break;
        }

        let has_capacity = channel_open && set.len() < workers;

        tokio::select! {
            Some(handle) = set.join_next() => {
                match handle.map_err(|e| Error::Internal(e.to_string()))? {
                    Ok(result) => {
                        if let Some(cb) = on_complete {
                            cb(&result);
                        }
                        results.push(result);
                    },
                    Err(e) => {
                        rx.close();
                        set.abort_all();
                        return Err(e);
                    },
                }
            }
            item = rx.recv(), if has_capacity => {
                match item {
                    Some(item) => {
                        set.spawn(spawn_fn(item));
                    },
                    None => { channel_open = false; },
                }
            }
            else => break,
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pool_processes_all_items() {
        let items: Vec<u32> = (0..10).collect();
        let results =
            run_pool(items, 3, |i| async move { Ok(i * 2) }, None::<&(dyn Fn(&u32) + Send + Sync)>)
                .await
                .unwrap();

        assert_eq!(results.len(), 10);
        let mut sorted = results;
        sorted.sort_unstable();
        assert_eq!(sorted, vec![0, 2, 4, 6, 8, 10, 12, 14, 16, 18]);
    }

    #[tokio::test]
    async fn pool_calls_on_complete() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let count = AtomicU32::new(0);
        let cb = |_: &u32| {
            count.fetch_add(1, Ordering::Relaxed);
        };

        let results =
            run_pool(vec![1, 2, 3], 2, |i| async move { Ok(i) }, Some(&cb)).await.unwrap();

        assert_eq!(results.len(), 3);
        assert_eq!(count.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn pool_fail_fast_on_error() {
        let results: Result<Vec<u32>> = run_pool(
            vec![1, 2, 3, 4, 5],
            2,
            |i| {
                async move {
                    if i == 3 {
                        Err(Error::Internal("boom".into()))
                    } else {
                        // Small delay so item 3 is likely in-flight
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        Ok(i)
                    }
                }
            },
            None::<&(dyn Fn(&u32) + Send + Sync)>,
        )
        .await;

        assert!(results.is_err());
    }

    #[tokio::test]
    async fn pool_empty_items() {
        let results: Result<Vec<u32>> = run_pool(
            Vec::new(),
            4,
            |i: u32| async move { Ok(i) },
            None::<&(dyn Fn(&u32) + Send + Sync)>,
        )
        .await;
        assert_eq!(results.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn pool_single_worker() {
        let results = run_pool(
            vec![10, 20, 30],
            1,
            |i| async move { Ok(i) },
            None::<&(dyn Fn(&u32) + Send + Sync)>,
        )
        .await
        .unwrap();

        assert_eq!(results.len(), 3);
        // With 1 worker, results should be in order
        assert_eq!(results, vec![10, 20, 30]);
    }

    #[tokio::test]
    #[should_panic(expected = "workers must be at least 1")]
    async fn pool_zero_workers_panics() {
        drop(
            run_pool(
                vec![1, 2, 3],
                0,
                |i: u32| async move { Ok(i) },
                None::<&(dyn Fn(&u32) + Send + Sync)>,
            )
            .await,
        );
    }

    #[tokio::test]
    async fn pool_rx_processes_all_items() {
        let (tx, rx) = mpsc::channel(4);
        tokio::spawn(async move {
            for i in 0..10_u32 {
                tx.send(i).await.unwrap();
            }
        });

        let results =
            run_pool_rx(rx, 3, |i| async move { Ok(i * 2) }, None::<&(dyn Fn(&u32) + Send + Sync)>)
                .await
                .unwrap();

        assert_eq!(results.len(), 10);
        let mut sorted = results;
        sorted.sort_unstable();
        assert_eq!(sorted, vec![0, 2, 4, 6, 8, 10, 12, 14, 16, 18]);
    }

    #[tokio::test]
    async fn pool_rx_fail_fast() {
        let (tx, rx) = mpsc::channel(4);
        tokio::spawn(async move {
            for i in 0..10_u32 {
                if tx.send(i).await.is_err() {
                    break;
                }
            }
        });

        let result: Result<Vec<u32>> = run_pool_rx(
            rx,
            2,
            |i| {
                async move {
                    if i == 3 {
                        Err(Error::Internal("boom".into()))
                    } else {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        Ok(i)
                    }
                }
            },
            None::<&(dyn Fn(&u32) + Send + Sync)>,
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn pool_rx_empty_channel() {
        let (tx, rx) = mpsc::channel::<u32>(1);
        // Drop tx immediately → channel closes
        drop(tx);

        let results = run_pool_rx(
            rx,
            4,
            |i: u32| async move { Ok(i) },
            None::<&(dyn Fn(&u32) + Send + Sync)>,
        )
        .await
        .unwrap();

        assert!(results.is_empty());
    }
}
