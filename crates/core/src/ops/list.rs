//! Paginated object listing — `ListObjectsV2`.

use std::fmt;

use bytes::Bytes;

use crate::{
    auth::Credentials,
    client::S3Client,
    config::Config,
    error::Result,
    http::{
        request::build_signed,
        response::{self, ListObject},
        retry::send_with_retry,
    },
    trace::maybe_debug,
};

/// Metadata for a single S3 object.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ObjectInfo {
    /// Object key.
    pub key: String,
    /// Object size in bytes.
    pub size: u64,
    /// `ETag` of the object.
    pub etag: String,
    /// Last modified timestamp (ISO 8601).
    pub last_modified: String,
}

impl From<ListObject> for ObjectInfo {
    #[inline]
    fn from(o: ListObject) -> Self {
        Self {
            key: o.key,
            size: o.size,
            etag: o.etag,
            last_modified: o.last_modified,
        }
    }
}

/// A single page of listing results.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ListPage {
    /// Objects in this page.
    pub objects: Vec<ObjectInfo>,
    /// Common prefixes (when a delimiter is used).
    pub common_prefixes: Vec<String>,
}

/// What to list.
#[non_exhaustive]
pub struct ListRequest {
    /// Target S3 bucket.
    pub bucket: String,
    /// Key prefix to filter by.
    pub prefix: String,
    /// Delimiter for grouping keys (e.g. `"/"`).
    pub delimiter: Option<String>,
}

impl fmt::Debug for ListRequest {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ListRequest")
            .field("bucket", &self.bucket)
            .field("prefix", &self.prefix)
            .field("delimiter", &self.delimiter)
            .finish()
    }
}

impl ListRequest {
    /// Create a new list request.
    #[inline]
    #[must_use]
    pub fn new(bucket: impl Into<String>, prefix: impl Into<String>) -> Self {
        Self {
            bucket: bucket.into(),
            prefix: prefix.into(),
            delimiter: None,
        }
    }

    /// Set a delimiter for directory-style grouping.
    #[inline]
    #[must_use]
    pub fn with_delimiter(mut self, delimiter: impl Into<String>) -> Self {
        self.delimiter = Some(delimiter.into());
        self
    }
}

/// Paginated S3 object listing.
///
/// Call [`next_page`](Self::next_page) in a loop to consume pages of ~1000
/// keys each. Owns copies of client internals — safe to hold across await
/// boundaries and across FFI.
///
/// # Examples
///
/// ```no_run
/// # use s3z::{Config, S3Client, ListRequest, auth::CredentialSource};
/// # async fn example() -> s3z::error::Result<()> {
/// let client = S3Client::new(Config::new("us-east-1", CredentialSource::Env)).await?;
/// let mut paginator = client.list(ListRequest::new("my-bucket", "data/"));
/// while let Some(page) = paginator.next_page().await? {
///     for obj in &page.objects {
///         println!("{} ({} bytes)", obj.key, obj.size);
///     }
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct ListPaginator {
    bucket: String,
    config: Config,
    continuation_token: Option<String>,
    creds: Credentials,
    delimiter: Option<String>,
    done: bool,
    http: reqwest::Client,
    prefix: String,
}

impl ListPaginator {
    /// Collect all remaining pages into a single `Vec`.
    ///
    /// Convenience for small-to-medium listings. For millions of keys,
    /// prefer iterating with [`next_page`](Self::next_page).
    ///
    /// # Errors
    ///
    /// Returns an error if any page fetch fails.
    pub async fn collect_all(&mut self) -> Result<Vec<ObjectInfo>> {
        let mut all = Vec::new();
        while let Some(page) = self.next_page().await? {
            all.extend(page.objects);
        }
        Ok(all)
    }

