//! S3 XML response parsing.

use quick_xml::de::from_str;
use serde::Deserialize;

use crate::error::{Error, Result};

/// Response body from `CompleteMultipartUpload`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct CompleteMultipartUploadResult {
    /// `ETag` of the completed object.
    #[serde(rename = "ETag")]
    pub etag: String,
}

/// Response body from `InitiateMultipartUpload`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct InitiateMultipartUploadResult {
    /// The upload ID assigned by S3.
    pub upload_id: String,
}

/// S3 XML error body.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct S3ErrorBody {
    code: String,
    message: String,
}

/// Parse the `CompleteMultipartUpload` response.
///
/// # Errors
///
/// Returns [`Error::S3`] if the XML body cannot be parsed.
pub(crate) fn parse_complete_multipart(body: &str) -> Result<CompleteMultipartUploadResult> {
    from_str(body).map_err(|_e| {
        Error::S3 {
            code: "ParseError".into(),
            message: "failed to parse CompleteMultipartUploadResult".into(),
        }
    })
}

/// Parse an S3 error XML body into an [`Error`].
pub(crate) fn parse_error(body: &str) -> Error {
    from_str::<S3ErrorBody>(body).map_or_else(
        |_| {
            Error::S3 {
                code: "Unknown".into(),
                message: body.to_owned(),
            }
        },
        |e| {
            Error::S3 {
                code: e.code,
                message: e.message,
            }
        },
    )
}

/// Parse the `InitiateMultipartUpload` response.
///
/// # Errors
///
/// Returns [`Error::S3`] if the XML body cannot be parsed.
pub(crate) fn parse_initiate_multipart(body: &str) -> Result<InitiateMultipartUploadResult> {
    from_str(body).map_err(|_e| {
        Error::S3 {
            code: "ParseError".into(),
            message: "failed to parse InitiateMultipartUploadResult".into(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn parse_initiate_multipart_ok() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <InitiateMultipartUploadResult>
                <Bucket>my-bucket</Bucket>
                <Key>my-key</Key>
                <UploadId>abc123</UploadId>
            </InitiateMultipartUploadResult>"#;
        let result = parse_initiate_multipart(xml).expect("should parse");
        assert_eq!(result.upload_id, "abc123");
    }

    #[test]
    fn parse_complete_multipart_ok() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <CompleteMultipartUploadResult>
                <Location>https://bucket.s3.amazonaws.com/key</Location>
                <Bucket>bucket</Bucket>
                <Key>key</Key>
                <ETag>"etag123"</ETag>
            </CompleteMultipartUploadResult>"#;
        let result = parse_complete_multipart(xml).expect("should parse");
        assert_eq!(result.etag, "\"etag123\"");
    }

    #[test]
    fn parse_error_valid_xml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <Error>
                <Code>NoSuchKey</Code>
                <Message>The specified key does not exist.</Message>
            </Error>"#;
        let err = parse_error(xml);
        match err {
            Error::S3 {
                code,
                message,
            } => {
                assert_eq!(code, "NoSuchKey");
                assert_eq!(message, "The specified key does not exist.");
            },
            other => panic!("expected Error::S3, got {other:?}"),
        }
    }

    #[test]
    fn parse_error_invalid_xml_falls_back() {
        let body = "not xml at all";
        let err = parse_error(body);
        match err {
            Error::S3 {
                code,
                message,
            } => {
                assert_eq!(code, "Unknown");
                assert_eq!(message, "not xml at all");
            },
            other => panic!("expected Error::S3, got {other:?}"),
        }
    }

    #[test]
    fn parse_initiate_multipart_bad_xml() {
        let result = parse_initiate_multipart("garbage");
        result.unwrap_err();
    }

    #[test]
    fn parse_complete_multipart_empty_body() {
        let result = parse_complete_multipart("");
        result.unwrap_err();
    }

    #[test]
    fn parse_initiate_multipart_empty_body() {
        let result = parse_initiate_multipart("");
        result.unwrap_err();
    }

    #[test]
    fn parse_complete_multipart_missing_etag() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <CompleteMultipartUploadResult>
                <Location>https://bucket.s3.amazonaws.com/key</Location>
                <Bucket>bucket</Bucket>
                <Key>key</Key>
            </CompleteMultipartUploadResult>"#;
        let result = parse_complete_multipart(xml);
        result.unwrap_err();
    }

    #[test]
    fn parse_initiate_multipart_missing_upload_id() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <InitiateMultipartUploadResult>
                <Bucket>my-bucket</Bucket>
                <Key>my-key</Key>
            </InitiateMultipartUploadResult>"#;
        let result = parse_initiate_multipart(xml);
        result.unwrap_err();
    }

    #[test]
    fn parse_error_extra_fields_ignored() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <Error>
                <Code>AccessDenied</Code>
                <Message>Access Denied</Message>
                <RequestId>ABC123</RequestId>
                <Resource>/mybucket</Resource>
            </Error>"#;
        let err = parse_error(xml);
        match err {
            Error::S3 {
                code,
                message,
            } => {
                assert_eq!(code, "AccessDenied");
                assert_eq!(message, "Access Denied");
            },
            other => panic!("expected Error::S3, got {other:?}"),
        }
    }

    #[test]
    fn parse_error_empty_body_falls_back() {
        let err = parse_error("");
        match err {
            Error::S3 {
                code,
                message,
            } => {
                assert_eq!(code, "Unknown");
                assert_eq!(message, "");
            },
            other => panic!("expected Error::S3, got {other:?}"),
        }
    }
}
