//! HTTP retry logic with exponential backoff.

use core::{future::Future, time::Duration};

use bytes::Bytes;
use tokio::time::sleep;

use crate::{
    config::RetryPolicy,
    error::{Error, Result},
    http::response,
    trace::{maybe_debug, maybe_warn},
};

/// Equal jitter: `base/2 + rand(0 .. base/2)`.
///
/// Avoids retry thundering herds when many workers are throttled
/// simultaneously. Uses system-time nanos as cheap entropy — sufficient
/// for backoff jitter where cryptographic randomness is irrelevant.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "half + jitter cannot overflow: jitter < half+1 ≤ nanos/2+1, and half ≤ nanos/2"
)]
fn jittered(base: Duration) -> Duration {
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

/// Classify a response and either return it or extract a retryable error.
enum Outcome {
    Fatal(Error),
    Retryable(Error),
    Success(reqwest::Response),
}

async fn classify(resp: reqwest::Response) -> Outcome {
    if resp.status().is_success() {
        return Outcome::Success(resp);
    }
    if resp.status().is_server_error() || resp.status() == http::StatusCode::TOO_MANY_REQUESTS {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_else(|e| format!("<body read failed: {e}>"));
        return Outcome::Retryable(Error::S3 {
            code: status.to_string(),
            message: body_text,
        });
    }
    let body_text = resp.text().await.unwrap_or_else(|e| format!("<body read failed: {e}>"));
    Outcome::Fatal(response::parse_error(&body_text))
}

/// Send a request with exponential backoff retry.
///
/// Retries on 5xx, 429 (throttling), and transport errors. Other 4xx errors fail immediately.
///
/// # Errors
///
/// Returns the last encountered error after all retries are exhausted,
/// or a 4xx error immediately.
pub(crate) async fn send_with_retry(
    http: &reqwest::Client, req: http::Request<Bytes>, policy: &RetryPolicy,
) -> Result<reqwest::Response> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let headers = req.headers().clone();
    let body = req.into_body();

    send_retry_loop(http, method, &uri, &headers, body, policy).await
}

/// Core retry loop shared by all send variants.
async fn send_retry_loop(
    http: &reqwest::Client, method: http::Method, uri: &http::Uri, headers: &http::HeaderMap,
    body: Bytes, policy: &RetryPolicy,
) -> Result<reqwest::Response> {
    let mut last_err = None;
    let mut delay = policy.base_delay;
    let attempts = policy.max_retries.saturating_add(1);
    let uri_string = uri.to_string();

    for attempt in 0..attempts {
        let reqwest_req = http
            .request(method.clone(), &uri_string)
            .headers(headers.clone())
            .body(body.clone())
            .build()?;

        maybe_debug!(%method, %uri, attempt, "sending request");

        #[cfg(feature = "tracing")]
        let start = std::time::Instant::now();

        match http.execute(reqwest_req).await {
            Ok(resp) => {
                #[cfg(feature = "tracing")]
                let elapsed = start.elapsed();

                match classify(resp).await {
                    Outcome::Success(r) => {
                        maybe_debug!(%method, %uri, ?elapsed, status = %r.status(), "request ok");
                        return Ok(r);
                    },
                    Outcome::Retryable(e) => {
                        maybe_warn!(
                            %method, %uri, ?elapsed, attempt, error = %e, "retryable error"
                        );
                        last_err = Some(e);
                    },
                    Outcome::Fatal(e) => {
                        maybe_warn!(%method, %uri, ?elapsed, error = %e, "fatal error");
                        return Err(e);
                    },
                }
            },
            Err(e) => {
                #[cfg(feature = "tracing")]
                let elapsed = start.elapsed();

                maybe_warn!(%method, %uri, ?elapsed, attempt, error = %e, "transport error");
                last_err = Some(Error::Http(e));
            },
        }

        if attempt.saturating_add(1) < attempts {
            let jittered_delay = jittered(delay);
            maybe_debug!(?jittered_delay, "backing off");
            sleep(jittered_delay).await;
            delay = delay.saturating_mul(2).min(policy.max_delay);
        }
    }

    Err(last_err.unwrap_or_else(|| Error::Internal("no attempts made".into())))
}

