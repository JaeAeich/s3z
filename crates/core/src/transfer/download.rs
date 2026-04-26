//! Download transfer — single GET and Range-based multipart download.
//!
//! Small files are fetched with a single GET and streamed to disk.
//! Large files are split into Range requests that write concurrently
//! to different offsets of the same file. Peak memory per part is
//! bounded by the streaming buffer size (256 KiB).
//!
//! Temp files use [`tempfile::NamedTempFile`] for crash-safe cleanup:
//! if the process is killed mid-download, the OS reclaims the temp
//! file instead of leaving orphaned `.s3z.part` files on disk.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use bytes::Bytes;
use futures_util::StreamExt as _;
use http::{Method, Uri};
use tokio::{
    fs,
    io::{AsyncSeekExt as _, AsyncWriteExt as _},
    sync::mpsc,
    task::{self, JoinSet},
};

use crate::{
    auth::Credentials,
    config::Config,
    error::{Error, Result},
    http::{ObjectKey, request::build_signed, retry::send_with_retry},
    trace::{maybe_debug, maybe_info},
    transfer::part::Part,
};

/// Download a single object with a plain GET — streams to a temp file,
/// then atomically renames to the final path. The temp file is
/// automatically cleaned up on error or process crash.
///
/// # Errors
///
/// Returns an error if the HTTP request, body streaming, or file I/O fails.
pub(crate) async fn download_single(
    http: &reqwest::Client, config: &Config, creds: &Credentials, bucket: &str, key: &ObjectKey,
    dest: &Path,
) -> Result<u64> {
    let uri: Uri = format!("{}/{bucket}/{}", config.endpoint_url(), key.encoded()).parse()?;

    let req = build_signed(Method::GET, uri, Bytes::new(), creds, &config.region)?;
    let resp = send_with_retry(http, req, &config.retry).await?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).await?;
    }

    let tmp = task::spawn_blocking({
        let dir = dest.parent().unwrap_or_else(|| Path::new(".")).to_owned();
        move || tempfile::NamedTempFile::new_in(dir)
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;

    // reopen() gives an independent fd to the same file. We wrap it in
    // tokio::fs::File for async I/O while the NamedTempFile guard stays
    // alive for cleanup-on-drop.
    let std_file = tmp.reopen()?;
    let mut file = fs::File::from_std(std_file);
    let mut size = 0_u64;
    let mut stream = resp.bytes_stream();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "download size bounded by S3 object size"
        )]
        {
            size += u64::try_from(chunk.len()).unwrap_or(u64::MAX);
        }
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);

    // Atomic rename — consumes the NamedTempFile so it won't delete-on-drop.
    let dest_owned = dest.to_owned();
    task::spawn_blocking(move || tmp.persist(dest_owned).map_err(|e| Error::Io(e.error)))
        .await
        .map_err(|e| Error::Internal(e.to_string()))??;

    Ok(size)
}

/// Context for a multipart download.
struct DownloadCtx {
    bucket: String,
    config: Config,
    creds: Credentials,
    http: reqwest::Client,
    key: ObjectKey,
}

/// Part job metadata for the download scheduler.
struct DownloadPartJob {
    dest: Arc<PathBuf>,
    #[cfg_attr(not(feature = "tracing"), expect(dead_code, reason = "read by maybe_debug!"))]
    number: u32,
    offset: u64,
    size: u64,
}

