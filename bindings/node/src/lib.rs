//! Node.js bindings for s3z via NAPI-RS.
//!
//! All async S3 operations are exposed as JavaScript promises that run
//! on the tokio runtime managed by NAPI-RS.

// napi-derive generates constructor helpers, trailing arrays, and async
// wrappers that trip these lints — no way to fix at source.
#![allow(
    clippy::needless_pass_by_value,
    clippy::trailing_empty_array,
    clippy::unused_async,
    clippy::use_self,
    missing_docs,
    reason = "napi-derive generated code"
)]

use std::path::PathBuf;

use napi::bindgen_prelude::*;
use napi_derive::napi;

/// Run an async operation on the shared tokio runtime.
///
/// If called from within a tokio context (e.g. NAPI async), spawns the
/// future on the current runtime. Otherwise creates a dedicated runtime.
fn block_on_async<F, T>(f: F) -> T
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    use std::sync::OnceLock;
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

    // If we're already inside a tokio runtime (NAPI async context),
    // spawn on it and block the current (libuv) thread.
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        std::thread::scope(|s| {
            s.spawn(|| handle.block_on(f)).join().expect("spawned thread panicked")
        })
    } else {
        let rt = RT.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime")
        });
        rt.block_on(f)
    }
}

/// Map s3z errors to napi errors.
fn to_napi_err(e: &s3z::error::Error) -> Error {
    Error::from_reason(e.to_string())
}

/// Validate that a parallelism value is non-zero.
fn nonzero(name: &str, v: u32) -> Result<usize> {
    if v == 0 {
        return Err(Error::from_reason(format!("{name} must be > 0")));
    }
    Ok(v as usize)
}

/// Convert `u64` to `f64` for JS (safe up to 2^53).
#[allow(
    clippy::cast_precision_loss,
    reason = "JS number has no u64; files >8 PiB are not a concern"
)]
const fn size_to_js(v: u64) -> f64 {
    v as f64
}

/// Configuration for the S3 client.
#[napi(object)]
#[derive(Debug, Clone)]
pub struct S3Config {
    /// AWS region (e.g. "us-east-1").
    pub region: String,
    /// AWS access key ID. If omitted, reads from `AWS_ACCESS_KEY_ID` env var.
    pub access_key: Option<String>,
    /// AWS secret access key. If omitted, reads from `AWS_SECRET_ACCESS_KEY` env var.
    pub secret_key: Option<String>,
    /// Custom endpoint URL for S3-compatible backends (`MinIO`, R2, GCS).
    pub endpoint: Option<String>,
}

/// Result of a single file upload.
#[napi(object)]
#[derive(Debug, Clone)]
pub struct FileUploadResult {
    /// `ETag` returned by S3.
    pub etag: String,
    /// S3 key the file was written to.
    pub key: String,
    /// Number of parts used (1 = single PUT).
    pub parts: u32,
    /// File size in bytes.
    pub size: f64,
    /// Local path that was uploaded.
    pub source: String,
}

/// Result of a single file download.
#[napi(object)]
#[derive(Debug, Clone)]
pub struct FileDownloadResult {
    /// Local path the file was written to.
    pub dest: String,
    /// S3 key that was downloaded.
    pub key: String,
    /// Number of parts used.
    pub parts: u32,
    /// File size in bytes.
    pub size: f64,
}

/// Metadata for a single S3 object.
#[napi(object)]
#[derive(Debug, Clone)]
pub struct ObjectInfo {
    /// Object key.
    pub key: String,
    /// Object size in bytes.
    pub size: f64,
    /// `ETag`.
    pub etag: String,
    /// Last modified timestamp (ISO 8601).
    pub last_modified: String,
}

/// Upload options.
#[napi(object)]
#[derive(Debug, Clone)]
pub struct UploadOptions {
    /// Local file/directory paths to upload.
    pub sources: Vec<String>,
    /// Target S3 bucket.
    pub bucket: String,
    /// Key prefix in the bucket.
    pub prefix: String,
    /// Number of files uploaded in parallel (default 32).
    pub workers: Option<u32>,
    /// Parts per file concurrency (default 8).
    pub concurrency_per_file: Option<u32>,
}

/// Download options.
#[napi(object)]
#[derive(Debug, Clone)]
pub struct DownloadOptions {
    /// Source S3 bucket.
    pub bucket: String,
    /// Key prefix.
    pub prefix: String,
    /// Local destination directory.
    pub dest_dir: String,
    /// Number of files downloaded in parallel (default 32).
    pub workers: Option<u32>,
    /// Parts per file concurrency (default 8).
    pub concurrency_per_file: Option<u32>,
}

