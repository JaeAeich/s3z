//! Batch upload operation — local files/dirs to S3.

use std::{
    fmt,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use bytes::Bytes;
use tokio::fs;
use walkdir::WalkDir;

use crate::{
    auth::Credentials,
    client::S3Client,
    config::{Config, TransferConfig},
    error::{Error, Result},
    http::{ObjectKey, request::build_signed, retry::send_with_retry},
    trace::{maybe_debug, maybe_info},
    transfer::{multipart, pool, scheduler},
};

/// Callback invoked after each file upload completes successfully.
type ProgressCallback = Arc<dyn Fn(&FileUploadResult) + Send + Sync>;

/// Outcome for a single uploaded file.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FileUploadResult {
    /// `ETag` returned by S3.
    pub etag: String,
    /// S3 key the file was written to.
    pub key: String,
    /// Number of parts used (1 = single PUT).
    pub parts: u32,
    /// File size in bytes.
    pub size: u64,
    /// Local path that was uploaded.
    pub source: PathBuf,
}

/// A batch upload request.
///
/// Accepts a list of files and/or directories. Directories are walked
/// recursively. All files are uploaded under `dest_prefix` in `dest_bucket`,
/// preserving relative paths from the common ancestor of `sources`.
#[non_exhaustive]
pub struct UploadRequest {
    /// Number of parts uploaded concurrently within a single multipart upload.
    pub concurrency_per_file: usize,
    /// Target S3 bucket.
    pub dest_bucket: String,
    /// Key prefix in the bucket (e.g. `data/2024/`).
    pub dest_prefix: String,
    /// Callback invoked after each file upload completes.
    on_file_complete: Option<ProgressCallback>,
    /// Local file or directory paths to upload.
    pub sources: Vec<PathBuf>,
    /// Number of files uploaded in parallel.
    pub workers: usize,
}

impl fmt::Debug for UploadRequest {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UploadRequest")
            .field("concurrency_per_file", &self.concurrency_per_file)
            .field("dest_bucket", &self.dest_bucket)
            .field("dest_prefix", &self.dest_prefix)
            .field("on_file_complete", &self.on_file_complete.as_ref().map(|_| ".."))
            .field("sources", &self.sources)
            .field("workers", &self.workers)
            .finish()
    }
}

impl UploadRequest {
    /// Expand sources and count the total number of files that would be uploaded.
    ///
    /// Useful for setting up progress bars before calling [`S3Client::upload`].
    ///
    /// # Errors
    ///
    /// Returns an error if source expansion fails (e.g. non-UTF-8 paths).
    #[inline]
    pub fn file_count(&self) -> Result<usize> {
        expand_sources(&self.sources, &self.dest_prefix).map(|entries| entries.len())
    }

    /// Create a new upload request with sensible defaults.
    ///
    /// Defaults: 32 workers, 8 concurrent parts per file.
    ///
    /// Each part is streamed from disk through a 256 KiB buffer, so peak
    /// memory per file is roughly `concurrency_per_file * 256 KiB`.
    #[inline]
    #[must_use]
    pub fn new(
        sources: Vec<PathBuf>, dest_bucket: impl Into<String>, dest_prefix: impl Into<String>,
    ) -> Self {
        Self {
            concurrency_per_file: 8,
            dest_bucket: dest_bucket.into(),
            dest_prefix: dest_prefix.into(),
            on_file_complete: None,
            sources,
            workers: 32,
        }
    }

    /// Set a callback that fires after each file upload completes.
    ///
    /// The callback receives a reference to the [`FileUploadResult`] for the
    /// just-completed file. It runs on the tokio worker thread, so keep it
    /// lightweight (e.g. update a progress bar).
    #[inline]
    #[must_use]
    pub fn on_file_complete(
        mut self, callback: impl Fn(&FileUploadResult) + Send + Sync + 'static,
    ) -> Self {
        self.on_file_complete = Some(Arc::new(callback));
        self
    }
}

/// Result of a batch upload.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct UploadResult {
    /// Per-file outcomes.
    pub files: Vec<FileUploadResult>,
}

