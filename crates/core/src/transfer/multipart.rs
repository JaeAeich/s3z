//! Multipart upload orchestration.
//!
//! Uses a producer-consumer pattern: one task schedules part metadata
//! through a bounded channel, N upload workers each open the file,
//! seek to their part offset, and stream the bytes directly to S3.
//! The channel bounds parallelism; no large buffers flow through it.
//!
//! Peak memory per file: `concurrency * STREAM_BUFFER_SIZE` (~256 KiB each).

use core::fmt::Write as _;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use bytes::Bytes;
use http::{Method, Uri};
use tokio::{
    fs::File,
    io::{AsyncReadExt as _, AsyncSeekExt as _},
    runtime::Handle,
    sync::mpsc,
    task::{self, JoinSet},
};
use tokio_util::io::ReaderStream;

use crate::{
    auth::Credentials,
    config::Config,
    error::{Error, Result},
    http::{
        ObjectKey,
        request::{build_signed, build_signed_unsigned_payload},
        response,
        retry::{send_with_retry, send_with_retry_stream},
    },
    trace::{maybe_debug, maybe_info, maybe_warn},
    transfer::part::{Part, PartResult},
};

/// Buffer size for the per-part `ReaderStream` (256 KiB).
const STREAM_BUFFER_SIZE: usize = 256 * 1024;

/// Context for a single multipart upload.
struct UploadCtx {
    bucket: String,
    config: Config,
    creds: Credentials,
    http: reqwest::Client,
    key: ObjectKey,
    upload_id: String,
}

/// Part metadata sent through the scheduling channel (no bytes).
struct PartJob {
    file_path: Arc<PathBuf>,
    number: u32,
    offset: u64,
    size: u64,
}

/// Drop guard that aborts the multipart upload on S3 if not disarmed.
struct AbortGuard {
    ctx: Option<Arc<UploadCtx>>,
}

impl AbortGuard {
    fn disarm(&mut self) {
        self.ctx = None;
    }

    const fn new(ctx: Arc<UploadCtx>) -> Self {
        Self {
            ctx: Some(ctx),
        }
    }
}

impl Drop for AbortGuard {
    fn drop(&mut self) {
        if let Some(ctx) = self.ctx.take() {
            let Ok(handle) = Handle::try_current() else {
                return;
            };
            drop(handle.spawn(async move {
                if let Err(_e) =
                    abort(&ctx.http, &ctx.config, &ctx.creds, &ctx.bucket, &ctx.key, &ctx.upload_id)
                        .await
                {
                    maybe_warn!(
                        upload_id = %ctx.upload_id,
                        key = %ctx.key,
                        bucket = %ctx.bucket,
                        "failed to abort multipart upload — may leak storage"
                    );
                }
            }));
        }
    }
}

/// `DELETE /{bucket}/{key}?uploadId=X` — abort a multipart upload.
async fn abort(
    http: &reqwest::Client, config: &Config, creds: &Credentials, bucket: &str, key: &ObjectKey,
    upload_id: &str,
) -> Result<()> {
    let uri: Uri =
        format!("{}/{bucket}/{}?uploadId={upload_id}", config.endpoint_url(), key.encoded(),)
            .parse()?;

    let req = build_signed(Method::DELETE, uri, Bytes::new(), creds, &config.region)?;
    let _resp = send_with_retry(http, req, &config.retry).await;
    Ok(())
}

/// `POST /{bucket}/{key}?uploadId=X` with XML body — complete multipart.
async fn complete(
    http: &reqwest::Client, config: &Config, creds: &Credentials, bucket: &str, key: &ObjectKey,
    upload_id: &str, results: &mut [PartResult],
) -> Result<String> {
    results.sort_by_key(|r| r.number);

    let mut xml = String::from("<CompleteMultipartUpload>");
    for r in results.iter() {
        #[expect(clippy::expect_used, reason = "write! to String is infallible")]
        write!(xml, "<Part><PartNumber>{}</PartNumber><ETag>{}</ETag></Part>", r.number, r.etag)
            .expect("write to String is infallible");
    }
    xml.push_str("</CompleteMultipartUpload>");

    let uri: Uri =
        format!("{}/{bucket}/{}?uploadId={upload_id}", config.endpoint_url(), key.encoded(),)
            .parse()?;

    let req = build_signed(Method::POST, uri, Bytes::from(xml), creds, &config.region)?;
    let resp = send_with_retry(http, req, &config.retry).await?;
    let body = resp.text().await?;
    Ok(response::parse_complete_multipart(&body)?.etag)
}

/// `POST /{bucket}/{key}?uploads` — initiate multipart upload.
async fn initiate(
    http: &reqwest::Client, config: &Config, creds: &Credentials, bucket: &str, key: &ObjectKey,
) -> Result<String> {
    let uri: Uri =
        format!("{}/{bucket}/{}?uploads", config.endpoint_url(), key.encoded()).parse()?;

    let req = build_signed(Method::POST, uri, Bytes::new(), creds, &config.region)?;
    let resp = send_with_retry(http, req, &config.retry).await?;
    let body = resp.text().await?;
    Ok(response::parse_initiate_multipart(&body)?.upload_id)
}

