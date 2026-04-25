//! HTTP layer — request building and response parsing.

pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod retry;

use core::fmt;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC};

/// Characters that must be percent-encoded in S3 key segments.
/// S3 follows RFC 3986 — unreserved characters (A-Z a-z 0-9 - . _ ~) plus `/`
/// (path separator in keys) are kept as-is.
const S3_ENCODE_SET: &AsciiSet =
    &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~').remove(b'/');

/// A percent-encoded S3 object key.
///
/// Stores the raw key (for display / result reporting) and its percent-encoded
/// form (for URI construction). Encoding is performed once at construction.
///
/// # Examples
///
/// ```
/// use s3z::ObjectKey;
///
/// let key = ObjectKey::new("path/to/my file.txt");
/// assert_eq!(key.raw(), "path/to/my file.txt");
/// assert_eq!(key.encoded(), "path/to/my%20file.txt");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectKey {
    /// The percent-encoded key for use in URI paths.
    encoded: String,
    /// The original, unencoded key.
    raw: String,
}

impl ObjectKey {
    /// The percent-encoded form, safe for use in URI paths.
    #[inline]
    #[must_use]
    pub fn encoded(&self) -> &str {
        &self.encoded
    }

    /// Create a new object key, eagerly percent-encoding it.
    #[inline]
    #[must_use]
    #[expect(clippy::impl_trait_in_params, reason = "ergonomic constructor API")]
    pub fn new(key: impl Into<String>) -> Self {
        #[expect(clippy::shadow_reuse, reason = "intentional into() conversion")]
        let key = key.into();
        let encoded = percent_encoding::utf8_percent_encode(&key, S3_ENCODE_SET).to_string();
        Self {
            encoded,
            raw: key,
        }
    }

    /// The raw, unencoded key.
    #[inline]
    #[must_use]
    pub fn raw(&self) -> &str {
        &self.raw
    }
}

impl fmt::Display for ObjectKey {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl From<String> for ObjectKey {
    #[inline]
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for ObjectKey {
    #[inline]
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_key_unchanged() {
        let key = ObjectKey::new("data/file.txt");
        assert_eq!(key.raw(), "data/file.txt");
        assert_eq!(key.encoded(), "data/file.txt");
    }

    #[test]
    fn spaces_percent_encoded() {
        let key = ObjectKey::new("path/to/my file.txt");
        assert_eq!(key.encoded(), "path/to/my%20file.txt");
        assert_eq!(key.raw(), "path/to/my file.txt");
    }

    #[test]
    fn unicode_characters_encoded() {
        let key = ObjectKey::new("donn\u{e9}es/r\u{e9}sum\u{e9}.pdf");
        assert_eq!(key.raw(), "donn\u{e9}es/r\u{e9}sum\u{e9}.pdf");
        // Each non-ASCII byte gets percent-encoded
        assert!(!key.encoded().contains('\u{e9}'));
        assert!(key.encoded().contains("%C3%A9"));
    }

    #[test]
    fn unreserved_characters_preserved() {
        let key = ObjectKey::new("a-b.c_d~e/f");
        assert_eq!(key.encoded(), "a-b.c_d~e/f");
    }

    #[test]
    fn special_characters_encoded() {
        let key = ObjectKey::new("key with+plus&ampersand=equals");
        assert!(key.encoded().contains("%2B"));
        assert!(key.encoded().contains("%26"));
        assert!(key.encoded().contains("%3D"));
        assert!(key.encoded().contains("%20"));
    }

    #[test]
    fn empty_key() {
        let key = ObjectKey::new("");
        assert_eq!(key.raw(), "");
        assert_eq!(key.encoded(), "");
    }

    #[test]
    fn slashes_preserved() {
        let key = ObjectKey::new("a/b/c/d");
        assert_eq!(key.encoded(), "a/b/c/d");
    }

    #[test]
    fn display_shows_raw() {
        let key = ObjectKey::new("path/my file.txt");
        assert_eq!(format!("{key}"), "path/my file.txt");
    }

    #[test]
    fn from_string_impl() {
        let key: ObjectKey = String::from("test/key").into();
        assert_eq!(key.raw(), "test/key");
    }

    #[test]
    fn from_str_impl() {
        let key: ObjectKey = "test/key".into();
        assert_eq!(key.raw(), "test/key");
    }

    #[test]
    fn equality_same_key() {
        let a = ObjectKey::new("same");
        let b = ObjectKey::new("same");
        assert_eq!(a, b);
    }

    #[test]
    fn equality_different_key() {
        let a = ObjectKey::new("one");
        let b = ObjectKey::new("two");
        assert_ne!(a, b);
    }
}
