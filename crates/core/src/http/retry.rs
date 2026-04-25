//! HTTP retry logic with exponential backoff.

use bytes::Bytes;
use tokio::time::sleep;

use crate::{
    config::RetryPolicy,
    error::{Error, Result},
    http::response,
};

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
        let body_text = resp.text().await.unwrap_or_default();
        return Outcome::Retryable(Error::S3 {
            code: status.to_string(),
            message: body_text,
        });
    }
    let body_text = resp.text().await.unwrap_or_default();
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

/// Send a request whose body is a separate `Bytes` (used for part uploads
/// where the signed request has an empty body placeholder).
pub(crate) async fn send_with_retry_bytes(
    http: &reqwest::Client, signed_req: http::Request<Bytes>, body: Bytes, policy: &RetryPolicy,
) -> Result<reqwest::Response> {
    let method = signed_req.method().clone();
    let uri = signed_req.uri().clone();
    let headers = signed_req.headers().clone();

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

    for attempt in 0..attempts {
        let reqwest_req = http
            .request(method.clone(), uri.to_string())
            .headers(headers.clone())
            .body(body.clone())
            .build()?;

        match http.execute(reqwest_req).await {
            Ok(resp) => {
                match classify(resp).await {
                    Outcome::Success(r) => return Ok(r),
                    Outcome::Retryable(e) => last_err = Some(e),
                    Outcome::Fatal(e) => return Err(e),
                }
            },
            Err(e) => {
                last_err = Some(Error::Http(e));
            },
        }

        if attempt.saturating_add(1) < attempts {
            sleep(delay).await;
            delay = delay.saturating_mul(2).min(policy.max_delay);
        }
    }

    Err(last_err.unwrap_or_else(|| Error::Internal("no attempts made".into())))
}
