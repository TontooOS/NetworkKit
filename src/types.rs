use crate::lang;
use std::fmt;
use std::io;

#[derive(Debug)]
pub enum NetworkError {
    NotAvailable,
    PermissionDenied,
    Timeout,
    CommandFailed(String),
    ParseError(String),
    IoError(String),
}

impl NetworkError {
    pub fn from_io(err: io::Error) -> Self {
        match err.kind() {
            io::ErrorKind::PermissionDenied => NetworkError::PermissionDenied,
            io::ErrorKind::TimedOut => NetworkError::Timeout,
            _ => NetworkError::IoError(err.to_string()),
        }
    }
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkError::NotAvailable => write!(f, "{}", lang::t("not_available")),
            NetworkError::PermissionDenied => write!(f, "{}", lang::t("permission_denied")),
            NetworkError::Timeout => write!(f, "{}", lang::t("timeout")),
            NetworkError::CommandFailed(c) => write!(f, "{}", lang::t_fmt("command_failed", c)),
            NetworkError::ParseError(e) => write!(f, "{}", lang::t_fmt("parse_error", e)),
            NetworkError::IoError(e) => write!(f, "{}", lang::t_fmt("io_error", e)),
        }
    }
}

impl std::error::Error for NetworkError {}

impl From<io::Error> for NetworkError {
    fn from(err: io::Error) -> Self {
        NetworkError::from_io(err)
    }
}

pub type Result<T> = std::result::Result<T, NetworkError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_messages_localized() {
        assert_eq!(
            NetworkError::NotAvailable.to_string(),
            lang::t("not_available")
        );
        assert_eq!(
            NetworkError::PermissionDenied.to_string(),
            lang::t("permission_denied")
        );
        assert_eq!(
            NetworkError::CommandFailed("boom".into()).to_string(),
            lang::t_fmt("command_failed", "boom")
        );
    }

    #[test]
    fn maps_io_error_kinds() {
        let denied = NetworkError::from_io(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "nope",
        ));
        assert!(matches!(denied, NetworkError::PermissionDenied));

        let timed_out =
            NetworkError::from_io(io::Error::new(io::ErrorKind::TimedOut, "slow"));
        assert!(matches!(timed_out, NetworkError::Timeout));
    }
}
