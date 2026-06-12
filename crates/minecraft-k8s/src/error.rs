use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("invalid mod file: {path}: {reason}")]
    InvalidMod { path: PathBuf, reason: String },

    #[error("manifest error: {0}")]
    Manifest(String),

    #[error("io error at {path}: {message}")]
    Io { path: PathBuf, message: String },

    #[error("eula must be accepted (set eula=true)")]
    EulaNotAccepted,

    #[error("probe failed with exit code {0}")]
    ProbeFailed(i32),
}

pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_error_display() {
        let err = AppError::Config("missing field".into());
        assert_eq!(err.to_string(), "configuration error: missing field");
    }

    #[test]
    fn invalid_mod_error_display() {
        let err = AppError::InvalidMod {
            path: PathBuf::from("bad.txt"),
            reason: "not a jar".into(),
        };
        assert!(err.to_string().contains("bad.txt"));
        assert!(err.to_string().contains("not a jar"));
    }

    #[test]
    fn manifest_error_display() {
        let err = AppError::Manifest("bad yaml".into());
        assert_eq!(err.to_string(), "manifest error: bad yaml");
    }

    #[test]
    fn io_error_display() {
        let err = AppError::Io {
            path: PathBuf::from("/tmp/x"),
            message: "denied".into(),
        };
        assert!(err.to_string().contains("/tmp/x"));
        assert!(err.to_string().contains("denied"));
    }

    #[test]
    fn eula_error_display() {
        let err = AppError::EulaNotAccepted;
        assert_eq!(
            err.to_string(),
            "eula must be accepted (set eula=true)"
        );
    }

    #[test]
    fn probe_failed_error_display() {
        let err = AppError::ProbeFailed(1);
        assert_eq!(err.to_string(), "probe failed with exit code 1");
    }
}