    /// Fetch the next page of results (~1000 keys).
    ///
    /// Returns `None` when the listing is exhausted.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or XML parsing fails.
    pub async fn next_page(&mut self) -> Result<Option<ListPage>> {
        if self.done {
            return Ok(None);
        }

        let mut query = format!(
            "{}/{bucket}?list-type=2&prefix={prefix}",
            self.config.endpoint_url(),
            bucket = self.bucket,
            prefix = percent_encoding::utf8_percent_encode(
                &self.prefix,
                percent_encoding::NON_ALPHANUMERIC
            ),
        );

        if let Some(delim) = &self.delimiter {
            query.push_str("&delimiter=");
            query.push_str(
                &percent_encoding::utf8_percent_encode(delim, percent_encoding::NON_ALPHANUMERIC)
                    .to_string(),
            );
        }

        if let Some(token) = &self.continuation_token {
            query.push_str("&continuation-token=");
            query.push_str(
                &percent_encoding::utf8_percent_encode(token, percent_encoding::NON_ALPHANUMERIC)
                    .to_string(),
            );
        }

        let uri: http::Uri = query.parse()?;
        maybe_debug!(%uri, "listing page");

        let req =
            build_signed(http::Method::GET, uri, Bytes::new(), &self.creds, &self.config.region)?;
        let resp = send_with_retry(&self.http, req, &self.config.retry).await?;
        let body = resp.text().await?;

        let parsed = response::parse_list_objects(&body)?;

        if parsed.is_truncated {
            self.continuation_token = parsed.next_continuation_token;
        } else {
            self.done = true;
            self.continuation_token = None;
        }

        let objects: Vec<ObjectInfo> = parsed.contents.into_iter().map(ObjectInfo::from).collect();
        let common_prefixes: Vec<String> =
            parsed.common_prefixes.into_iter().map(|cp| cp.prefix).collect();

        maybe_debug!(
            keys = objects.len(),
            prefixes = common_prefixes.len(),
            truncated = !self.done,
            "page received"
        );

        if objects.is_empty() && common_prefixes.is_empty() && self.done {
            return Ok(None);
        }

        Ok(Some(ListPage {
            objects,
            common_prefixes,
        }))
    }
}

#[expect(clippy::multiple_inherent_impl, reason = "ops extend S3Client from their own modules")]
impl S3Client {
    /// Create a paginator for listing objects under a prefix.
    ///
    /// No network call is made until [`ListPaginator::next_page`] is called.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use s3z::{Config, S3Client, ListRequest, auth::CredentialSource};
    /// # async fn example() -> s3z::error::Result<()> {
    /// let client = S3Client::new(Config::new("us-east-1", CredentialSource::Env)).await?;
    /// let mut paginator = client.list(ListRequest::new("my-bucket", "data/"));
    /// while let Some(page) = paginator.next_page().await? {
    ///     for obj in &page.objects {
    ///         println!("{} ({} bytes)", obj.key, obj.size);
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[inline]
    #[must_use]
    pub fn list(&self, req: ListRequest) -> ListPaginator {
        ListPaginator {
            bucket: req.bucket,
            config: self.config.clone(),
            continuation_token: None,
            creds: self.creds.clone(),
            delimiter: req.delimiter,
            done: false,
            http: self.http.clone(),
            prefix: req.prefix,
        }
    }
}

#[cfg(test)]
mod tests {
    use core::sync::atomic::{AtomicU32, Ordering};

    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

    use super::*;
    use crate::{Config, auth::CredentialSource, http::response::ListObject};

    async fn test_client(server: &MockServer) -> S3Client {
        let config = Config::with_endpoint(
            "us-east-1",
            CredentialSource::Static {
                access_key: "AKID".into(),
                secret_key: "SECRET".into(),
            },
            server.uri(),
        );
        S3Client::new(config).await.unwrap()
    }

    fn single_page_xml(keys: &[(&str, u64)]) -> String {
        use core::fmt::Write as _;

        let mut xml = String::from(
            r#"<?xml version="1.0" encoding="UTF-8"?>
            <ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
            <Name>bucket</Name><Prefix></Prefix>
            <IsTruncated>false</IsTruncated>"#,
        );
        for (key, size) in keys {
            write!(
                xml,
                r#"<Contents><Key>{key}</Key><Size>{size}</Size>
                <ETag>"etag"</ETag><LastModified>2024-01-01T00:00:00.000Z</LastModified>
                </Contents>"#,
            )
            .unwrap();
        }
        xml.push_str("</ListBucketResult>");
        xml
    }

    #[tokio::test]
    async fn list_single_page() {
        let server = MockServer::start().await;
        let body = single_page_xml(&[("file1.txt", 100), ("file2.txt", 200)]);

        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(&body))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let mut paginator = client.list(ListRequest::new("bucket", ""));

        let page = paginator.next_page().await.unwrap().unwrap();
        assert_eq!(page.objects.len(), 2);
        assert_eq!(page.objects[0].key, "file1.txt");
        assert_eq!(page.objects[0].size, 100);
        assert_eq!(page.objects[1].key, "file2.txt");
        assert_eq!(page.objects[1].size, 200);

