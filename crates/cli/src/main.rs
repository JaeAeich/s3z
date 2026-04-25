//! s3z CLI — S3 ops, but fearlessly fast.

#![expect(
    clippy::multiple_crate_versions,
    reason = "transitive deps from aws-sigv4 pull duplicate versions"
)]
#![expect(clippy::print_stdout, reason = "CLI output is the whole point")]
#![expect(
    clippy::arbitrary_source_item_ordering,
    reason = "CLI field ordering is dictated by UX, not alphabetical"
)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "binary modules are private — pub(crate) is the correct visibility"
)]

mod cli;
mod commands;
mod fmt;

use clap::Parser as _;
use s3z::S3Client;

use crate::cli::{Cli, Command};

#[expect(clippy::pattern_type_mismatch, reason = "matching on &Command")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let config = cli::build_config(&cli);
    let client = S3Client::new(config)?;

    match &cli.command {
        Command::Upload(args) => commands::upload::run(&client, args, cli.quiet).await?,
    }

    Ok(())
}
