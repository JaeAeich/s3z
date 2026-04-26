//! Download subcommand implementation.

use core::time::Duration;
use std::{sync::Arc, time::Instant};

use indicatif::{ProgressBar, ProgressStyle};
use s3z::{DownloadRequest, DownloadResult, S3Client, tune_parallelism};

use crate::{cli::DownloadArgs, fmt};

/// Execute the download subcommand.
pub(crate) async fn run(client: &S3Client, args: &DownloadArgs, quiet: bool) -> anyhow::Result<()> {
    let mut req = DownloadRequest::new(&args.bucket, &args.prefix, &args.dest);

    if quiet {
        // When quiet + explicit args: honour them; otherwise let
        // S3Client::download auto-tune internally.
        if let Some(w) = args.workers {
            req.workers = w;
        }
        if let Some(c) = args.concurrency {
            req.concurrency_per_file = c;
        }
        client.download(req).await?;
    } else {
        // List once, use the results for both the progress bar and download.
        let mut paginator = client.list(s3z::ListRequest::new(&args.bucket, &args.prefix));
        let objects = paginator.collect_all().await?;
        let total_files = objects.len();

        // Auto-tune unless the user explicitly set workers/concurrency.
        let tuned = tune_parallelism(&objects, client.config().transfer.multipart_threshold);
        req.workers = args.workers.unwrap_or(tuned.workers);
        req.concurrency_per_file = args.concurrency.unwrap_or(tuned.concurrency_per_file);

        let pb = Arc::new(make_progress_bar(total_files));
        let pb_cb = Arc::clone(&pb);

        req = req.on_file_complete(move |f| {
            pb_cb.println(format!(
                "  \u{2713} {} ({}, {} parts)",
                f.key,
                fmt::bytes(f.size),
                f.parts,
            ));
            pb_cb.inc(1);
        });

        let start = Instant::now();
        let result = client.download_objects(req, objects).await?;
        let elapsed = start.elapsed();

        pb.finish_and_clear();
        print_summary(&result, &args.dest.display().to_string(), elapsed);
    }

    Ok(())
}

#[expect(clippy::as_conversions, reason = "file count fits in u64")]
fn make_progress_bar(total: usize) -> ProgressBar {
    let pb = ProgressBar::new(total as u64);
    let template =
        concat!("  [{bar:30}]  {pos}/{len} files", "  |  {elapsed_precise}", "  |  {msg}",);
    #[expect(clippy::expect_used, reason = "static template string is always valid")]
    let style = ProgressStyle::default_bar()
        .template(template)
        .expect("valid template")
        .progress_chars("=>-");
    pb.set_style(style);
    pb.set_message("downloading...");
    pb
}

fn print_summary(result: &DownloadResult, dest: &str, elapsed: Duration) {
    let total_bytes: u64 = result.files.iter().map(|f| f.size).sum();
    let total_parts: u32 = result.files.iter().map(|f| f.parts).sum();

    println!();
    println!(
        "{} file(s) -> {} | {} | {} part(s) | {:.2}s ({})",
        result.files.len(),
        dest,
        fmt::bytes(total_bytes),
        total_parts,
        elapsed.as_secs_f64(),
        fmt::throughput(total_bytes, elapsed.as_secs_f64()),
    );
}
