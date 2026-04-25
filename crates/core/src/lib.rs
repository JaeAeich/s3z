//! # s3z
//!
//! S3 ops, but fearlessly fast.
//!
//! A lightweight, high-throughput S3 client built on raw HTTP + `SigV4` signing.
//! No AWS SDK — just `reqwest`, `aws-sigv4`, and `tokio`.
//!
//! ## Quick start
//!
//! ```no_run
//! use s3z::{Config, S3Client, UploadRequest, auth::CredentialSource};
//!
//! # async fn example() -> s3z::error::Result<()> {
//! let client = S3Client::new(Config::new("us-east-1", CredentialSource::Env))?;
//!
//! let result =
//!     client.upload(UploadRequest::new(vec!["./data".into()], "my-bucket", "uploads/")).await?;
//!
//! for f in &result.files {
//!     println!("{} -> s3://{} ({} parts)", f.source.display(), f.key, f.parts);
//! }
//! # Ok(())
//! # }
//! ```

#![cfg_attr(
    test,
    expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::panic,
        clippy::wildcard_enum_match_arm,
        clippy::expect_used,
        clippy::absolute_paths,
        clippy::pattern_type_mismatch,
        clippy::std_instead_of_core,
        reason = "test code"
    )
)]
#![expect(
    clippy::multiple_crate_versions,
    reason = "transitive deps from aws-sigv4 pull duplicate versions"
)]
#![expect(
    clippy::module_name_repetitions,
    reason = "types like Config and Error are re-exported at crate root"
)]
#![expect(clippy::pub_use, reason = "re-exports form the public API surface")]
#![expect(
    clippy::redundant_pub_crate,
    reason = "pub(crate) in private modules is intentional for clarity"
)]

pub mod auth;
mod client;
pub mod config;
pub mod error;
mod http;
pub mod ops;
mod transfer;

pub use client::S3Client;
pub use config::Config;
pub use http::ObjectKey;
pub use ops::upload::{FileUploadResult, UploadRequest, UploadResult};
