//! Batch download operation — S3 prefix to local directory.

use std::{
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::sync::mpsc;

use crate::{
    client::S3Client,
    config::TransferConfig,
    error::{Error, Result},
    http::ObjectKey,
    ops::list::{ListRequest, ObjectInfo},
    trace::{maybe_debug, maybe_info},
    transfer::{download, pool, scheduler},
};

/// Default number of files downloaded in parallel.
const DEFAULT_WORKERS: usize = 32;
/// Default number of parts downloaded concurrently per file.
const DEFAULT_CONCURRENCY: usize = 8;

/// Total concurrent HTTP request budget used by [`tune_parallelism`].
const TOTAL_REQUEST_BUDGET: usize = 256;

/// Callback invoked after each file download completes successfully.
type ProgressCallback = Arc<dyn Fn(&FileDownloadResult) + Send + Sync>;

/// Outcome for a single downloaded file.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FileDownloadResult {
    /// Local path the file was written to.
    pub dest: PathBuf,
    /// S3 key that was downloaded.
    pub key: String,
    /// Number of parts used (1 = single GET).
    pub parts: u32,
    /// File size in bytes.
    pub size: u64,
}

/// A batch download request.
///
/// Downloads all objects under `prefix` in `bucket` into `dest_dir`,
/// preserving the key structure relative to the prefix.
#[non_exhaustive]
pub struct DownloadRequest {
    /// Source S3 bucket.
    pub bucket: String,
    /// Number of parts downloaded concurrently within a single multipart download.
    pub concurrency_per_file: usize,
    /// Local directory to write files into.
    pub dest_dir: PathBuf,
    /// Callback invoked after each file download completes.
    on_file_complete: Option<ProgressCallback>,
    /// Key prefix in the bucket.
    pub prefix: String,
    /// Number of files downloaded in parallel.
    pub workers: usize,
}

impl fmt::Debug for DownloadRequest {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DownloadRequest")
            .field("bucket", &self.bucket)
            .field("concurrency_per_file", &self.concurrency_per_file)
            .field("dest_dir", &self.dest_dir)
            .field("on_file_complete", &self.on_file_complete.as_ref().map(|_| ".."))
            .field("prefix", &self.prefix)
            .field("workers", &self.workers)
            .finish()
    }
}

impl DownloadRequest {
    /// Create a new download request with sensible defaults.
    #[inline]
    #[must_use]
    pub fn new(
        bucket: impl Into<String>, prefix: impl Into<String>, dest_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            bucket: bucket.into(),
            concurrency_per_file: DEFAULT_CONCURRENCY,
            dest_dir: dest_dir.into(),
            on_file_complete: None,
            prefix: prefix.into(),
            workers: DEFAULT_WORKERS,
        }
    }

    /// Auto-tune workers and concurrency based on the object list.
    ///
    /// Call this after listing when using [`S3Client::download_objects`]
    /// to let the heuristic pick optimal parallelism for the workload.
    #[inline]
    #[must_use]
    pub fn auto_tune(mut self, objects: &[ObjectInfo], multipart_threshold: u64) -> Self {
        let config = tune_parallelism(objects, multipart_threshold);
        self.workers = config.workers;
        self.concurrency_per_file = config.concurrency_per_file;
        self
    }

    /// Set a callback that fires after each file download completes.
    #[inline]
    #[must_use]
    pub fn on_file_complete(
        mut self, callback: impl Fn(&FileDownloadResult) + Send + Sync + 'static,
    ) -> Self {
        self.on_file_complete = Some(Arc::new(callback));
        self
    }
}

/// Result of a batch download.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DownloadResult {
    /// Per-file outcomes.
    pub files: Vec<FileDownloadResult>,
}

