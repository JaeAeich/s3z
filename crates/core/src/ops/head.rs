//! Head-object operation — retrieve object metadata without downloading the body.

use std::collections::HashMap;

use bytes::Bytes;

use crate::{
    auth::Credentials,
    client::S3Client,
    config::{Config, RetryPolicy},
    error::{Error, Result},
    http::{ObjectKey, request::build_signed},
    trace::{maybe_debug, maybe_info, maybe_warn},
};

/// Output from a `HEAD` object request.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct HeadObjectOutput {
    /// Object size in bytes.
    pub content_length: u64,
    /// MIME type of the object.
    pub content_type: Option<String>,
    /// Entity tag.
    pub etag: Option<String>,
    /// Last modified timestamp (RFC 2822 / HTTP-date).
    pub last_modified: Option<String>,
    /// User-defined metadata (`x-amz-meta-*` headers).
    pub metadata: HashMap<String, String>,
    /// Server-side encryption algorithm, if any.
    pub server_side_encryption: Option<String>,
    /// Storage class (e.g. `STANDARD`, `GLACIER`).
    pub storage_class: Option<String>,
    /// Version ID when bucket versioning is enabled.
    pub version_id: Option<String>,
}

/// A `HEAD` object request.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct HeadObjectRequest {
    /// Target S3 bucket.
    pub bucket: String,
    /// Object key.
    pub key: String,
}

impl HeadObjectRequest {
    /// Create a new head-object request.
    #[inline]
    #[must_use]
    pub fn new(bucket: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            bucket: bucket.into(),
            key: key.into(),
        }
    }
}

/// Send a HEAD request with retry, mapping HTTP status codes to typed errors.
///
/// HEAD responses carry no XML body, so we cannot use `response::parse_error`.
/// Instead we map status codes directly:
/// - 404 → `Error::S3 { code: "NotFound", .. }`
/// - 403 → `Error::S3 { code: "AccessDenied", .. }`
/// - 5xx / 429 → retryable
async fn send_head_with_retry(
    http: &reqwest::Client, config: &Config, creds: &Credentials, bucket: &str, key: &ObjectKey,
    policy: &RetryPolicy,
) -> Result<reqwest::Response> {
    let uri_base = format!(
        "{endpoint}/{bucket}/{key}",
        endpoint = config.endpoint_url(),
        key = key.encoded(),
    );

    let mut last_err: Option<Error> = None;
    let mut delay = policy.base_delay;
    let attempts = policy.max_retries.saturating_add(1);

    for attempt in 0..attempts {
        let uri: http::Uri = uri_base.parse()?;
        let signed =
            build_signed(http::Method::HEAD, uri.clone(), Bytes::new(), creds, &config.region)?;

        let method = signed.method().clone();
        let headers = signed.headers().clone();

        let reqwest_req = http
            .request(method.clone(), uri.to_string())
            .headers(headers)
            .body(Bytes::new())
            .build()?;

        maybe_debug!(%method, %uri, attempt, "sending HEAD request");

        #[cfg(feature = "tracing")]
        let start = std::time::Instant::now();

        match http.execute(reqwest_req).await {
            Ok(resp) => {
                #[cfg(feature = "tracing")]
                let elapsed = start.elapsed();

                let status = resp.status();
                if status.is_success() {
                    maybe_debug!(%method, %uri, ?elapsed, %status, "HEAD ok");
                    return Ok(resp);
                }

                if status.is_server_error() || status == http::StatusCode::TOO_MANY_REQUESTS {
                    maybe_warn!(%method, %uri, ?elapsed, attempt, %status, "retryable HEAD error");
                    last_err = Some(Error::S3 {
                        code: status.to_string(),
                        message: String::new(),
                    });
                } else {
                    // Fatal 4xx — map well-known codes.
                    let (code, message) = match status.as_u16() {
                        404 => ("NotFound".to_owned(), "object does not exist".to_owned()),
                        403 => ("AccessDenied".to_owned(), "access denied".to_owned()),
                        _ => (status.to_string(), String::new()),
                    };
                    maybe_warn!(%method, %uri, ?elapsed, %status, "fatal HEAD error");
                    return Err(Error::S3 {
                        code,
                        message,
                    });
                }
            },
            Err(e) => {
                #[cfg(feature = "tracing")]
                let elapsed = start.elapsed();

                maybe_warn!(%uri, ?elapsed, attempt, error = %e, "HEAD transport error");
                last_err = Some(Error::Http(e));
            },
        }

        if attempt.saturating_add(1) < attempts {
            let jittered_delay = jittered(delay);
            maybe_debug!(?jittered_delay, "backing off");
            tokio::time::sleep(jittered_delay).await;
            delay = delay.saturating_mul(2).min(policy.max_delay);
        }
    }

    Err(last_err.unwrap_or_else(|| Error::Internal("no attempts made".into())))
}