/// Download a single object using concurrent Range requests.
///
/// Each part writes to the correct offset in the destination file.
/// The file is created at full size first, then parts fill it concurrently.
/// Uses [`tempfile::NamedTempFile`] for crash-safe cleanup.
///
/// # Errors
///
/// Returns an error if any Range request or file I/O fails.
#[expect(clippy::too_many_arguments, reason = "internal fn, context struct would add indirection")]
pub(crate) async fn download_multipart(
    http: &reqwest::Client, config: &Config, creds: &Credentials, bucket: &str, key: &ObjectKey,
    parts: &[Part], dest: &Path, total_size: u64, concurrency: usize,
) -> Result<u64> {
    assert!(concurrency > 0, "concurrency must be at least 1");

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).await?;
    }

    let tmp = task::spawn_blocking({
        let dir = dest.parent().unwrap_or_else(|| Path::new(".")).to_owned();
        move || tempfile::NamedTempFile::new_in(dir)
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;

    // Workers need the path to open independent fds for concurrent writes.
    let tmp_path = tmp.path().to_owned();

    // Pre-allocate via reopen() so we use the same underlying file, not
    // a fresh create that would disconnect from the NamedTempFile guard.
    let std_file = tmp.reopen()?;
    let file = fs::File::from_std(std_file);
    file.set_len(total_size).await?;
    drop(file);

    maybe_info!(
        key = %key, parts = parts.len(), concurrency, size = total_size,
        "multipart download started"
    );

    let ctx = Arc::new(DownloadCtx {
        bucket: bucket.to_owned(),
        config: config.clone(),
        creds: creds.clone(),
        http: http.clone(),
        key: key.clone(),
    });

    let result = download_all_parts(&ctx, parts, &tmp_path, concurrency).await;

    match result {
        Ok(()) => {
            let dest = dest.to_owned();
            task::spawn_blocking(move || tmp.persist(dest).map_err(|e| Error::Io(e.error)))
                .await
                .map_err(|e| Error::Internal(e.to_string()))??;
            maybe_info!(key = %key, size = total_size, "multipart download complete");
            Ok(total_size)
        },
        Err(e) => {
            // NamedTempFile drops here → auto-deletes the temp file.
            drop(tmp);
            Err(e)
        },
    }
}

/// Schedule and download all parts using a producer-consumer pipeline.
async fn download_all_parts(
    ctx: &Arc<DownloadCtx>, parts: &[Part], dest: &Path, concurrency: usize,
) -> Result<()> {
    let (tx, rx) = mpsc::channel::<DownloadPartJob>(concurrency);

    let shared_dest = Arc::new(dest.to_owned());
    let parts_owned: Vec<Part> = parts.to_vec();
    let dest_for_scheduler = Arc::clone(&shared_dest);

    let scheduler_handle = task::spawn(async move {
        for part in &parts_owned {
            let job = DownloadPartJob {
                dest: Arc::clone(&dest_for_scheduler),
                number: part.number,
                offset: part.offset,
                size: part.size,
            };
            if tx.send(job).await.is_err() {
                break;
            }
        }
    });

    let download_result = run_download_workers(ctx, rx, concurrency).await;
    drop(scheduler_handle.await);
    download_result
}

/// Spawn N download workers, collect results.
async fn run_download_workers(
    ctx: &Arc<DownloadCtx>, mut rx: mpsc::Receiver<DownloadPartJob>, concurrency: usize,
) -> Result<()> {
    let mut set = JoinSet::new();
    let mut channel_open = true;

    loop {
        if set.is_empty() && !channel_open {
            break;
        }

        let has_capacity = channel_open && set.len() < concurrency;

        tokio::select! {
            Some(handle) = set.join_next() => {
                match handle.map_err(|e| Error::Internal(e.to_string()))? {
                    Ok(()) => {},
                    Err(e) => {
                        rx.close();
                        set.abort_all();
                        return Err(e);
                    },
                }
            }
            job = rx.recv(), if has_capacity => {
                match job {
                    Some(job) => {
                        let c = Arc::clone(ctx);
                        set.spawn(async move { download_part(&c, job).await });
                    },
                    None => { channel_open = false; },
                }
            }
            else => break,
        }
    }

    Ok(())
}

/// Download a single Range part and write it to the correct file offset.
///
/// # Safety invariant (not `unsafe`, but correctness-critical)
///
/// Each task opens its own file descriptor via `File::options().open()`.
/// On Unix this creates an independent kernel file-description with its own
/// offset, so concurrent seek+write from different tasks cannot interfere.
/// Do **not** refactor this to share a single `File` handle across tasks.
async fn download_part(ctx: &DownloadCtx, job: DownloadPartJob) -> Result<()> {
    debug_assert!(job.size > 0, "zero-size part would underflow range calculation");
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "offset + size bounded by file size; size > 0 guaranteed by plan_parts"
    )]
    let range_end = job.offset + job.size - 1;
    let range_header = format!("bytes={}-{range_end}", job.offset);

    let uri: Uri =
        format!("{}/{}/{}", ctx.config.endpoint_url(), ctx.bucket, ctx.key.encoded()).parse()?;

    maybe_debug!(key = %ctx.key, part = job.number, offset = job.offset, size = job.size, "downloading part");

    let mut signed = build_signed(Method::GET, uri, Bytes::new(), &ctx.creds, &ctx.config.region)?;
    signed.headers_mut().insert(
        "range",
        range_header
            .parse()
            .map_err(|e: http::header::InvalidHeaderValue| Error::Internal(e.to_string()))?,
    );

    let resp = send_with_retry(&ctx.http, signed, &ctx.config.retry).await?;

    let mut file = fs::File::options().write(true).open(&*job.dest).await?;
    file.seek(std::io::SeekFrom::Start(job.offset)).await?;

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
    }
    file.flush().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn tempfile_creates_in_target_dir() {
        let dir = std::env::temp_dir();
        let tmp = tempfile::NamedTempFile::new_in(&dir).unwrap();
        assert!(tmp.path().starts_with(&dir));
    }
}
