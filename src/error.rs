//! Error types for s3z.

use std::io;

use http::uri::InvalidUri;

/// Errors that can occur during s3z operations.
#[expect(clippy::error_impl_error, reason = "this is the crate's primary error type")]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Authentication or credential resolution error.
    #[error("auth error: {0}")]
    Auth(String),

    /// Value conversion error.
    #[error("conversion error: {0}")]
    Conversion(String),

    /// HTTP transport error.
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    /// HTTP request building error.
    #[error("http build error: {0}")]
    HttpBuild(#[from] http::Error),

    /// Internal error (task join, semaphore, etc.).
    #[error("internal error: {0}")]
    Internal(String),

    /// Filesystem IO error.
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    /// S3 service returned an error response.
    #[error("s3 error {code}: {message}")]
    S3 {
        /// S3 error code (e.g. `NoSuchKey`).
        code: String,
        /// Human-readable error message.
        message: String,
    },

    /// Invalid URI.
    #[error("invalid uri: {0}")]
    Uri(#[from] InvalidUri),

    /// Directory walking error.
    #[error("walk error: {0}")]
    Walk(#[from] walkdir::Error),
}

/// Convenience alias.
#[expect(clippy::absolute_paths, reason = "must use full path to avoid shadowing std Result")]
pub type Result<T> = core::result::Result<T, Error>;

/// Helper to format an [`Error::Auth`] from a missing env var.
pub(crate) fn missing_env(name: &str) -> Error {
    Error::Auth(format!("{name} not set"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_env_formats_var_name() {
        let err = missing_env("MY_VAR");
        match err {
            Error::Auth(msg) => assert_eq!(msg, "MY_VAR not set"),
            other => panic!("expected Auth, got {other:?}"),
        }
    }

    #[test]
    fn display_auth_error() {
        let err = Error::Auth("creds expired".into());
        assert_eq!(format!("{err}"), "auth error: creds expired");
    }

    #[test]
    fn display_s3_error() {
        let err = Error::S3 {
            code: "NoSuchKey".into(),
            message: "not found".into(),
        };
        assert_eq!(format!("{err}"), "s3 error NoSuchKey: not found");
    }

    #[test]
    fn display_conversion_error() {
        let err = Error::Conversion("bad utf8".into());
        assert_eq!(format!("{err}"), "conversion error: bad utf8");
    }

    #[test]
    fn display_internal_error() {
        let err = Error::Internal("task panicked".into());
        assert_eq!(format!("{err}"), "internal error: task panicked");
    }

    #[test]
    fn display_io_error() {
        let err = Error::Io(io::Error::new(io::ErrorKind::NotFound, "gone"));
        let display = format!("{err}");
        assert!(display.contains("io error"), "got: {display}");
    }

    #[test]
    fn from_io_error() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "nope");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io(_)));
    }

    #[test]
    fn from_walkdir_error() {
        // walkdir::Error can't be constructed directly without a real fs op,
        // but we can verify the From impl exists via the type system.
        // A non-existent path triggers a walkdir error.
        let walk = walkdir::WalkDir::new("/nonexistent_s3z_test_path_12345");
        for entry in walk {
            if let Err(e) = entry {
                let err: Error = e.into();
                assert!(matches!(err, Error::Walk(_)));
                return;
            }
        }
        // If the path happens to exist (very unlikely), skip
    }
}
