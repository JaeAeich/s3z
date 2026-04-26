//! Configuration types for the S3 client.

use core::time::Duration;

use crate::auth::CredentialSource;

/// Top-level client configuration.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Config {
    /// How to obtain AWS credentials.
    pub credentials: CredentialSource,
    /// Custom endpoint for S3-compatible backends (`MinIO`, R2, GCS).
    /// When `None`, uses the default AWS endpoint.
    pub endpoint: Option<String>,
    /// AWS region (e.g. `us-east-1`).
    pub region: String,
    /// Resolved endpoint URL (cached at construction time).
    resolved_endpoint: String,
    /// Retry behaviour.
    pub retry: RetryPolicy,
    /// Transfer tuning knobs.
    pub transfer: TransferConfig,
}

/// Retry behaviour with exponential backoff.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct RetryPolicy {
    /// Initial backoff delay.
    pub base_delay: Duration,
    /// Maximum backoff delay.
    pub max_delay: Duration,
    /// Maximum number of retries per request.
    pub max_retries: u32,
}

/// Controls multipart thresholds and concurrency.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct TransferConfig {
    /// Files larger than this (bytes) use multipart upload.
    pub multipart_threshold: u64,
    /// Default part size hint (bytes). For multipart uploads, the actual
    /// part size is auto-computed from file size and concurrency — this
    /// value is used only as the fallback for `plan_parts` when called
    /// directly. See [`crate::transfer::scheduler::compute_part_size`].
    pub part_size: u64,
}

impl Config {
    /// Resolved S3 endpoint URL.
    #[inline]
    #[must_use]
    pub fn endpoint_url(&self) -> &str {
        &self.resolved_endpoint
    }

    /// Create a new config with the given region and credentials.
    ///
    /// Uses default transfer and retry settings. Override fields after
    /// construction if needed.
    #[inline]
    #[must_use]
    pub fn new(region: impl Into<String>, credentials: CredentialSource) -> Self {
        let region = region.into();
        let resolved_endpoint = format!("https://s3.{region}.amazonaws.com");
        Self {
            credentials,
            endpoint: None,
            region,
            resolved_endpoint,
            retry: RetryPolicy::default(),
            transfer: TransferConfig::default(),
        }
    }

    /// Create a new config with a custom endpoint for S3-compatible backends.
    #[inline]
    #[must_use]
    pub fn with_endpoint(
        region: impl Into<String>, credentials: CredentialSource, endpoint: String,
    ) -> Self {
        let region = region.into();
        let resolved_endpoint = endpoint.clone();
        Self {
            credentials,
            endpoint: Some(endpoint),
            region,
            resolved_endpoint,
            retry: RetryPolicy::default(),
            transfer: TransferConfig::default(),
        }
    }
}

impl TransferConfig {
    /// Connection pool size — must be >= peak in-flight requests
    /// (`workers * concurrency_per_file`) to avoid connection churn.
    pub(crate) const MAX_IDLE_CONNECTIONS: usize = 256;
}

impl Default for RetryPolicy {
    #[inline]
    fn default() -> Self {
        Self {
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            max_retries: 5,
        }
    }
}

impl Default for TransferConfig {
    #[inline]
    fn default() -> Self {
        Self {
            multipart_threshold: 50 * 1024 * 1024,
            part_size: 50 * 1024 * 1024,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::CredentialSource;

    #[test]
    fn default_endpoint_uses_region() {
        let cfg = Config::new("eu-west-1", CredentialSource::Env);
        assert_eq!(cfg.endpoint_url(), "https://s3.eu-west-1.amazonaws.com");
        assert_eq!(cfg.region, "eu-west-1");
        assert!(cfg.endpoint.is_none());
    }

    #[test]
    fn custom_endpoint_used_verbatim() {
        let cfg = Config::with_endpoint(
            "us-east-1",
            CredentialSource::Env,
            "http://localhost:9000".into(),
        );
        assert_eq!(cfg.endpoint_url(), "http://localhost:9000");
        assert_eq!(cfg.endpoint.as_deref(), Some("http://localhost:9000"));
    }

    #[test]
    fn retry_policy_defaults() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, 5);
        assert_eq!(policy.base_delay, Duration::from_millis(100));
        assert_eq!(policy.max_delay, Duration::from_secs(30));
    }

    #[test]
    fn transfer_config_defaults() {
        let tc = TransferConfig::default();
        assert_eq!(tc.multipart_threshold, 50 * 1024 * 1024);
        assert_eq!(tc.part_size, 50 * 1024 * 1024);
    }

    #[test]
    fn config_new_uses_default_retry_and_transfer() {
        let cfg = Config::new("us-east-1", CredentialSource::Env);
        assert_eq!(cfg.retry.max_retries, RetryPolicy::default().max_retries);
        assert_eq!(cfg.transfer.part_size, TransferConfig::default().part_size);
    }
}
