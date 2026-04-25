//! Batch upload operation — local files/dirs to S3.

use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use bytes::Bytes;
use tokio::{
    fs,
    sync::{AcquireError, OwnedSemaphorePermit, Semaphore},
    task::JoinSet,
};
use walkdir::WalkDir;

use crate::{
    auth::Credentials,
    client::S3Client,
    config::Config,
    error::{Error, Result},
    http::{ObjectKey, request::build_signed, retry::send_with_retry},
    transfer::{multipart, scheduler},
};

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
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct UploadRequest {
    /// Number of parts uploaded concurrently within a single multipart upload.
    pub concurrency_per_file: usize,
    /// Target S3 bucket.
    pub dest_bucket: String,
    /// Key prefix in the bucket (e.g. `data/2024/`).
    pub dest_prefix: String,
    /// Local file or directory paths to upload.
    pub sources: Vec<PathBuf>,
    /// Number of files uploaded in parallel.
    pub workers: usize,
}

impl UploadRequest {
    /// Create a new upload request with sensible defaults.
    ///
    /// Defaults: 32 workers, 8 concurrent parts per file.
    ///
    /// Parts are streamed from disk, so memory usage is bounded by
    /// tokio's read buffer size (~8 KiB) per in-flight part, not part size.
    #[inline]
    #[must_use]
    #[expect(clippy::impl_trait_in_params, reason = "ergonomic constructor API")]
    pub fn new(
        sources: Vec<PathBuf>, dest_bucket: impl Into<String>, dest_prefix: impl Into<String>,
    ) -> Self {
        Self {
            concurrency_per_file: 8,
            dest_bucket: dest_bucket.into(),
            dest_prefix: dest_prefix.into(),
            sources,
            workers: 32,
        }
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
        let creds = self.creds.clone();
        let entries = expand_sources(&req.sources, &req.dest_prefix)?;

        let sem = Arc::new(Semaphore::new(req.workers));
        let mut set = JoinSet::new();

        for (path, key) in entries {
            // Acquire permit before spawning to bound both memory and concurrency.
            let permit: OwnedSemaphorePermit = Arc::clone(&sem)
                .acquire_owned()
                .await
                .map_err(|e: AcquireError| Error::Internal(e.to_string()))?;
            let client = self.http.clone();
            let cfg = self.config.clone();
            let cr = creds.clone();
            let bkt = req.dest_bucket.clone();
            let conc = req.concurrency_per_file;

            set.spawn(async move {
                let _permit = permit;
                upload_single_file(&client, &cfg, &cr, &bkt, &key, &path, conc).await
            });
        }

        let mut files = Vec::with_capacity(set.len());
        while let Some(handle) = set.join_next().await {
            match handle.map_err(|e| Error::Internal(e.to_string()))? {
                Ok(result) => files.push(result),
                Err(e) => {
                    set.abort_all();
                    return Err(e);
                },
            }
        }

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
                #[expect(clippy::shadow_reuse, reason = "unwrapping Result into same-named value")]
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

    if size <= config.transfer.multipart_threshold {
        let body = fs::read(path).await?;
        let (etag, parts) = single_put(http, config, creds, bucket, key, body).await?;
        Ok(FileUploadResult {
            etag,
            key: key.raw().to_owned(),
            parts,
            size,
            source: path.to_owned(),
        })
    } else {
        let parts_plan = scheduler::plan_parts(size, &config.transfer);
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