#[expect(clippy::multiple_inherent_impl, reason = "ops extend S3Client from their own modules")]
impl S3Client {
    /// Download all objects under a prefix to a local directory.
    ///
    /// Listing and downloading are pipelined: the first page of
    /// `ListObjectsV2` results is used to auto-tune parallelism, then
    /// remaining pages are fetched concurrently with downloads so that
    /// large listings don't block the start of data transfer.
    ///
    /// If `workers` or `concurrency_per_file` were left at their defaults,
    /// they are replaced with auto-tuned values. Explicitly set values are
    /// respected, but a tracing warning is emitted when they differ from
    /// the recommendation.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use s3z::{Config, S3Client, DownloadRequest, auth::CredentialSource};
    /// # async fn example() -> s3z::error::Result<()> {
    /// let client = S3Client::new(Config::new("us-east-1", CredentialSource::Env)).await?;
    /// let result = client.download(DownloadRequest::new("my-bucket", "data/", "./local/")).await?;
    /// for f in &result.files {
    ///     println!("{} -> {}", f.key, f.dest.display());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if listing, any individual download, or file I/O fails.
    pub async fn download(&self, mut req: DownloadRequest) -> Result<DownloadResult> {
        let mut paginator = self.list(ListRequest::new(&req.bucket, &req.prefix));

        // Fetch the first page to sample the workload for auto-tuning.
        let first_page = paginator.next_page().await?;
        let first_objects = match first_page {
            Some(page) => page.objects,
            None => {
                return Ok(DownloadResult {
                    files: Vec::new(),
                });
            },
        };

        // Auto-tune based on the first page sample.
        let tuned =
            tune_parallelism(&first_objects, self.config.transfer.multipart_download_threshold);
        let user_set_workers = req.workers != DEFAULT_WORKERS;
        let user_set_concurrency = req.concurrency_per_file != DEFAULT_CONCURRENCY;

        if user_set_workers || user_set_concurrency {
            // Respect explicit values, but warn if they differ from the recommendation.
            if req.workers != tuned.workers
                || req.concurrency_per_file != tuned.concurrency_per_file
            {
                maybe_info!(
                    user_workers = req.workers,
                    user_concurrency = req.concurrency_per_file,
                    recommended_workers = tuned.workers,
                    recommended_concurrency = tuned.concurrency_per_file,
                    "using user-specified parallelism; auto-tuned values differ"
                );
            }
        } else {
            req.workers = tuned.workers;
            req.concurrency_per_file = tuned.concurrency_per_file;
        }

        // Buffer enough items to keep the pool fed without over-allocating.
        let channel_cap = req.workers.saturating_mul(2).max(1);
        let (tx, rx) = mpsc::channel::<ObjectInfo>(channel_cap);

        // Spawn the paginator as a background producer. It sends the first
        // page's objects, then continues fetching remaining pages.
        // Returns Ok(()) on success, Err on listing failure.
        let mut paginator_owned = paginator;
        let list_handle = tokio::task::spawn(async move {
            // Send first-page objects.
            for obj in first_objects {
                if tx.send(obj).await.is_err() {
                    return Ok(());
                }
            }
            // Continue with remaining pages.
            loop {
                match paginator_owned.next_page().await {
                    Ok(Some(page)) => {
                        for obj in page.objects {
                            if tx.send(obj).await.is_err() {
                                return Ok(());
                            }
                        }
                    },
                    Ok(None) => return Ok(()),
                    Err(e) => return Err(e),
                }
            }
        });

        let http = self.http.clone();
        let config = self.config.clone();
        let creds = self.creds.clone();
        let bucket = req.bucket.clone();
        let prefix = req.prefix.clone();
        let dest_dir = req.dest_dir.clone();
        let concurrency = req.concurrency_per_file;

        let on_complete: Option<&(dyn Fn(&FileDownloadResult) + Send + Sync)> =
            req.on_file_complete.as_deref();

        let pool_result = pool::run_pool_rx(
            rx,
            req.workers,
            |obj| {
                let http = http.clone();
                let cfg = config.clone();
                let cr = creds.clone();
                let bkt = bucket.clone();
                let pfx = prefix.clone();
                let dest = dest_dir.clone();
                async move {
                    download_single_object(&http, &cfg, &cr, &bkt, &pfx, &obj, &dest, concurrency)
                        .await
                }
            },
            on_complete,
        )
        .await;

        // Check the listing task first — a listing failure is the root cause
        // when the pool starves because the channel closed early.
        let list_result = list_handle.await.map_err(|e| Error::Internal(e.to_string()))?;
        list_result?;

        let files = pool_result?;

        Ok(DownloadResult {
            files,
        })
    }