/// Send a streaming request with exponential backoff retry and re-signing.
///
/// On each attempt (including retries):
/// 1. `make_request` is called to produce a freshly signed `Request<Bytes>` (headers only — body is
///    `Bytes::new()`).
/// 2. `make_body` is called to produce a fresh `reqwest::Body` from disk.
///
/// Re-signing on every attempt ensures the `SigV4` timestamp stays current
/// even after long backoff delays.
pub(crate) async fn send_with_retry_stream<S, SF, B, BF>(
    http: &reqwest::Client, make_request: S, make_body: B, policy: &RetryPolicy,
) -> Result<reqwest::Response>
where
    S: Fn() -> SF,
    SF: Future<Output = Result<http::Request<Bytes>>>,
    B: Fn() -> BF,
    BF: Future<Output = Result<reqwest::Body>>,
{
    let mut last_err = None;
    let mut delay = policy.base_delay;
    let attempts = policy.max_retries.saturating_add(1);

    for attempt in 0..attempts {
        let signed_req = make_request().await?;
        let method = signed_req.method().clone();
        let uri = signed_req.uri().clone();
        let headers = signed_req.headers().clone();

        let body = make_body().await?;

        let reqwest_req =
            http.request(method.clone(), uri.to_string()).headers(headers).body(body).build()?;

        maybe_debug!(%method, %uri, attempt, "sending request");

        #[cfg(feature = "tracing")]
        let start = std::time::Instant::now();

        match http.execute(reqwest_req).await {
            Ok(resp) => {
                #[cfg(feature = "tracing")]
                let elapsed = start.elapsed();

                match classify(resp).await {
                    Outcome::Success(r) => {
                        maybe_debug!(%method, %uri, ?elapsed, status = %r.status(), "request ok");
                        return Ok(r);
                    },
                    Outcome::Retryable(e) => {
                        maybe_warn!(
                            %method, %uri, ?elapsed, attempt, error = %e, "retryable error"
                        );
                        last_err = Some(e);
                    },
                    Outcome::Fatal(e) => {
                        maybe_warn!(%method, %uri, ?elapsed, error = %e, "fatal error");
                        return Err(e);
                    },
                }
            },
            Err(e) => {
                #[cfg(feature = "tracing")]
                let elapsed = start.elapsed();

                maybe_warn!(%method, %uri, ?elapsed, attempt, error = %e, "transport error");
                last_err = Some(Error::Http(e));
            },
        }

        if attempt.saturating_add(1) < attempts {
            let jittered_delay = jittered(delay);
            maybe_debug!(?jittered_delay, "backing off");
            sleep(jittered_delay).await;
            delay = delay.saturating_mul(2).min(policy.max_delay);
        }
    }

    Err(last_err.unwrap_or_else(|| Error::Internal("no attempts made".into())))
}

#[cfg(test)]
mod tests {
    use core::{
        sync::atomic::{AtomicU32, Ordering},
        time::Duration,
    };

    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

    use super::*;

    fn fast_policy(max_retries: u32) -> RetryPolicy {
        RetryPolicy {
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(10),
            max_retries,
        }
    }

    fn build_request(uri: &str) -> http::Request<Bytes> {
        http::Request::builder().method(http::Method::GET).uri(uri).body(Bytes::new()).unwrap()
    }