/// List options.
#[napi(object)]
#[derive(Debug, Clone)]
pub struct ListOptions {
    /// S3 bucket.
    pub bucket: String,
    /// Key prefix.
    pub prefix: String,
    /// Optional delimiter for directory grouping.
    pub delimiter: Option<String>,
}

/// The s3z S3 client.
#[napi]
pub struct S3Client {
    inner: s3z::S3Client,
}

#[napi]
impl S3Client {
    /// Create a new S3 client.
    ///
    /// # Errors
    ///
    /// Returns an error if credential resolution or HTTP client setup fails.
    #[napi(factory)]
    pub async fn create(config: S3Config) -> Result<S3Client> {
        let cred_source = match (config.access_key, config.secret_key) {
            (Some(ak), Some(sk)) => {
                s3z::auth::CredentialSource::Static {
                    access_key: ak,
                    secret_key: sk,
                }
            },
            _ => s3z::auth::CredentialSource::Env,
        };

        let core_config = if let Some(ep) = config.endpoint {
            s3z::Config::with_endpoint(config.region, cred_source, ep)
        } else {
            s3z::Config::new(config.region, cred_source)
        };

        let client =
            block_on_async(s3z::S3Client::new(core_config)).map_err(|ref e| to_napi_err(e))?;

        Ok(Self {
            inner: client,
        })
    }

    /// Upload files/directories to S3.
    ///
    /// # Errors
    ///
    /// Returns an error if any file upload or source expansion fails.
    #[napi]
    pub async fn upload(&self, options: UploadOptions) -> Result<Vec<FileUploadResult>> {
        let paths: Vec<PathBuf> = options.sources.into_iter().map(PathBuf::from).collect();
        let mut req = s3z::UploadRequest::new(paths, options.bucket, options.prefix);
        if let Some(w) = options.workers {
            req.workers = nonzero("workers", w)?;
        }
        if let Some(c) = options.concurrency_per_file {
            req.concurrency_per_file = nonzero("concurrency_per_file", c)?;
        }

        let client = self.inner.clone();
        let result = block_on_async(async move { client.upload(req).await })
            .map_err(|ref e| to_napi_err(e))?;

        Ok(result
            .files
            .into_iter()
            .map(|f| {
                FileUploadResult {
                    etag: f.etag,
                    key: f.key,
                    parts: f.parts,
                    size: size_to_js(f.size),
                    source: f.source.to_string_lossy().into_owned(),
                }
            })
            .collect())
    }

    /// Download objects under a prefix to a local directory.
    ///
    /// # Errors
    ///
    /// Returns an error if listing, any download, or file I/O fails.
    #[napi]
    pub async fn download(&self, options: DownloadOptions) -> Result<Vec<FileDownloadResult>> {
        let mut req = s3z::DownloadRequest::new(options.bucket, options.prefix, options.dest_dir);
        if let Some(w) = options.workers {
            req.workers = nonzero("workers", w)?;
        }
        if let Some(c) = options.concurrency_per_file {
            req.concurrency_per_file = nonzero("concurrency_per_file", c)?;
        }

        let client = self.inner.clone();
        let result = block_on_async(async move { client.download(req).await })
            .map_err(|ref e| to_napi_err(e))?;

        Ok(result
            .files
            .into_iter()
            .map(|f| {
                FileDownloadResult {
                    dest: f.dest.to_string_lossy().into_owned(),
                    key: f.key,
                    parts: f.parts,
                    size: size_to_js(f.size),
                }
            })
            .collect())
    }

    /// List all objects under a prefix.
    ///
    /// # Errors
    ///
    /// Returns an error if any listing page fetch fails.
    #[napi]
    pub async fn list(&self, options: ListOptions) -> Result<Vec<ObjectInfo>> {
        let mut req = s3z::ListRequest::new(options.bucket, options.prefix);
        if let Some(d) = options.delimiter {
            req = req.with_delimiter(d);
        }

        let client = self.inner.clone();
        let objects = block_on_async(async move { client.list(req).collect_all().await })
            .map_err(|ref e| to_napi_err(e))?;

        Ok(objects
            .into_iter()
            .map(|o| {
                ObjectInfo {
                    key: o.key,
                    size: size_to_js(o.size),
                    etag: o.etag,
                    last_modified: o.last_modified,
                }
            })
            .collect())
    }
}
