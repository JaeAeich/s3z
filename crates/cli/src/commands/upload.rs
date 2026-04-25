//! Upload subcommand implementation.

use core::time::Duration;
use std::{sync::Arc, time::Instant};

use indicatif::{ProgressBar, ProgressStyle};
use s3z::{S3Client, UploadRequest, UploadResult};

use crate::{cli::UploadArgs, fmt};

/// Execute the upload subcommand.
pub(crate) async fn run(client: &S3Client, args: &UploadArgs, quiet: bool) -> anyhow::Result<()> {
    let mut req = UploadRequest::new(args.sources.clone(), &args.bucket, &args.prefix);
    req.workers = args.workers;
    req.concurrency_per_file = args.concurrency;

    let total_files = req.file_count()?;

    if quiet {
        client.upload(req).await?;
    } else {
        let pb = Arc::new(make_progress_bar(total_files));
        let pb_cb = Arc::clone(&pb);

        req = req.on_file_complete(move |f| {
            pb_cb.println(format!(
                "  \u{2713} {} ({}, {} parts)",
                f.source.display(),
                fmt::bytes(f.size),
                f.parts,
            ));
            pb_cb.inc(1);
        });

        let start = Instant::now();
        let result = client.upload(req).await?;
        let elapsed = start.elapsed();

        pb.finish_and_clear();
        print_summary(&result, &args.bucket, elapsed);
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
    pb.set_message("uploading...");
    pb
}

fn print_summary(result: &UploadResult, bucket: &str, elapsed: Duration) {
    let total_bytes: u64 = result.files.iter().map(|f| f.size).sum();
    let total_parts: u32 = result.files.iter().map(|f| f.parts).sum();

    println!();
    println!(
        "{} file(s) -> s3://{} | {} | {} part(s) | {:.2}s ({})",
        result.files.len(),
        bucket,
        fmt::bytes(total_bytes),
        total_parts,
        elapsed.as_secs_f64(),
        fmt::throughput(total_bytes, elapsed.as_secs_f64()),
    );
}