    #[tokio::test]
    async fn retry_success_on_first_attempt() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .expect(1)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let req = build_request(&server.uri());
        let resp = send_with_retry(&client, req, &fast_policy(3)).await.unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn retry_retries_on_500_then_succeeds() {
        let server = MockServer::start().await;
        let call_count = AtomicU32::new(0);

        Mock::given(method("GET"))
            .respond_with(move |_: &wiremock::Request| {
                let n = call_count.fetch_add(1, Ordering::Relaxed);
                if n < 2 {
                    ResponseTemplate::new(500).set_body_string("server error")
                } else {
                    ResponseTemplate::new(200).set_body_string("ok")
                }
            })
            .expect(3)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let req = build_request(&server.uri());
        let resp = send_with_retry(&client, req, &fast_policy(5)).await.unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn retry_retries_on_429() {
        let server = MockServer::start().await;
        let call_count = AtomicU32::new(0);

        Mock::given(method("GET"))
            .respond_with(move |_: &wiremock::Request| {
                let n = call_count.fetch_add(1, Ordering::Relaxed);
                if n == 0 {
                    ResponseTemplate::new(429).set_body_string("throttled")
                } else {
                    ResponseTemplate::new(200).set_body_string("ok")
                }
            })
            .expect(2)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let req = build_request(&server.uri());
        let resp = send_with_retry(&client, req, &fast_policy(3)).await.unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn retry_fatal_on_403() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(403).set_body_string(
                    "<Error><Code>AccessDenied</Code><Message>no</Message></Error>",
                ),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let req = build_request(&server.uri());
        let err = send_with_retry(&client, req, &fast_policy(3)).await.unwrap_err();
        match err {
            Error::S3 {
                code, ..
            } => assert_eq!(code, "AccessDenied"),
            other => panic!("expected S3 error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn retry_exhaustion_returns_last_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(503).set_body_string("unavailable"))
            .expect(3) // 1 initial + 2 retries
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let req = build_request(&server.uri());
        let err = send_with_retry(&client, req, &fast_policy(2)).await.unwrap_err();
        assert!(matches!(err, Error::S3 { .. }));
    }

    #[tokio::test]
    async fn stream_retry_success() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .expect(1)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let uri_str = server.uri();
        let resp = send_with_retry_stream(
            &client,
            || {
                let u = uri_str.clone();
                async move {
                    Ok(http::Request::builder()
                        .method(http::Method::PUT)
                        .uri(u)
                        .header("content-length", 5)
                        .body(Bytes::new())
                        .unwrap())
                }
            },
            || async { Ok(reqwest::Body::from("hello")) },
            &fast_policy(2),
        )
        .await
        .unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn stream_retry_re_signs_on_each_attempt() {
        let server = MockServer::start().await;
        let sign_count = AtomicU32::new(0);
        let call_count = AtomicU32::new(0);

        Mock::given(method("PUT"))
            .respond_with(move |_: &wiremock::Request| {
                let n = call_count.fetch_add(1, Ordering::Relaxed);
                if n == 0 {
                    ResponseTemplate::new(500).set_body_string("error")
                } else {
                    ResponseTemplate::new(200).set_body_string("ok")
                }
            })
            .expect(2)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let uri_str = server.uri();
        let resp = send_with_retry_stream(
            &client,
            || {
                sign_count.fetch_add(1, Ordering::Relaxed);
                let u = uri_str.clone();
                async move {
                    Ok(http::Request::builder()
                        .method(http::Method::PUT)
                        .uri(u)
                        .header("content-length", 4)
                        .body(Bytes::new())
                        .unwrap())
                }
            },
            || async { Ok(reqwest::Body::from("data")) },
            &fast_policy(3),
        )
        .await
        .unwrap();

        assert_eq!(resp.status(), 200);
        // make_request called twice (once per attempt), verifying re-signing
        assert_eq!(sign_count.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn stream_retry_fatal_4xx_no_retry() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .respond_with(
                ResponseTemplate::new(403)
                    .set_body_string("<Error><Code>Forbidden</Code><Message>no</Message></Error>"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let client = reqwest::Client::new();
        let uri_str = server.uri();
        let err = send_with_retry_stream(
            &client,
            || {
                let u = uri_str.clone();
                async move {
                    Ok(http::Request::builder()
                        .method(http::Method::PUT)
                        .uri(u)
                        .header("content-length", 0)
                        .body(Bytes::new())
                        .unwrap())
                }
            },
            || async { Ok(reqwest::Body::from("")) },
            &fast_policy(3),
        )
        .await
        .unwrap_err();

        assert!(matches!(err, Error::S3 { .. }));
    }
}
