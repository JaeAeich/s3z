//! Credential resolution and request signing.

mod sigv4;

use std::env;

pub(crate) use sigv4::sign_request;

use crate::error::{self, Result};

/// How to obtain AWS credentials.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum CredentialSource {
    /// Read from `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` env vars,
    /// plus `AWS_SESSION_TOKEN` if set.
    Env,
    /// Temporary credentials (STS, SSO, assumed roles).
    Session {
        /// AWS access key ID.
        access_key: String,
        /// AWS secret access key.
        secret_key: String,
        /// AWS session token.
        session_token: String,
    },
    /// Explicit access key / secret key pair.
    Static {
        /// AWS access key ID.
        access_key: String,
        /// AWS secret access key.
        secret_key: String,
    },
}

/// Resolved credentials ready for signing.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub(crate) struct Credentials {
    /// AWS access key ID.
    pub(crate) access_key: String,
    /// AWS secret access key.
    pub(crate) secret_key: String,
    /// AWS session token for temporary credentials.
    pub(crate) session_token: Option<String>,
}

/// Resolve [`CredentialSource`] into concrete [`Credentials`].
///
/// # Errors
///
/// Returns [`error::Error::Auth`] if env vars are missing when using
/// [`CredentialSource::Env`].
pub(crate) fn resolve(source: &CredentialSource) -> Result<Credentials> {
    match source {
        CredentialSource::Env => {
            let access_key = env::var("AWS_ACCESS_KEY_ID")
                .map_err(|_e| error::missing_env("AWS_ACCESS_KEY_ID"))?;
            let secret_key = env::var("AWS_SECRET_ACCESS_KEY")
                .map_err(|_e| error::missing_env("AWS_SECRET_ACCESS_KEY"))?;
            let session_token = env::var("AWS_SESSION_TOKEN").ok().filter(|t| !t.is_empty());
            Ok(Credentials {
                access_key,
                secret_key,
                session_token,
            })
        },
        CredentialSource::Session {
            access_key,
            secret_key,
            session_token,
        } => {
            Ok(Credentials {
                access_key: access_key.clone(),
                secret_key: secret_key.clone(),
                session_token: Some(session_token.clone()),
            })
        },
        CredentialSource::Static {
            access_key,
            secret_key,
        } => {
            Ok(Credentials {
                access_key: access_key.clone(),
                secret_key: secret_key.clone(),
                session_token: None,
            })
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_credentials_resolved() {
        let source = CredentialSource::Static {
            access_key: "AKID".into(),
            secret_key: "SECRET".into(),
        };
        let creds = resolve(&source).unwrap();
        assert_eq!(creds.access_key, "AKID");
        assert_eq!(creds.secret_key, "SECRET");
    }

    #[test]
    fn session_credentials_resolved() {
        let source = CredentialSource::Session {
            access_key: "AKID".into(),
            secret_key: "SECRET".into(),
            session_token: "TOKEN".into(),
        };
        let creds = resolve(&source).unwrap();
        assert_eq!(creds.session_token.as_deref(), Some("TOKEN"));
    }

    #[test]
    fn static_credentials_have_no_session_token() {
        let source = CredentialSource::Static {
            access_key: "AKID".into(),
            secret_key: "SECRET".into(),
        };
        assert!(resolve(&source).unwrap().session_token.is_none());
    }

    #[test]
    fn static_credentials_empty_strings_accepted() {
        let source = CredentialSource::Static {
            access_key: String::new(),
            secret_key: String::new(),
        };
        let creds = resolve(&source).unwrap();
        assert_eq!(creds.access_key, "");
        assert_eq!(creds.secret_key, "");
    }

    #[test]
    fn missing_env_helper_formats_correctly() {
        let err = error::missing_env("AWS_ACCESS_KEY_ID");
        match err {
            error::Error::Auth(msg) => {
                assert_eq!(msg, "AWS_ACCESS_KEY_ID not set");
            },
            other => panic!("expected Auth error, got {other:?}"),
        }
    }
}
