//! Client for the TontooOS settings daemon (WiFi domain).
//!
//! The daemon owns WiFi control. Read-only calls (`Wifi::scan`,
//! `Wifi::status`) query the daemon first and fall back to direct nmcli
//! reads when it is unreachable. Write operations (connect, disconnect,
//! enable, disable, forget) exist only in the daemon protocol and have no
//! client here: only the Settings app may use them.

use crate::types::{NetworkError, Result};
use std::path::PathBuf;

pub const DEFAULT_SOCKET_PATH: &str = "/run/tontoo-settings.sock";

/// Daemon socket path (`SETTINGS_SOCKET` override, else the default).
pub fn socket_path() -> PathBuf {
    std::env::var("SETTINGS_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_SOCKET_PATH))
}

/// True when the daemon socket file exists.
pub fn daemon_available() -> bool {
    socket_path().exists()
}

/// Send one request frame, return the `result` of a success frame.
///
/// Errors: socket missing/unreachable map to `NotAvailable`, a daemon
/// error frame maps to `CommandFailed`, malformed frames to `ParseError`.
#[cfg(unix)]
pub fn call(op: &str, params: serde_json::Value) -> Result<serde_json::Value> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    let path = socket_path();
    if !path.exists() {
        return Err(NetworkError::NotAvailable);
    }
    let mut stream = UnixStream::connect(&path).map_err(NetworkError::from_io)?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(15)));

    let mut line = serde_json::json!({"id": 1, "op": op, "params": params}).to_string();
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .map_err(NetworkError::from_io)?;
    stream.flush().map_err(NetworkError::from_io)?;

    let mut reader = BufReader::new(&stream);
    let mut reply = String::new();
    reader
        .read_line(&mut reply)
        .map_err(NetworkError::from_io)?;
    let frame: serde_json::Value =
        serde_json::from_str(&reply).map_err(|e| NetworkError::ParseError(e.to_string()))?;
    if frame.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        Ok(frame
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    } else {
        Err(NetworkError::CommandFailed(
            frame
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("daemon error")
                .to_string(),
        ))
    }
}

/// Non-Unix stub: the daemon protocol is Linux-only.
#[cfg(not(unix))]
pub fn call(_op: &str, _params: serde_json::Value) -> Result<serde_json::Value> {
    Err(NetworkError::NotAvailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn socket_override_and_missing_socket() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("SETTINGS_SOCKET", "/tmp/custom-test.sock");
        assert_eq!(socket_path(), PathBuf::from("/tmp/custom-test.sock"));
        std::env::set_var(
            "SETTINGS_SOCKET",
            "/nonexistent-tontoo-settings-test.sock",
        );
        let err = call("wifi_list", serde_json::json!({})).unwrap_err();
        assert!(matches!(err, NetworkError::NotAvailable));
        std::env::remove_var("SETTINGS_SOCKET");
    }
}
