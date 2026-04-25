//! S3 client — owns the HTTP connection pool and config.

use core::time::Duration;

use crate::{
    auth::{self, Credentials},
    config::{Config, TransferConfig},
    error::Result,
};

/// The s3z S3 client.
///
/// Cheap to clone (shares the underlying connection pool).
#[derive(Debug, Clone)]
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "direct field access avoids trivial getters"
)]
pub struct S3Client {
    pub(crate) config: Config,
    pub(crate) creds: Credentials,
    pub(crate) http: reqwest::Client,
}

impl S3Client {
    /// Create a new client with the given configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if credential resolution fails (e.g.
    /// [`crate::auth::CredentialSource::Env`] and the env vars are missing)
    /// or if the HTTP client cannot be built.
    #[inline]
    pub fn new(config: Config) -> Result<Self> {
        let creds = auth::resolve(&config.credentials)?;

        let http = reqwest::Client::builder()
            .pool_max_idle_per_host(TransferConfig::MAX_IDLE_CONNECTIONS)
            .tcp_keepalive(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(90))
            .build()?;

        Ok(Self {
            config,
            creds,
            http,
        })
    }
}