/// Execute a full multipart upload for one file.
///
/// # Errors
///
/// Returns an error if initiation, any part upload, or completion fails.
#[expect(clippy::too_many_arguments, reason = "internal fn, context struct would add indirection")]
pub(crate) async fn upload_multipart(
    http: &reqwest::Client, config: &Config, creds: &Credentials, bucket: &str, key: &ObjectKey,
    parts: &[Part], file_path: &Path, concurrency: usize,
) -> Result<(String, u32)> {
    #[cfg(feature = "tracing")]
    let start = std::time::Instant::now();

    let uid = initiate(http, config, creds, bucket, key).await?;
    maybe_info!(
        key = %key, upload_id = %uid, parts = parts.len(), concurrency, "multipart initiated"
    );

    let ctx = Arc::new(UploadCtx {
        bucket: bucket.to_owned(),
        config: config.clone(),
        creds: creds.clone(),
        http: http.clone(),
        key: key.clone(),
        upload_id: uid.clone(),
    });

    let mut guard = AbortGuard::new(Arc::clone(&ctx));
    let result = upload_all_parts(&ctx, parts, file_path, concurrency).await;

    match result {
        Ok(mut results) => {
            #[cfg(feature = "tracing")]
            let complete_start = std::time::Instant::now();

            let etag = complete(http, config, creds, bucket, key, &uid, &mut results).await?;

            #[cfg(feature = "tracing")]
            let complete_elapsed = complete_start.elapsed();

            let parts_count =
                u32::try_from(results.len()).map_err(|e| Error::Conversion(e.to_string()))?;
            guard.disarm();

            #[cfg(feature = "tracing")]
            let total_elapsed = start.elapsed();

            maybe_info!(
                key = %key,
                parts = parts_count,
                ?complete_elapsed,
                ?total_elapsed,
                "multipart complete"
            );
            Ok((etag, parts_count))
        },
        Err(e) => Err(e),
    }
}

/// Schedule and upload all parts using a producer-consumer pipeline.
///
/// One task sends part metadata (offset, size — no bytes) over a bounded
/// channel. N upload workers each open the file, seek, and stream their
/// part directly to S3.
///
/// - Bounded channel (capacity = concurrency) limits in-flight parts
/// - Peak memory per file: `concurrency * STREAM_BUFFER_SIZE`
async fn upload_all_parts(
    ctx: &Arc<UploadCtx>, parts: &[Part], file_path: &Path, concurrency: usize,
) -> Result<Vec<PartResult>> {
    let (tx, rx) = mpsc::channel::<PartJob>(concurrency);

    let shared_path = Arc::new(file_path.to_owned());
    let parts_owned: Vec<Part> = parts.to_vec();
    let path_for_scheduler = Arc::clone(&shared_path);

    let scheduler_handle = task::spawn(async move {
        schedule_parts(parts_owned, path_for_scheduler, tx).await;
    });

    let upload_result = run_upload_workers(ctx, rx, concurrency, parts.len()).await;

    // Wait for the scheduler to finish (it's infallible — just sending metadata).
    drop(scheduler_handle.await);

    upload_result
}

/// Scheduler task: sends part metadata through the channel. No I/O.
async fn schedule_parts(parts: Vec<Part>, file_path: Arc<PathBuf>, tx: mpsc::Sender<PartJob>) {
    for part in &parts {
        let job = PartJob {
            file_path: Arc::clone(&file_path),
            number: part.number,
            offset: part.offset,
            size: part.size,
        };

        if tx.send(job).await.is_err() {
            break;
        }
    }
}

/// Spawn N upload workers, collect results.
///
/// Uses `tokio::select!` to simultaneously wait for completed uploads and
/// new jobs from the scheduler, keeping exactly `concurrency` tasks in
/// flight at all times.
async fn run_upload_workers(
    ctx: &Arc<UploadCtx>, mut rx: mpsc::Receiver<PartJob>, concurrency: usize, total_parts: usize,
) -> Result<Vec<PartResult>> {
    let mut set = JoinSet::new();
    let mut results = Vec::with_capacity(total_parts);
    let mut channel_open = true;

    loop {
        // Spawn up to `concurrency` tasks while jobs are available.
        while channel_open && set.len() < concurrency {
            match rx.recv().await {
                Some(job) => {
                    let c = Arc::clone(ctx);
                    set.spawn(async move { upload_part_streaming(&c, job).await });
                },
                None => {
                    channel_open = false;
                },
            }
        }

        if set.is_empty() {
            break;
        }

        // Wait for a completed upload, and optionally receive a new job.
        tokio::select! {
            Some(handle) = set.join_next() => {
                match handle.map_err(|e| Error::Internal(e.to_string()))? {
                    Ok(result) => results.push(result),
                    Err(e) => {
                        rx.close();
                        set.abort_all();
                        return Err(e);
                    },
                }
            }
            Some(job) = rx.recv(), if channel_open && set.len() >= concurrency => {
                let c = Arc::clone(ctx);
                set.spawn(async move { upload_part_streaming(&c, job).await });
            }
            else => break,
        }
    }

    Ok(results)
}

