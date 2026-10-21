//! Crate-wide error type.

use std::fmt;

/// Everything that can go wrong while building a dashboard.
#[derive(Debug)]
pub enum Error {
    /// Bad or unreadable configuration.
    Config(String),
    /// A provider has no credentials configured.
    AuthMissing(String),
    /// Network / HTTP failure (including timeouts and non-2xx responses).
    Http(String),
    /// A response or file could not be parsed.
    Parse(String),
    /// Local I/O failure.
    Io(std::io::Error),
    /// The provider cannot run on this platform.
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Config(m) => write!(f, "config: {m}"),
            Error::AuthMissing(m) => write!(f, "auth missing: {m}"),
            Error::Http(m) => write!(f, "network: {m}"),
            Error::Parse(m) => write!(f, "parse: {m}"),
            Error::Io(e) => write!(f, "io: {e}"),
            Error::Unsupported(m) => write!(f, "unsupported: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Parse(e.to_string())
    }
}

impl From<ureq::Error> for Error {
    fn from(e: ureq::Error) -> Self {
        match e {
            ureq::Error::Status(401, _) | ureq::Error::Status(403, _) => {
                Error::AuthMissing(format!("{e} (token rejected or expired)"))
            }
            ureq::Error::Status(429, _) => Error::Http("rate limited (HTTP 429)".into()),
            other => Error::Http(other.to_string()),
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Parse(format!("sqlite: {e}"))
    }
}
