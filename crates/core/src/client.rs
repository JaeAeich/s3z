//! S3 client — owns the HTTP connection pool and config.

use core::time::Duration;

use tokio::task::JoinSet;

use crate::{
    auth::{self, Credentials},
    config::{Config, TransferConfig},
    error::Result,
    trace::maybe_debug,
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
    /// # Errors
    ///
    /// Returns an error if credential resolution fails (e.g.
    /// [`crate::auth::CredentialSource::Env`] and the env vars are missing)
    /// or if the HTTP client cannot be built.
    #[inline]
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

        let client = Self {
            config,
            creds,
            http,
        };

        client.warmup_pool().await;
        Ok(client)
    }

    /// Pre-establish TCP connections so the first operation doesn't
    /// stall on sequential connection setup (~2 ms each).
    async fn warmup_pool(&self) {
        let count = TransferConfig::MAX_IDLE_CONNECTIONS;
        let uri = self.config.endpoint_url().to_owned();
        let mut set = JoinSet::new();

        for _ in 0..count {
            let client = self.http.clone();
            let u = uri.clone();
            set.spawn(async move {
                drop(client.head(&u).send().await);
            });
        }

        while set.join_next().await.is_some() {}
        maybe_debug!(connections = count, "connection pool warmed");
    }
}
