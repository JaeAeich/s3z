//! Bounded async work pool — runs up to N tasks in parallel, fail-fast on error.

use std::future::Future;

use tokio::task::JoinSet;

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
}
