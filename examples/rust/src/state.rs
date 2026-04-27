//! Shared application state.

use std::env;

use s3z::{Config, S3Client, auth::CredentialSource};

/// Application state shared across all handlers.
#[derive(Debug, Clone)]
pub(crate) struct AppState {
    /// The s3z S3 client.
    pub(crate) client: S3Client,
    /// Target S3 bucket.
    pub(crate) bucket: String,
}

impl AppState {
    /// Build state from environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if credential resolution or HTTP client setup fails.
    pub(crate) async fn from_env() -> s3z::error::Result<Self> {
        let region = env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".into());
        let endpoint = env::var("S3_ENDPOINT").ok();
        let bucket = env::var("S3_BUCKET").unwrap_or_else(|_| "bench-bucket".into());

        let creds =
            match (env::var("AWS_ACCESS_KEY_ID").ok(), env::var("AWS_SECRET_ACCESS_KEY").ok()) {
                (Some(ak), Some(sk)) => {
                    CredentialSource::Static {
                        access_key: ak,
                        secret_key: sk,
                    }
                },
                _ => CredentialSource::Env,
            };

        let config = if let Some(ep) = endpoint {
            Config::with_endpoint(region, creds, ep)
        } else {
            Config::new(region, creds)
        };

        let client = S3Client::new(config).await?;
        Ok(Self {
            client,
            bucket,
        })
    }
}