/// Equal jitter: `base/2 + rand(0 .. base/2)`.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "half + jitter cannot overflow: jitter < half+1 <= nanos/2+1, and half <= nanos/2"
)]
fn jittered(base: core::time::Duration) -> core::time::Duration {
    use core::time::Duration;

    let nanos = base.as_nanos();
    if nanos <= 1 {
        return base;
    }
    let half = nanos / 2;
    let entropy = u128::from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos(),
    );
    let jitter = entropy % half.saturating_add(1);
    Duration::from_nanos(u64::try_from(half + jitter).unwrap_or(u64::MAX))
}

/// Parse the HEAD response headers into [`HeadObjectOutput`].
fn parse_head_response(resp: &reqwest::Response) -> HeadObjectOutput {
    let headers = resp.headers();

    let content_length = headers
        .get(http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    let content_type =
        headers.get(http::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(String::from);

    let etag = headers.get(http::header::ETAG).and_then(|v| v.to_str().ok()).map(String::from);

    let last_modified =
        headers.get(http::header::LAST_MODIFIED).and_then(|v| v.to_str().ok()).map(String::from);

    let version_id =
        headers.get("x-amz-version-id").and_then(|v| v.to_str().ok()).map(String::from);

    let storage_class =
        headers.get("x-amz-storage-class").and_then(|v| v.to_str().ok()).map(String::from);

    let server_side_encryption =
        headers.get("x-amz-server-side-encryption").and_then(|v| v.to_str().ok()).map(String::from);

    let metadata: HashMap<String, String> = headers
        .iter()
        .filter_map(|(name, value)| {
            let name_str = name.as_str();
            name_str.strip_prefix("x-amz-meta-").map(|meta_key| {
                (meta_key.to_owned(), value.to_str().unwrap_or_default().to_owned())
            })
        })
        .collect();

    HeadObjectOutput {
        content_length,
        content_type,
        etag,
        last_modified,
        metadata,
        server_side_encryption,
        storage_class,
        version_id,
    }
}

#[expect(clippy::multiple_inherent_impl, reason = "ops extend S3Client from their own modules")]
impl S3Client {
    /// Retrieve object metadata via a HEAD request.
    ///
    /// Returns [`HeadObjectOutput`] containing size, content type, `ETag`,
    /// timestamps, custom metadata, and other S3 headers — without
    /// downloading the object body.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use s3z::{Config, S3Client, HeadObjectRequest, auth::CredentialSource};
    /// # async fn example() -> s3z::error::Result<()> {
    /// let client = S3Client::new(Config::new("us-east-1", CredentialSource::Env)).await?;
    /// let req = HeadObjectRequest::new("my-bucket", "data/file.csv");
    /// let output = client.head_object(req).await?;
    /// println!("size = {}, etag = {:?}", output.content_length, output.etag);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// - `Error::S3 { code: "NotFound", .. }` — the object does not exist (HTTP 404).
    /// - `Error::S3 { code: "AccessDenied", .. }` — insufficient permissions (HTTP 403).
    /// - Transport or signing errors propagated from the HTTP layer.
    pub async fn head_object(&self, req: HeadObjectRequest) -> Result<HeadObjectOutput> {
        let key = ObjectKey::new(&req.key);

        maybe_info!(bucket = %req.bucket, key = %key, "head_object");

        let resp = send_head_with_retry(
            &self.http,
            &self.config,
            &self.creds,
            &req.bucket,
            &key,
            &self.config.retry,
        )
        .await?;

        let output = parse_head_response(&resp);

        maybe_info!(
            bucket = %req.bucket,
            key = %key,
            content_length = output.content_length,
            "head_object complete"
        );

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use core::{
        sync::atomic::{AtomicU32, Ordering},
        time::Duration,
    };

    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

    use super::*;
    use crate::{Config, auth::CredentialSource};

    async fn test_client(server: &MockServer) -> S3Client {
        let mut config = Config::with_endpoint(
            "us-east-1",
            CredentialSource::Static {
                access_key: "AKID".into(),
                secret_key: "SECRET".into(),
            },
            server.uri(),
        );
        // Fast retries for tests.
        config.retry = RetryPolicy {
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(10),
            max_retries: 2,
        };
        S3Client::new(config).await.unwrap()
    }

    #[tokio::test]
    async fn head_object_success() {
        let server = MockServer::start().await;

        Mock::given(method("HEAD"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", "12345")
                    .insert_header("content-type", "application/octet-stream")
                    .insert_header("etag", "\"abc123\"")
                    .insert_header("last-modified", "Mon, 01 Jan 2024 00:00:00 GMT")
                    .insert_header("x-amz-version-id", "v1")
                    .insert_header("x-amz-storage-class", "STANDARD")
                    .insert_header("x-amz-server-side-encryption", "AES256"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let output = client.head_object(HeadObjectRequest::new("bucket", "key.txt")).await.unwrap();

        assert_eq!(output.content_length, 12345);
        assert_eq!(output.content_type.as_deref(), Some("application/octet-stream"));
        assert_eq!(output.etag.as_deref(), Some("\"abc123\""));
        assert_eq!(output.last_modified.as_deref(), Some("Mon, 01 Jan 2024 00:00:00 GMT"));
        assert_eq!(output.version_id.as_deref(), Some("v1"));
        assert_eq!(output.storage_class.as_deref(), Some("STANDARD"));
        assert_eq!(output.server_side_encryption.as_deref(), Some("AES256"));
    }

    #[tokio::test]
    async fn head_object_not_found() {
        let server = MockServer::start().await;

        Mock::given(method("HEAD"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let err =
            client.head_object(HeadObjectRequest::new("bucket", "missing.txt")).await.unwrap_err();

        match err {
            Error::S3 {
                code,
                message,
            } => {
                assert_eq!(code, "NotFound");
                assert_eq!(message, "object does not exist");
            },
            other => panic!("expected S3 NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn head_object_access_denied() {
        let server = MockServer::start().await;

        Mock::given(method("HEAD"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let err =
            client.head_object(HeadObjectRequest::new("bucket", "secret.txt")).await.unwrap_err();

        match err {
            Error::S3 {
                code,
                message,
            } => {
                assert_eq!(code, "AccessDenied");
                assert_eq!(message, "access denied");
            },
            other => panic!("expected S3 AccessDenied, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn head_object_extracts_custom_metadata() {
        let server = MockServer::start().await;

        Mock::given(method("HEAD"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", "0")
                    .insert_header("x-amz-meta-author", "alice")
                    .insert_header("x-amz-meta-project", "s3z"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let output =
            client.head_object(HeadObjectRequest::new("bucket", "file.txt")).await.unwrap();

        assert_eq!(output.metadata.get("author").map(String::as_str), Some("alice"));
        assert_eq!(output.metadata.get("project").map(String::as_str), Some("s3z"));
    }

    #[tokio::test]
    async fn head_object_retries_on_500() {
        let server = MockServer::start().await;
        let call_count = AtomicU32::new(0);

        Mock::given(method("HEAD"))
            .respond_with(move |_: &wiremock::Request| {
                let n = call_count.fetch_add(1, Ordering::Relaxed);
                if n < 2 {
                    ResponseTemplate::new(500)
                } else {
                    ResponseTemplate::new(200).insert_header("content-length", "42")
                }
            })
            .expect(3)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let output = client.head_object(HeadObjectRequest::new("bucket", "key.txt")).await.unwrap();

        assert_eq!(output.content_length, 42);
    }

    #[tokio::test]
    async fn head_object_minimal_headers() {
        let server = MockServer::start().await;

        Mock::given(method("HEAD"))
            .respond_with(ResponseTemplate::new(200).insert_header("content-length", "0"))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let output =
            client.head_object(HeadObjectRequest::new("bucket", "empty.txt")).await.unwrap();

        assert_eq!(output.content_length, 0);
        assert!(output.content_type.is_none());
        assert!(output.etag.is_none());
        assert!(output.last_modified.is_none());
        assert!(output.version_id.is_none());
        assert!(output.storage_class.is_none());
        assert!(output.server_side_encryption.is_none());
        assert!(output.metadata.is_empty());
    }
}
