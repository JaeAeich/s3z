//! CLI argument definitions.

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use s3z::{Config, auth::CredentialSource};

/// S3 ops, but fearlessly fast.
#[derive(Debug, Parser)]
#[command(name = "s3z", version, about, propagate_version = true)]
pub(crate) struct Cli {
    /// AWS region.
    #[arg(short, long, global = true, env = "AWS_DEFAULT_REGION", default_value = "us-east-1")]
    pub region: String,

    /// Custom S3 endpoint URL (for `MinIO`, R2, GCS, etc.).
    #[arg(short, long, global = true, env = "AWS_ENDPOINT_URL")]
    pub endpoint: Option<String>,

    /// AWS access key ID. Falls back to `AWS_ACCESS_KEY_ID` env var.
    #[arg(long, global = true, env = "AWS_ACCESS_KEY_ID")]
    pub access_key: Option<String>,

    /// AWS secret access key. Falls back to `AWS_SECRET_ACCESS_KEY` env var.
    #[arg(long, global = true, env = "AWS_SECRET_ACCESS_KEY")]
    pub secret_key: Option<String>,

    /// Suppress all output except errors.
    #[arg(short, long, global = true, default_value_t = false)]
    pub quiet: bool,

    /// Subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Available commands.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Download objects from S3 to a local directory.
    Download(DownloadArgs),
    /// List objects in an S3 bucket.
    Ls(ListArgs),
    /// Upload files or directories to S3.
    Upload(UploadArgs),
}

/// Arguments for the download subcommand.
#[derive(Debug, clap::Args)]
pub(crate) struct DownloadArgs {
    /// Source S3 bucket.
    #[arg(short, long)]
    pub bucket: String,

    /// Key prefix to download (e.g. `data/2024/`).
    #[arg(short, long, default_value = "")]
    pub prefix: String,

    /// Local directory to write files into.
    #[arg(short, long, default_value = ".")]
    pub dest: PathBuf,

    /// Number of files downloaded in parallel (auto-tuned if omitted).
    #[arg(short, long)]
    pub workers: Option<usize>,

    /// Number of parts downloaded concurrently per file (auto-tuned if omitted).
    #[arg(short, long)]
    pub concurrency: Option<usize>,
}

/// Arguments for the ls subcommand.
#[derive(Debug, clap::Args)]
pub(crate) struct ListArgs {
    /// S3 bucket to list.
    #[arg(short, long)]
    pub bucket: String,

    /// Key prefix to filter by (e.g. `data/2024/`).
    #[arg(short, long, default_value = "")]
    pub prefix: String,
}

/// Arguments for the upload subcommand.
#[derive(Debug, clap::Args)]
pub(crate) struct UploadArgs {
    /// Local paths (files or directories) to upload.
    #[arg(required = true)]
    pub sources: Vec<PathBuf>,

    /// Destination S3 bucket.
    #[arg(short, long)]
    pub bucket: String,

    /// Key prefix in the bucket (e.g. `data/2024/`).
    #[arg(short, long, default_value = "")]
    pub prefix: String,

    /// Number of files uploaded in parallel.
    #[arg(short, long, default_value_t = 32)]
    pub workers: usize,

    /// Number of parts uploaded concurrently per file.
    #[arg(short, long, default_value_t = 8)]
    pub concurrency: usize,
}

/// Build an [`s3z::Config`] from the global CLI options.
pub(crate) fn build_config(cli: &Cli) -> Config {
    let creds = match (&cli.access_key, &cli.secret_key) {
        (Some(ak), Some(sk)) => {
            CredentialSource::Static {
                access_key: ak.clone(),
                secret_key: sk.clone(),
            }
        },
        _ => CredentialSource::Env,
    };

    match &cli.endpoint {
        Some(ep) => Config::with_endpoint(&cli.region, creds, ep.clone()),
        None => Config::new(&cli.region, creds),
    }
}
