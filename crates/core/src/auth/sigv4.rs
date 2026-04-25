//! `SigV4` request signing using the `aws-sigv4` crate.

use std::time::SystemTime;

use aws_credential_types::Credentials as AwsCreds;
use aws_sigv4::{
    http_request::{
        PayloadChecksumKind,
        SignableBody,
        SignableRequest,
        SignatureLocation,
        SigningSettings,
        sign as sigv4_sign,
    },
    sign::v4::SigningParams,
};

use crate::{
    auth::Credentials,
    error::{Error, Result},
};

/// Sign an HTTP request in place using `SigV4`.
///
/// When `unsigned_payload` is `true`, the payload is not hashed — the
/// signature covers `UNSIGNED-PAYLOAD` instead. This is required for
/// streaming uploads where the body is not available at signing time.
///
/// # Errors
///
/// Returns [`Error::Auth`] if signing parameters cannot be built or if
/// request signing fails.
pub(crate) fn sign_request(
    request: &mut http::Request<bytes::Bytes>, creds: &Credentials, region: &str,
    unsigned_payload: bool,
) -> Result<()> {
    let aws_creds = AwsCreds::new(&creds.access_key, &creds.secret_key, None, None, "s3z");

    let mut settings = SigningSettings::default();
    settings.payload_checksum_kind = PayloadChecksumKind::XAmzSha256;
    settings.signature_location = SignatureLocation::Headers;

    let identity = aws_creds.into();
    let signing_params = SigningParams::builder()
        .identity(&identity)
        .region(region)
        .name("s3")
        .time(SystemTime::now())
        .settings(settings)
        .build()
        .map_err(|e| Error::Auth(e.to_string()))?;

    let signable_body = if unsigned_payload {
        SignableBody::UnsignedPayload
    } else {
        SignableBody::Bytes(request.body())
    };

    let headers: Vec<(&str, &str)> = request
        .headers()
        .iter()
        .map(|(k, v)| {
            v.to_str()
                .map(|s| (k.as_str(), s))
                .map_err(|e| Error::Auth(format!("non-ASCII header value for {k}: {e}")))
        })
        .collect::<Result<_>>()?;

    let signable_request = SignableRequest::new(
        request.method().as_str(),
        request.uri().to_string(),
        headers.iter().copied(),
        signable_body,
    )
    .map_err(|e| Error::Auth(e.to_string()))?;

    let output = sigv4_sign(signable_request, &signing_params.into())
        .map_err(|e| Error::Auth(e.to_string()))?;

    let (instructions, _signature) = output.into_parts();
    instructions.apply_to_request_http1x(request);

    Ok(())
}
