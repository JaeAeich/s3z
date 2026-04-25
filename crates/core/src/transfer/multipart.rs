//! Multipart upload orchestration.
//!
//! Uses a producer-consumer pattern: one reader task reads parts
//! sequentially from a single file handle into buffers, N upload workers
//! pull from a bounded channel and upload in parallel. The channel acts
//! as backpressure — peak memory is `concurrency * part_size`.

use core::fmt::Write as _;
use std::{path::Path, sync::Arc};

use bytes::Bytes;
use http::{Method, Uri};
use tokio::{
    fs::File,
    io::AsyncReadExt as _,
    runtime::Handle,
    sync::mpsc,
    task::{self, JoinSet},
};

use crate::{
    auth::Credentials,
    config::Config,
    error::{Error, Result},
    http::{
        ObjectKey,
        request::{build_signed, build_signed_unsigned_payload},
        response,
        retry::{send_with_retry, send_with_retry_bytes},
    },
    trace::{maybe_debug, maybe_info},
    transfer::part::{Part, PartResult},
};

/// Context for a single multipart upload.
struct UploadCtx {
    bucket: String,
    config: Config,
    creds: Credentials,
    http: reqwest::Client,
    key: ObjectKey,
    upload_id: String,
}

/// A part that has been read from disk and is ready to upload.
struct ReadyPart {
    data: Bytes,
    number: u32,
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
                drop(
                    abort(
                        &ctx.http,
                        &ctx.config,
                        &ctx.creds,
                        &ctx.bucket,
                        &ctx.key,
                        &ctx.upload_id,
                    )
                    .await,
                );
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

/// Upload all parts using a producer-consumer pipeline.
///
/// One reader task reads parts sequentially from a single file handle
/// and sends them over a bounded channel. N upload workers pull from
/// the channel and upload in parallel.
/// - Sequential disk reads avoid file handle thrashing
/// - Bounded channel (capacity = concurrency) acts as backpressure
/// - Peak memory: `concurrency * part_size`
async fn upload_all_parts(
    ctx: &Arc<UploadCtx>, parts: &[Part], file_path: &Path, concurrency: usize,
) -> Result<Vec<PartResult>> {
    let (tx, rx) = mpsc::channel::<ReadyPart>(concurrency);

    let reader_parts: Vec<Part> = parts.to_vec();
    let reader_path = file_path.to_owned();
    let reader_handle =
        task::spawn(async move { read_parts(reader_parts, &reader_path, tx).await });

    let upload_result = run_upload_workers(ctx, rx, concurrency, parts.len()).await;

    let reader_result = reader_handle.await.map_err(|e| Error::Internal(e.to_string()))?;

    match (upload_result, reader_result) {
        (Ok(results), Ok(())) => Ok(results),
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

/// Reader task: opens the file once, reads each part sequentially into a
/// buffer, sends it through the channel.
async fn read_parts(parts: Vec<Part>, file_path: &Path, tx: mpsc::Sender<ReadyPart>) -> Result<()> {
    let mut file = File::open(file_path).await?;

    for part in &parts {
        let buf_size = usize::try_from(part.size).map_err(|e| Error::Conversion(e.to_string()))?;
        let mut buf = vec![0_u8; buf_size];
        file.read_exact(&mut buf).await?;

        let ready = ReadyPart {
            data: Bytes::from(buf),
            number: part.number,
            size: part.size,
        };

        if tx.send(ready).await.is_err() {
            break;
        }
    }

    Ok(())
}

/// Spawn N upload workers, collect results.
async fn run_upload_workers(
    ctx: &Arc<UploadCtx>, mut rx: mpsc::Receiver<ReadyPart>, concurrency: usize, total_parts: usize,
) -> Result<Vec<PartResult>> {
    let mut set = JoinSet::new();
    let mut results = Vec::with_capacity(total_parts);
    let mut spawned = 0_usize;

    while spawned < concurrency {
        match rx.recv().await {
            Some(ready) => {
                let c = Arc::clone(ctx);
                set.spawn(async move { upload_part_from_buffer(&c, ready).await });
                spawned = spawned.saturating_add(1);
            },
            None => break,
        }
    }

    loop {
        if set.is_empty() {
            break;
        }

        let Some(handle) = set.join_next().await else {
            break;
        };

        match handle.map_err(|e| Error::Internal(e.to_string()))? {
            Ok(result) => results.push(result),
            Err(e) => {
                rx.close();
                set.abort_all();
                return Err(e);
            },
        }

        if let Ok(ready) = rx.try_recv() {
            let c = Arc::clone(ctx);
            set.spawn(async move { upload_part_from_buffer(&c, ready).await });
        }
    }

    Ok(results)
}

/// Upload a single part from an already-read buffer.
async fn upload_part_from_buffer(ctx: &UploadCtx, ready: ReadyPart) -> Result<PartResult> {
    let part_number = ready.number;
    let upload_id = &ctx.upload_id;
    let uri: Uri = format!(
        "{}/{}/{}?partNumber={part_number}&uploadId={upload_id}",
        ctx.config.endpoint_url(),
        ctx.bucket,
        ctx.key.encoded(),
    )
    .parse()?;

    maybe_debug!(key = %ctx.key, part_number, size = ready.size, "uploading part");

    #[cfg(feature = "tracing")]
    let part_start = std::time::Instant::now();

    let req = build_signed_unsigned_payload(
        Method::PUT,
        uri,
        ready.size,
        &ctx.creds,
        &ctx.config.region,
    )?;
    let resp = send_with_retry_bytes(&ctx.http, req, ready.data, &ctx.config.retry).await?;

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
