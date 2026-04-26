//! S3 XML response parsing.

use quick_xml::de::from_str;
use serde::Deserialize;

use crate::error::{Error, Result};

/// A common prefix entry from `ListObjectsV2` (when using a delimiter).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct CommonPrefix {
    /// The prefix string.
    pub prefix: String,
}

/// Response body from `ListObjectsV2`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ListBucketResult {
    /// Object entries in this page.
    #[serde(default)]
    pub contents: Vec<ListObject>,
    /// Common prefixes when a delimiter is used.
    #[serde(default)]
    pub common_prefixes: Vec<CommonPrefix>,
    /// Token for fetching the next page, if truncated.
    pub next_continuation_token: Option<String>,
    /// Whether there are more pages.
    #[serde(default)]
    pub is_truncated: bool,
}

/// A single object entry in a `ListObjectsV2` response.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(crate) struct ListObject {
    /// Object key.
    pub key: String,
    /// Object size in bytes.
    pub size: u64,
    /// `ETag` of the object.
    #[serde(rename = "ETag")]
    pub etag: String,
    /// Last modified timestamp (ISO 8601).
    pub last_modified: String,
}

/// Parse a `ListObjectsV2` response.
///
/// # Errors
///
/// Returns [`Error::S3`] if the XML body cannot be parsed.
pub(crate) fn parse_list_objects(body: &str) -> Result<ListBucketResult> {
    from_str(body).map_err(|_e| {
        Error::S3 {
            code: "ParseError".into(),
            message: "failed to parse ListBucketResult".into(),
        }
    })
}

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

    // --- ListObjectsV2 parsing ---

    #[test]
    fn parse_list_objects_basic() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
                <Name>my-bucket</Name>
                <Prefix>data/</Prefix>
                <IsTruncated>false</IsTruncated>
                <Contents>
                    <Key>data/file1.txt</Key>
                    <Size>1024</Size>
                    <ETag>"abc123"</ETag>
                    <LastModified>2024-01-15T10:30:00.000Z</LastModified>
                </Contents>
                <Contents>
                    <Key>data/file2.txt</Key>
                    <Size>2048</Size>
                    <ETag>"def456"</ETag>
                    <LastModified>2024-01-16T11:00:00.000Z</LastModified>
                </Contents>
            </ListBucketResult>"#;
        let result = parse_list_objects(xml).expect("should parse");
        assert_eq!(result.contents.len(), 2);
        assert_eq!(result.contents[0].key, "data/file1.txt");
        assert_eq!(result.contents[0].size, 1024);
        assert_eq!(result.contents[0].etag, "\"abc123\"");
        assert_eq!(result.contents[1].key, "data/file2.txt");
        assert_eq!(result.contents[1].size, 2048);
        assert!(!result.is_truncated);
        assert!(result.next_continuation_token.is_none());
    }

    #[test]
    fn parse_list_objects_truncated_with_token() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
                <Name>bucket</Name>
                <Prefix></Prefix>
                <IsTruncated>true</IsTruncated>
                <NextContinuationToken>abc-token-123</NextContinuationToken>
                <Contents>
                    <Key>file.txt</Key>
                    <Size>100</Size>
                    <ETag>"e1"</ETag>
                    <LastModified>2024-01-01T00:00:00.000Z</LastModified>
                </Contents>
            </ListBucketResult>"#;
        let result = parse_list_objects(xml).expect("should parse");
        assert!(result.is_truncated);
        assert_eq!(result.next_continuation_token.as_deref(), Some("abc-token-123"));
        assert_eq!(result.contents.len(), 1);
    }

    #[test]
    fn parse_list_objects_with_common_prefixes() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
                <Name>bucket</Name>
                <Prefix></Prefix>
                <Delimiter>/</Delimiter>
                <IsTruncated>false</IsTruncated>
                <CommonPrefixes>
                    <Prefix>photos/</Prefix>
                </CommonPrefixes>
                <CommonPrefixes>
                    <Prefix>docs/</Prefix>
                </CommonPrefixes>
            </ListBucketResult>"#;
        let result = parse_list_objects(xml).expect("should parse");
        assert!(result.contents.is_empty());
        assert_eq!(result.common_prefixes.len(), 2);
        assert_eq!(result.common_prefixes[0].prefix, "photos/");
        assert_eq!(result.common_prefixes[1].prefix, "docs/");
    }

    #[test]
    fn parse_list_objects_empty_bucket() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
                <Name>bucket</Name>
                <Prefix></Prefix>
                <IsTruncated>false</IsTruncated>
            </ListBucketResult>"#;
        let result = parse_list_objects(xml).expect("should parse");
        assert!(result.contents.is_empty());
        assert!(result.common_prefixes.is_empty());
        assert!(!result.is_truncated);
    }

    #[test]
    fn parse_list_objects_bad_xml() {
        let result = parse_list_objects("not xml");
        result.unwrap_err();
    }

    #[test]
    fn parse_list_objects_empty_body() {
        let result = parse_list_objects("");
        result.unwrap_err();
    }
}