    /// Download a pre-listed set of objects to a local directory.
    ///
    /// Use this when you have already listed the objects (e.g. to build a
    /// progress bar) and want to avoid a second `ListObjectsV2` round-trip.
    ///
    /// # Errors
    ///
    /// Returns an error if any individual download or file I/O fails.
    pub async fn download_objects(
        &self, req: DownloadRequest, objects: Vec<ObjectInfo>,
    ) -> Result<DownloadResult> {
        #[cfg(feature = "tracing")]
        let total = objects.len();
        maybe_info!(
            files = total,
            workers = req.workers,
            concurrency = req.concurrency_per_file,
            bucket = %req.bucket,
            prefix = %req.prefix,
            dest = %req.dest_dir.display(),
            "starting download batch"
        );

        if objects.is_empty() {
            return Ok(DownloadResult {
                files: Vec::new(),
            });
        }

        let http = self.http.clone();
        let config = self.config.clone();
        let creds = self.creds.clone();
        let bucket = req.bucket.clone();
        let prefix = req.prefix.clone();
        let dest_dir = req.dest_dir.clone();
        let concurrency = req.concurrency_per_file;

        let on_complete: Option<&(dyn Fn(&FileDownloadResult) + Send + Sync)> =
            req.on_file_complete.as_deref();

        let files = pool::run_pool(
            objects,
            req.workers,
            |obj| {
                let http = http.clone();
                let cfg = config.clone();
                let cr = creds.clone();
                let bkt = bucket.clone();
                let pfx = prefix.clone();
                let dest = dest_dir.clone();
                async move {
                    download_single_object(&http, &cfg, &cr, &bkt, &pfx, &obj, &dest, concurrency)
                        .await
                }
            },
            on_complete,
        )
        .await?;

        Ok(DownloadResult {
            files,
        })
    }
}

/// Parallelism settings returned by [`tune_parallelism`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParallelismConfig {
    /// Number of files downloaded in parallel.
    pub workers: usize,
    /// Number of parts downloaded concurrently within a single multipart download.
    pub concurrency_per_file: usize,
}

/// Analyze a set of objects and return optimal `(workers, concurrency_per_file)`.
///
/// Uses a fixed total request budget (256 concurrent requests) and distributes
/// it based on the median file size relative to the multipart threshold:
///
/// - **Small-files workload** (median ≤ threshold): most files will use a single GET, so maximize
///   worker count and set concurrency to 1.
/// - **Large-files workload** (median > threshold): fewer workers, more per-file concurrency to
///   saturate bandwidth on each large object.
///
/// Returns defaults if the object list is empty.
#[must_use]
pub fn tune_parallelism(objects: &[ObjectInfo], multipart_threshold: u64) -> ParallelismConfig {
    if objects.is_empty() {
        return ParallelismConfig {
            workers: DEFAULT_WORKERS,
            concurrency_per_file: DEFAULT_CONCURRENCY,
        };
    }

    let mut sizes: Vec<u64> = objects.iter().map(|o| o.size).collect();
    let mid = sizes.len() / 2;
    sizes.select_nth_unstable(mid);
    let median = sizes[mid];

    if median <= multipart_threshold {
        // Small-files mode: single GET per file, maximize parallelism.
        let workers = TOTAL_REQUEST_BUDGET.min(objects.len());
        ParallelismConfig {
            workers: workers.max(1),
            concurrency_per_file: 1,
        }
    } else {
        // Large-files mode: fewer workers, more parts per file.
        // Target ~8 parts per file (matches DEFAULT_CONCURRENCY), but
        // cap so workers * concurrency ≤ budget.
        let target_concurrency = DEFAULT_CONCURRENCY;
        let workers = (TOTAL_REQUEST_BUDGET / target_concurrency).min(objects.len()).max(1);
        ParallelismConfig {
            workers,
            concurrency_per_file: target_concurrency,
        }
    }
}

