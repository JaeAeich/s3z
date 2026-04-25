//! Upload subcommand implementation.

use std::time::Instant;

use s3z::{S3Client, UploadRequest};

use crate::{cli::UploadArgs, fmt};

/// Execute the upload subcommand.
pub(crate) async fn run(client: &S3Client, args: &UploadArgs, quiet: bool) -> anyhow::Result<()> {
    let mut req = UploadRequest::new(args.sources.clone(), &args.bucket, &args.prefix);
    req.workers = args.workers;
    req.concurrency_per_file = args.concurrency;

    let start = Instant::now();
    let result = client.upload(req).await?;
    let elapsed = start.elapsed();

    if !quiet {
        for f in &result.files {
            println!(
                "  {} -> s3://{}/{} ({}, {} parts)",
                f.source.display(),
                args.bucket,
                f.key,
                fmt::bytes(f.size),
                f.parts,
            );
        }

        let total_bytes: u64 = result.files.iter().map(|f| f.size).sum();
        let total_parts: u32 = result.files.iter().map(|f| f.parts).sum();

        println!();
        println!(
            "{} file(s), {}, {} part(s) in {:.2}s ({})",
            result.files.len(),
            fmt::bytes(total_bytes),
            total_parts,
            elapsed.as_secs_f64(),
            fmt::throughput(total_bytes, elapsed.as_secs_f64()),
        );
    }

    Ok(())
}
