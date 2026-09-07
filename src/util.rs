use crate::types::{NetworkError, Result};
use std::path::Path;
use std::process::{Command, Output};

pub fn tool_available(name: &str) -> bool {
    let path = match std::env::var("PATH") {
        Ok(p) => p,
        Err(_) => return false,
    };

    let ext = if cfg!(windows) { ".exe" } else { "" };

    for dir in std::env::split_paths(&path) {
        if dir.join(format!("{}{}", name, ext)).is_file() {
            return true;
        }
    }

    false
}

pub fn run(program: &str, args: &[&str]) -> Result<String> {
    try_run(program, args).and_then(|out| check_output(program, out))
}

pub fn try_run(program: &str, args: &[&str]) -> Result<Output> {
    Command::new(program)
        .args(args)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                NetworkError::NotAvailable
            } else {
                NetworkError::from_io(e)
            }
        })
}

fn check_output(program: &str, output: Output) -> Result<String> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let reason = if stderr.is_empty() {
            format!("exit code {}", output.status.code().unwrap_or(-1))
        } else {
            stderr
        };
        return Err(NetworkError::CommandFailed(format!(
            "{}: {}",
            program, reason
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn sys_exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// Returns whether a radio of the given rfkill type is blocked.
///
/// Reads `/sys/class/rfkill/*/type|soft|hard`. Returns `None` when no radio of
/// that type exists. Hard blocks always report as blocked; soft blocks too.
pub fn rfkill_blocked(kind: &str) -> Option<bool> {
    let entries = std::fs::read_dir("/sys/class/rfkill").ok()?;

    let mut found = None;
    for entry in entries.filter_map(|e| e.ok()) {
        let type_path = entry.path().join("type");
        if std::fs::read_to_string(&type_path)
            .map(|t| t.trim() == kind)
            .unwrap_or(false)
        {
            let base = entry.path();
            let hard = std::fs::read_to_string(base.join("hard"))
                .map(|v| v.trim() == "1")
                .unwrap_or(false);
            let soft = std::fs::read_to_string(base.join("soft"))
                .map(|v| v.trim() == "1")
                .unwrap_or(false);
            found = Some(hard || soft);
            break;
        }
    }

    found
}

pub fn sleep_ms(ms: u64) {
    std::thread::sleep(std::time::Duration::from_millis(ms));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_missing_tool() {
        assert!(!tool_available("definitely-not-a-real-tool-xyz"));
    }

    #[test]
    fn missing_tool_maps_to_not_available() {
        let err = run("definitely-not-a-real-tool-xyz", &[]).unwrap_err();
        assert!(matches!(err, NetworkError::NotAvailable));
    }

    #[test]
    fn rfkill_missing_returns_none_on_non_linux() {
        if !Path::new("/sys/class/rfkill").exists() {
            assert_eq!(rfkill_blocked("bluetooth"), None);
        }
    }
}