#[expect(clippy::multiple_inherent_impl, reason = "ops extend S3Client from their own modules")]
impl S3Client {
    /// Upload files and/or directories to S3.
    ///
    /// Files below the multipart threshold are uploaded with a single PUT.
    /// Larger files are automatically split into parts and uploaded concurrently.
    ///
    /// # Errors
    ///
    /// Returns an error if credential resolution, source expansion, or any
    /// individual file upload fails.
    #[inline]
    pub async fn upload(&self, req: UploadRequest) -> Result<UploadResult> {
        let entries = expand_sources(&req.sources, &req.dest_prefix)?;

        #[cfg(feature = "tracing")]
        let total = entries.len();
        maybe_info!(
            files = total,
            workers = req.workers,
            concurrency = req.concurrency_per_file,
            bucket = %req.dest_bucket,
            prefix = %req.dest_prefix,
            "starting upload batch"
        );

        let http = self.http.clone();
        let config = self.config.clone();
        let creds = self.creds.clone();
        let bucket = req.dest_bucket.clone();
        let concurrency = req.concurrency_per_file;

        let on_complete: Option<&(dyn Fn(&FileUploadResult) + Send + Sync)> =
            req.on_file_complete.as_deref();

        let files =
            pool::run_pool(
                entries,
                req.workers,
                |(path, key)| {
                    let http = http.clone();
                    let cfg = config.clone();
                    let cr = creds.clone();
                    let bkt = bucket.clone();
                    async move {
                        upload_single_file(&http, &cfg, &cr, &bkt, &key, &path, concurrency).await
                    }
                },
                on_complete,
            )
            .await?;

        Ok(UploadResult {
            files,
        })
    }
}

/// Expand sources into a flat list of `(local_path, object_key)` pairs.
fn expand_sources(sources: &[PathBuf], prefix: &str) -> Result<Vec<(PathBuf, ObjectKey)>> {
    let mut entries = Vec::new();

    for source in sources {
        if source.is_dir() {
            for walk_entry in WalkDir::new(source) {
                let walk_entry = walk_entry?;
                if !walk_entry.file_type().is_dir() {
                    let rel = walk_entry
                        .path()
                        .strip_prefix(source)
                        .map_err(|e| Error::Conversion(e.to_string()))?;
                    let rel_str = rel.to_str().ok_or_else(|| {
                        Error::Conversion(format!("non-UTF-8 path: {}", rel.to_string_lossy()))
                    })?;
                    let key = ObjectKey::new(format!("{prefix}{rel_str}"));
                    entries.push((walk_entry.into_path(), key));
                }
            }
        } else {
            let file_name = source
                .file_name()
                .ok_or_else(|| {
                    Error::Io(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "source has no file name",
                    ))
                })?
                .to_str()
                .ok_or_else(|| Error::Conversion("non-UTF-8 file name".into()))?;
            let key = ObjectKey::new(format!("{prefix}{file_name}"));
            entries.push((source.clone(), key));
        }
    }

    Ok(entries)
}

/// Single PUT upload for small files.
async fn single_put(
    http: &reqwest::Client, config: &Config, creds: &Credentials, bucket: &str, key: &ObjectKey,
    body: Vec<u8>,
) -> Result<(String, u32)> {
    let uri: http::Uri = format!("{}/{bucket}/{}", config.endpoint_url(), key.encoded()).parse()?;

    let req = build_signed(http::Method::PUT, uri, Bytes::from(body), creds, &config.region)?;
    let resp = send_with_retry(http, req, &config.retry).await?;

    let etag = resp
        .headers()
        .get("etag")
        .ok_or_else(|| {
            Error::S3 {
                code: "MissingETag".into(),
                message: "single PUT response missing ETag header".into(),
            }
        })?
        .to_str()
        .map_err(|e| {
            Error::S3 {
                code: "InvalidETag".into(),
                message: format!("ETag header is not valid ASCII: {e}"),
            }
        })?
        .to_owned();

    Ok((etag, 1))
}

