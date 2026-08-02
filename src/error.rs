//! Crate-wide error type.
//!
//! We use `thiserror` to derive `Display` + `From` impls. Every fallible
//! function in the library returns [`Result<T>`]; tests and `main` are
//! allowed to use `anyhow::Result` for ergonomics.

use std::path::PathBuf;

/// Every error this crate can produce. One variant per failure category
/// so callers can match and react (e.g. `Error::NotInitialized` for
/// "you ran `sl search` before `sl init`").
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The cwd has no `.localrag1/` yet — user needs to run `sl init`.
    #[error("no .localrag1/ found in {0:?} or its parents — run `sl init` first")]
    NotInitialized(PathBuf),

    /// Config file exists but is malformed TOML.
    #[error("invalid config at {path:?}: {source}")]
    InvalidConfig {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    /// IO error during a specific operation.
    #[error("io error during {operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },

    /// SQLite error (rusqlite).
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("walk error at {root:?}: {source}")]
    Walk {
        root: PathBuf,
        #[source]
        source: walkdir::Error,
    },

    /// The LLM provider returned a transport-level error.
    #[error("llm transport error: {0}")]
    LlmTransport(String),

    /// The LLM provider returned an unexpected / malformed response.
    #[error("llm response error: {0}")]
    LlmResponse(String),

    /// A tool call from the LLM was rejected for safety reasons.
    #[error("llm tool call rejected: {0}")]
    LlmToolRejected(String),

    /// Catch-all for other problems.
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Convenience for ad-hoc stringly-typed errors.
    pub fn other(msg: impl Into<String>) -> Self {
        Error::Other(msg.into())
    }
}

/// Crate-wide result alias. Use this everywhere instead of `Result<T, Error>`.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_includes_path_for_not_initialized() {
        let err = Error::NotInitialized("/tmp/x".into());
        let s = err.to_string();
        // The error message should help the user fix the problem.
        assert!(s.contains(".localrag1"), "got: {s}");
        assert!(s.contains("sl init"), "got: {s}");
    }

    #[test]
    fn from_rusqlite() {
        // Force an error via bad SQL on an in-memory connection — proves the
        // From<rusqlite::Error> impl on Error wires up correctly.
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let r: std::result::Result<(), rusqlite::Error> =
            conn.execute_batch("this is not sql");
        let err: Error = r.unwrap_err().into();
        assert!(matches!(err, Error::Sqlite(_)));
    }
}