/// Download a single object — picks single GET or multipart based on size.
#[expect(clippy::too_many_arguments, reason = "internal fn, context struct would add indirection")]
async fn download_single_object(
    http: &reqwest::Client, config: &crate::config::Config, creds: &crate::auth::Credentials,
    bucket: &str, prefix: &str, obj: &ObjectInfo, dest_dir: &Path, concurrency: usize,
) -> Result<FileDownloadResult> {
    let rel_key = obj.key.strip_prefix(prefix).unwrap_or(&obj.key);
    let dest = dest_dir.join(rel_key);
    let key = ObjectKey::new(&obj.key);

    #[cfg(feature = "tracing")]
    let file_start = std::time::Instant::now();

    let (size, parts) = if obj.size <= config.transfer.multipart_download_threshold {
        maybe_debug!(key = %key, size = obj.size, "single GET");
        let size = download::download_single(http, config, creds, bucket, &key, &dest).await?;
        (size, 1_u32)
    } else {
        maybe_debug!(key = %key, size = obj.size, "multipart download");
        let part_size = scheduler::compute_download_part_size(obj.size, concurrency);
        let transfer = TransferConfig {
            part_size,
            ..config.transfer
        };
        let parts_plan = scheduler::plan_parts(obj.size, &transfer);
        let parts_count =
            u32::try_from(parts_plan.len()).map_err(|e| Error::Conversion(e.to_string()))?;
        let size = download::download_multipart(
            http,
            config,
            creds,
            bucket,
            &key,
            &parts_plan,
            &dest,
            obj.size,
            concurrency,
        )
        .await?;
        (size, parts_count)
    };

    #[cfg(feature = "tracing")]
    let elapsed = file_start.elapsed();

    maybe_info!(key = %key, size, parts, ?elapsed, "file downloaded");
    Ok(FileDownloadResult {
        dest,
        key: obj.key.clone(),
        parts,
        size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1024 * 1024;
    const THRESHOLD: u64 = 50 * MB;

    fn make_objects(sizes: &[u64]) -> Vec<ObjectInfo> {
        sizes
            .iter()
            .enumerate()
            .map(|(i, &size)| {
                ObjectInfo {
                    key: format!("file{i}.bin"),
                    size,
                    etag: String::new(),
                    last_modified: String::new(),
                }
            })
            .collect()
    }

    #[test]
    fn tune_empty_returns_defaults() {
        let cfg = tune_parallelism(&[], THRESHOLD);
        assert_eq!(cfg.workers, DEFAULT_WORKERS);
        assert_eq!(cfg.concurrency_per_file, DEFAULT_CONCURRENCY);
    }

    #[test]
    fn tune_small_files_maximizes_workers() {
        let objects = make_objects(&[MB; 100]);
        let cfg = tune_parallelism(&objects, THRESHOLD);
        assert_eq!(cfg.workers, 100);
        assert_eq!(cfg.concurrency_per_file, 1);
    }

    #[test]
    fn tune_small_files_caps_at_budget() {
        let objects = make_objects(&[MB; 1000]);
        let cfg = tune_parallelism(&objects, THRESHOLD);
        assert_eq!(cfg.workers, TOTAL_REQUEST_BUDGET);
        assert_eq!(cfg.concurrency_per_file, 1);
    }

    #[test]
    fn tune_large_files_uses_concurrency() {
        let objects = make_objects(&[500 * MB; 10]);
        let cfg = tune_parallelism(&objects, THRESHOLD);
        assert_eq!(cfg.concurrency_per_file, DEFAULT_CONCURRENCY);
        assert_eq!(cfg.workers, 10);
    }

    #[test]
    fn tune_large_files_caps_workers() {
        let objects = make_objects(&[500 * MB; 100]);
        let cfg = tune_parallelism(&objects, THRESHOLD);
        assert_eq!(cfg.concurrency_per_file, DEFAULT_CONCURRENCY);
        assert_eq!(cfg.workers, TOTAL_REQUEST_BUDGET / DEFAULT_CONCURRENCY);
    }

    #[test]
    fn tune_single_small_file() {
        let objects = make_objects(&[10 * MB]);
        let cfg = tune_parallelism(&objects, THRESHOLD);
        assert_eq!(cfg.workers, 1);
        assert_eq!(cfg.concurrency_per_file, 1);
    }

    #[test]
    fn tune_single_large_file() {
        let objects = make_objects(&[500 * MB]);
        let cfg = tune_parallelism(&objects, THRESHOLD);
        assert_eq!(cfg.workers, 1);
        assert_eq!(cfg.concurrency_per_file, DEFAULT_CONCURRENCY);
    }

    #[test]
    fn tune_mixed_files_uses_median() {
        // 5 small, 4 large → median is small → small-files mode
        let mut sizes = vec![MB; 5];
        sizes.extend([500 * MB; 4]);
        let objects = make_objects(&sizes);
        let cfg = tune_parallelism(&objects, THRESHOLD);
        assert_eq!(cfg.concurrency_per_file, 1);
    }

    #[test]
    fn auto_tune_applies_config() {
        let objects = make_objects(&[MB; 50]);
        let req =
            DownloadRequest::new("bucket", "prefix/", "/tmp/dest").auto_tune(&objects, THRESHOLD);
        assert_eq!(req.workers, 50);
        assert_eq!(req.concurrency_per_file, 1);
    }
}