/// Upload a single file — picks single PUT or multipart based on size.
async fn upload_single_file(
    http: &reqwest::Client, config: &Config, creds: &Credentials, bucket: &str, key: &ObjectKey,
    path: &Path, concurrency: usize,
) -> Result<FileUploadResult> {
    let metadata = fs::metadata(path).await?;
    let size = metadata.len();

    #[cfg(feature = "tracing")]
    let file_start = std::time::Instant::now();

    if size <= config.transfer.multipart_threshold {
        maybe_debug!(key = %key, size, "single PUT");
        let body = fs::read(path).await?;
        let (etag, parts) = single_put(http, config, creds, bucket, key, body).await?;

        #[cfg(feature = "tracing")]
        let elapsed = file_start.elapsed();

        maybe_info!(key = %key, size, ?elapsed, "file uploaded (single PUT)");
        Ok(FileUploadResult {
            etag,
            key: key.raw().to_owned(),
            parts,
            size,
            source: path.to_owned(),
        })
    } else {
        maybe_debug!(key = %key, size, "multipart upload");
        let part_size = scheduler::compute_part_size(size, concurrency);
        let transfer = TransferConfig {
            part_size,
            ..config.transfer
        };
        let parts_plan = scheduler::plan_parts(size, &transfer);
        let (etag, parts_count) = multipart::upload_multipart(
            http,
            config,
            creds,
            bucket,
            key,
            &parts_plan,
            path,
            concurrency,
        )
        .await?;

        #[cfg(feature = "tracing")]
        let elapsed = file_start.elapsed();

        maybe_info!(key = %key, size, parts = parts_count, ?elapsed, "file uploaded (multipart)");
        Ok(FileUploadResult {
            etag,
            key: key.raw().to_owned(),
            parts: parts_count,
            size,
            source: path.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn expand_single_file() {
        let dir = tempdir("single");
        let file = dir.join("hello.txt");
        fs::write(&file, "data").unwrap();

        let entries = expand_sources(std::slice::from_ref(&file), "prefix/").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, file);
        assert_eq!(entries[0].1.raw(), "prefix/hello.txt");
    }

    #[test]
    fn expand_directory_recursive() {
        let dir = tempdir("recursive");
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(dir.join("a.txt"), "a").unwrap();
        fs::write(sub.join("b.txt"), "b").unwrap();

        let entries = expand_sources(std::slice::from_ref(&dir), "p/").unwrap();
        assert_eq!(entries.len(), 2);

        let keys: Vec<&str> = entries.iter().map(|(_, k)| k.raw()).collect();
        assert!(keys.contains(&"p/a.txt"));
        assert!(keys.contains(&"p/sub/b.txt"));
    }

    #[test]
    fn expand_no_file_name_errors() {
        let result = expand_sources(&[PathBuf::from("/")], "prefix/");
        result.unwrap_err();
    }

    #[test]
    fn expand_empty_directory_produces_no_entries() {
        let dir = tempdir("empty");
        // dir exists but contains no files
        let entries = expand_sources(std::slice::from_ref(&dir), "p/").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn expand_nested_empty_dirs_produces_no_entries() {
        let dir = tempdir("nested_empty");
        fs::create_dir_all(dir.join("a/b/c")).unwrap();
        let entries = expand_sources(std::slice::from_ref(&dir), "p/").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn expand_preserves_relative_paths_in_key() {
        let dir = tempdir("relative");
        let deep = dir.join("a/b");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("data.bin"), "x").unwrap();

        let entries = expand_sources(std::slice::from_ref(&dir), "out/").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1.raw(), "out/a/b/data.bin");
    }

    #[test]
    fn expand_multiple_sources() {
        let dir = tempdir("multi");
        let f1 = dir.join("one.txt");
        let f2 = dir.join("two.txt");
        fs::write(&f1, "1").unwrap();
        fs::write(&f2, "2").unwrap();

        let entries = expand_sources(&[f1, f2], "pre/").unwrap();
        assert_eq!(entries.len(), 2);
        let keys: Vec<&str> = entries.iter().map(|(_, k)| k.raw()).collect();
        assert!(keys.contains(&"pre/one.txt"));
        assert!(keys.contains(&"pre/two.txt"));
    }

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("s3z_test_{name}_{}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