/// Open a file, seek to offset, return a `reqwest::Body` that streams
/// `size` bytes through a 256 KiB buffer.
async fn make_part_body(path: &Path, offset: u64, size: u64) -> Result<reqwest::Body> {
    let mut file = File::open(path).await?;
    if offset > 0 {
        file.seek(std::io::SeekFrom::Start(offset)).await?;
    }
    let limited = file.take(size);
    let stream = ReaderStream::with_capacity(limited, STREAM_BUFFER_SIZE);
    Ok(reqwest::Body::wrap_stream(stream))
}

/// Upload a single part by streaming from disk.
async fn upload_part_streaming(ctx: &UploadCtx, job: PartJob) -> Result<PartResult> {
    let part_number = job.number;
    let upload_id = &ctx.upload_id;
    let uri: Uri = format!(
        "{}/{}/{}?partNumber={part_number}&uploadId={upload_id}",
        ctx.config.endpoint_url(),
        ctx.bucket,
        ctx.key.encoded(),
    )
    .parse()?;

    maybe_debug!(key = %ctx.key, part_number, size = job.size, "uploading part");

    #[cfg(feature = "tracing")]
    let part_start = std::time::Instant::now();

    let content_length = job.size;
    let creds = ctx.creds.clone();
    let region = ctx.config.region.clone();

    let path = Arc::clone(&job.file_path);
    let offset = job.offset;
    let size = job.size;

    let resp = send_with_retry_stream(
        &ctx.http,
        || {
            let u = uri.clone();
            let c = creds.clone();
            let r = region.clone();
            async move { build_signed_unsigned_payload(Method::PUT, u, content_length, &c, &r) }
        },
        || {
            let p = Arc::clone(&path);
            async move { make_part_body(&p, offset, size).await }
        },
        &ctx.config.retry,
    )
    .await?;

    #[cfg(feature = "tracing")]
    let part_elapsed = part_start.elapsed();

    maybe_debug!(key = %ctx.key, part_number, ?part_elapsed, "part uploaded");

    let etag = resp
        .headers()
        .get("etag")
        .ok_or_else(|| {
            Error::S3 {
                code: "MissingETag".into(),
                message: "upload part response missing ETag header".into(),
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

    Ok(PartResult {
        etag,
        number: part_number,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn tempfile(name: &str, data: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("s3z_mp_{name}_{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("data.bin");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(data).unwrap();
        path
    }

    /// Read a file segment the same way `make_part_body` does, but collect
    /// the bytes directly instead of wrapping in a `reqwest::Body`.
    async fn read_segment(path: &Path, offset: u64, size: u64) -> Vec<u8> {
        let mut file = File::open(path).await.unwrap();
        if offset > 0 {
            file.seek(std::io::SeekFrom::Start(offset)).await.unwrap();
        }
        let mut limited = file.take(size);
        let mut buf = Vec::new();
        limited.read_to_end(&mut buf).await.unwrap();
        buf
    }

    #[tokio::test]
    async fn make_part_body_reads_from_start() {
        let data = b"hello world";
        let path = tempfile("start", data);
        let bytes = read_segment(&path, 0, 5).await;
        assert_eq!(&bytes, b"hello");
    }

    #[tokio::test]
    async fn make_part_body_reads_with_offset() {
        let data = b"hello world";
        let path = tempfile("offset", data);
        let bytes = read_segment(&path, 6, 5).await;
        assert_eq!(&bytes, b"world");
    }

    #[tokio::test]
    async fn make_part_body_size_larger_than_remaining() {
        let data = b"short";
        let path = tempfile("larger", data);
        let bytes = read_segment(&path, 3, 100).await;
        assert_eq!(&bytes, b"rt");
    }

    #[tokio::test]
    async fn make_part_body_zero_offset_full_file() {
        let data: Vec<u8> = (0_u8..=255).cycle().take(1024).collect();
        let path = tempfile("full", &data);
        let bytes = read_segment(&path, 0, 1024).await;
        assert_eq!(bytes, data);
    }

    #[tokio::test]
    async fn make_part_body_nonexistent_file_errors() {
        let result = make_part_body(Path::new("/nonexistent_s3z_test_file"), 0, 10).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn make_part_body_returns_body_successfully() {
        let data = b"test data for body creation";
        let path = tempfile("body_ok", data);
        // Verify make_part_body itself doesn't error
        let body = make_part_body(&path, 0, 10).await;
        assert!(body.is_ok());
    }
}
