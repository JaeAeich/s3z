//! Signed HTTP request construction.

use bytes::Bytes;
use http::{Method, Request, Uri};

use crate::{
    auth::{self, Credentials},
    error::Result,
};

/// Build and sign an HTTP request for S3.
///
/// # Errors
///
/// Returns an error if the request cannot be built or signing fails.
pub(crate) fn build_signed(
    method: Method, uri: Uri, body: Bytes, creds: &Credentials, region: &str,
) -> Result<Request<Bytes>> {
    let content_length = body.len();
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-length", content_length)
        .body(body)?;

    auth::sign_request(&mut req, creds, region, false)?;
    Ok(req)
}

/// Build and sign an HTTP request with `UNSIGNED-PAYLOAD`.
///
/// The request is signed but the body hash is `UNSIGNED-PAYLOAD`, allowing
/// the caller to replace the body with a stream after signing.
pub(crate) fn build_signed_unsigned_payload(
    method: Method, uri: Uri, content_length: u64, creds: &Credentials, region: &str,
) -> Result<Request<Bytes>> {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-length", content_length)
        .body(Bytes::new())?;

    auth::sign_request(&mut req, creds, region, true)?;
    Ok(req)
}