        // Second call returns None
        assert!(paginator.next_page().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_empty_bucket() {
        let server = MockServer::start().await;
        let body = r#"<?xml version="1.0" encoding="UTF-8"?>
            <ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
            <Name>bucket</Name><Prefix></Prefix>
            <IsTruncated>false</IsTruncated>
            </ListBucketResult>"#;

        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .expect(1)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let mut paginator = client.list(ListRequest::new("bucket", ""));

        assert!(paginator.next_page().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn list_multi_page_pagination() {
        let server = MockServer::start().await;
        let call_count = AtomicU32::new(0);

        Mock::given(method("GET"))
            .respond_with(move |_: &wiremock::Request| {
                let n = call_count.fetch_add(1, Ordering::Relaxed);
                if n == 0 {
                    ResponseTemplate::new(200).set_body_string(
                        r#"<?xml version="1.0" encoding="UTF-8"?>
                        <ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
                        <Name>bucket</Name><Prefix></Prefix>
                        <IsTruncated>true</IsTruncated>
                        <NextContinuationToken>token123</NextContinuationToken>
                        <Contents><Key>page1.txt</Key><Size>10</Size>
                        <ETag>"e1"</ETag><LastModified>2024-01-01T00:00:00.000Z</LastModified>
                        </Contents>
                        </ListBucketResult>"#,
                    )
                } else {
                    ResponseTemplate::new(200).set_body_string(
                        r#"<?xml version="1.0" encoding="UTF-8"?>
                        <ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
                        <Name>bucket</Name><Prefix></Prefix>
                        <IsTruncated>false</IsTruncated>
                        <Contents><Key>page2.txt</Key><Size>20</Size>
                        <ETag>"e2"</ETag><LastModified>2024-01-02T00:00:00.000Z</LastModified>
                        </Contents>
                        </ListBucketResult>"#,
                    )
                }
            })
            .expect(2)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let mut paginator = client.list(ListRequest::new("bucket", ""));

        let page1 = paginator.next_page().await.unwrap().unwrap();
        assert_eq!(page1.objects.len(), 1);
        assert_eq!(page1.objects[0].key, "page1.txt");

        let page2 = paginator.next_page().await.unwrap().unwrap();
        assert_eq!(page2.objects.len(), 1);
        assert_eq!(page2.objects[0].key, "page2.txt");

        assert!(paginator.next_page().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn collect_all_aggregates_pages() {
        let server = MockServer::start().await;
        let call_count = AtomicU32::new(0);

        Mock::given(method("GET"))
            .respond_with(move |_: &wiremock::Request| {
                let n = call_count.fetch_add(1, Ordering::Relaxed);
                if n == 0 {
                    ResponseTemplate::new(200).set_body_string(
                        r#"<?xml version="1.0" encoding="UTF-8"?>
                        <ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
                        <Name>b</Name><Prefix></Prefix>
                        <IsTruncated>true</IsTruncated>
                        <NextContinuationToken>t</NextContinuationToken>
                        <Contents><Key>a.txt</Key><Size>1</Size>
                        <ETag>"e"</ETag><LastModified>2024-01-01T00:00:00.000Z</LastModified>
                        </Contents>
                        </ListBucketResult>"#,
                    )
                } else {
                    ResponseTemplate::new(200).set_body_string(
                        r#"<?xml version="1.0" encoding="UTF-8"?>
                        <ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
                        <Name>b</Name><Prefix></Prefix>
                        <IsTruncated>false</IsTruncated>
                        <Contents><Key>b.txt</Key><Size>2</Size>
                        <ETag>"e"</ETag><LastModified>2024-01-02T00:00:00.000Z</LastModified>
                        </Contents>
                        </ListBucketResult>"#,
                    )
                }
            })
            .expect(2)
            .mount(&server)
            .await;

        let client = test_client(&server).await;
        let mut paginator = client.list(ListRequest::new("b", ""));
        let all = paginator.collect_all().await.unwrap();

        assert_eq!(all.len(), 2);
        assert_eq!(all[0].key, "a.txt");
        assert_eq!(all[1].key, "b.txt");
    }

    #[tokio::test]
    async fn list_request_builder() {
        let req = ListRequest::new("bucket", "prefix/").with_delimiter("/");
        assert_eq!(req.bucket, "bucket");
        assert_eq!(req.prefix, "prefix/");
        assert_eq!(req.delimiter.as_deref(), Some("/"));
    }

    #[test]
    fn object_info_from_list_object() {
        let lo = ListObject {
            key: "k".into(),
            size: 42,
            etag: "\"e\"".into(),
            last_modified: "2024-01-01".into(),
        };
        let info = ObjectInfo::from(lo);
        assert_eq!(info.key, "k");
        assert_eq!(info.size, 42);
        assert_eq!(info.etag, "\"e\"");
    }
}
