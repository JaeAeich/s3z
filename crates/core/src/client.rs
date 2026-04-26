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
pub struct S3Client {
    pub(crate) config: Config,
    pub(crate) creds: Credentials,
    pub(crate) http: reqwest::Client,
}

impl S3Client {
    /// Create a new client with the given configuration.
    ///
    /// The connection pool grows on demand — no eager warmup. The first
    /// operation pays ~2 ms per new TCP connection, which is negligible
    /// compared to S3 round-trip latency.
    ///
    /// # Errors
    ///
    /// Returns an error if credential resolution fails (e.g.
    /// [`crate::auth::CredentialSource::Env`] and the env vars are missing)
    /// or if the HTTP client cannot be built.
    #[inline]
    #[expect(clippy::unused_async, reason = "API is async-ready for future credential refresh")]
    pub async fn new(config: Config) -> Result<Self> {
        let creds = auth::resolve(&config.credentials)?;

        // tcp_nodelay: disable Nagle so 256 KiB streaming writes don't
        // interact with TCP delayed-ACK (~40 ms stalls per chunk).
        // http1_only: S3 backends speak HTTP/1.1; skip ALPN negotiation
        // to avoid an extra round trip on every new connection.
        let http = reqwest::Client::builder()
            .pool_max_idle_per_host(TransferConfig::MAX_IDLE_CONNECTIONS)
            .tcp_keepalive(Duration::from_secs(30))
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_nodelay(true)
            .http1_only()
            .build()?;

        Ok(Self {
            config,
            creds,
            http,
        })
    }
}
