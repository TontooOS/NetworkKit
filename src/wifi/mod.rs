use crate::types::{NetworkError, Result};
use crate::util::{rfkill_blocked, run, tool_available};
use serde::{Deserialize, Serialize};

pub const NMCLI: &str = "nmcli";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WifiNetwork {
    pub ssid: String,
    pub bssid: Option<String>,
    pub signal_pct: i32,
    pub frequency_mhz: Option<u32>,
    pub security: String,
    /// True when the SSID is stored as a known network (set by the
    /// settings daemon; always false for direct local scans).
    #[serde(default)]
    pub known: bool,
}

impl WifiNetwork {
    pub fn channel(&self) -> Option<u8> {
        self.frequency_mhz.and_then(freq_to_channel)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WifiStatus {
    pub interface: String,
    pub ssid: Option<String>,
    pub bssid: Option<String>,
    pub signal_pct: i32,
    pub frequency_mhz: Option<u32>,
    pub security: String,
    pub state: String,
    pub ipv4: Option<String>,
}

/// Splits one `nmcli -t` terse line into unescaped fields.
///
/// nmcli escapes literal colons as `\:` and backslashes as `\\`, so splitting
/// must skip escaped characters and unescape afterwards.
pub fn split_terse(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut escaped = false;

    for c in line.chars() {
        if escaped {
            current.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == ':' {
            fields.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }

    if !current.is_empty() || !fields.is_empty() {
        fields.push(current);
    }

    fields
}

/// Maps a 2.4 GHz or 5 GHz frequency to its WiFi channel number.
pub fn freq_to_channel(freq_mhz: u32) -> Option<u8> {
    match freq_mhz {
        2412..=2472 => Some(((freq_mhz - 2412) / 5 + 1) as u8),
        2484 => Some(14),
        5160..=5885 => Some((((freq_mhz - 5000) / 5) as i32).max(0) as u8),
        _ => None,
    }
}

fn normalize_bssid(raw: &str) -> Option<String> {
    if raw.len() != 17 || !raw.contains(':') {
        return None;
    }

    let groups: Vec<&str> = raw.split(':').collect();
    if groups.len() != 6 {
        return None;
    }

    if groups
        .iter()
        .any(|g| g.len() != 2 || !g.chars().all(|c| c.is_ascii_hexdigit()))
    {
        return None;
    }

    Some(raw.to_ascii_lowercase())
}

/// Parses one `nmcli -t -f SSID,BSSID,SIGNAL,FREQ,SECURITY dev wifi list` line.
pub fn parse_network_line(line: &str) -> Option<WifiNetwork> {
    let fields = split_terse(line);
    if fields.len() < 5 {
        return None;
    }

    let ssid = fields[0].trim().to_string();
    let bssid = normalize_bssid(fields[1].trim());
    let signal_pct = fields[2].trim().parse::<i32>().ok()?;
    let frequency_mhz = fields[3].trim().parse::<u32>().ok();
    let security = if fields[4].trim().is_empty() {
        "OPEN".to_string()
    } else {
        fields[4].trim().to_string()
    };

    Some(WifiNetwork {
        ssid,
        bssid,
        signal_pct: signal_pct.clamp(0, 100),
        frequency_mhz,
        security,
        known: false,
    })
}

/// Parses full `dev wifi list` output into sorted networks.
pub fn parse_network_list(text: &str) -> Vec<WifiNetwork> {
    let mut networks: std::collections::HashMap<(String, Option<String>), WifiNetwork> =
        std::collections::HashMap::new();

    for line in text.lines().skip(1) {
        if let Some(net) = parse_network_line(line) {
            networks
                .entry((net.ssid.clone(), net.bssid.clone()))
                .and_modify(|existing| {
                    if net.signal_pct > existing.signal_pct {
                        *existing = net.clone();
                    }
                })
                .or_insert(net);
        }
    }

    let mut list: Vec<WifiNetwork> = networks.into_values().collect();
    list.sort_by(|a, b| {
        b.signal_pct
            .cmp(&a.signal_pct)
            .then_with(|| a.ssid.cmp(&b.ssid))
    });
    list
}

#[derive(Debug, Clone)]
pub struct Wifi;

impl Wifi {
    pub fn new() -> Self {
        Self
    }

    /// True when a wireless interface exists under `/sys/class/net` and
    /// NetworkManager userland (`nmcli`) is installed.
    pub fn is_available(&self) -> bool {
        self.interface().is_some() && tool_available(NMCLI)
    }

    /// Returns whether the WLAN radio is blocked via rfkill.
    ///
    /// `None` means no rfkill entry of type `wlan` exists.
    pub fn radio_blocked(&self) -> Option<bool> {
        rfkill_blocked("wlan")
    }

    /// Name of the first wireless interface (usually `wlan0`).
    pub fn interface(&self) -> Option<String> {
        let entries = std::fs::read_dir("/sys/class/net").ok()?;
        entries
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .find(|name| {
                std::fs::metadata(format!("/sys/class/net/{}/wireless", name)).is_ok()
                    || std::fs::metadata(format!(
                        "/sys/class/net/{}/phy80211",
                        name
                    ))
                    .is_ok()
            })
    }

    /// Scans for visible networks. Queries the settings daemon first
    /// (which marks known networks); falls back to a direct local scan
    /// when the daemon is unreachable.
    pub fn scan(&self, rescan: bool) -> Result<Vec<WifiNetwork>> {
        if let Ok(networks) = self.scan_via_daemon() {
            return Ok(networks);
        }
        self.scan_direct(rescan)
    }

    fn scan_via_daemon(&self) -> Result<Vec<WifiNetwork>> {
        let result = crate::daemon::call("wifi_list", serde_json::json!({}))?;
        let networks: Vec<WifiNetwork> = serde_json::from_value(
            result.get("networks").cloned().unwrap_or(serde_json::Value::Null),
        )
        .map_err(|e| NetworkError::ParseError(e.to_string()))?;
        Ok(networks)
    }

    /// Direct local scan via nmcli. Used as a fallback when the settings
    /// daemon is unreachable. With `rescan = true` an active scan is
    /// triggered before listing.
    pub fn scan_direct(&self, rescan: bool) -> Result<Vec<WifiNetwork>> {
        if !self.is_available() {
            return Err(NetworkError::NotAvailable);
        }

        let mut args = vec![
            "--terse",
            "--escape",
            "yes",
            "-f",
            "SSID,BSSID,SIGNAL,FREQ,SECURITY",
            "device",
            "wifi",
            "list",
        ];
        if rescan {
            args.push("--rescan");
            args.push("yes");
        }

        let text = run(NMCLI, &args)?;
        Ok(parse_network_list(&text))
    }

    /// Details about the currently connected network, if any. Queries
    /// the settings daemon first; falls back to a direct local read
    /// when the daemon is unreachable.
    pub fn status(&self) -> Result<Option<WifiStatus>> {
        if let Ok(status) = self.status_via_daemon() {
            return Ok(status);
        }
        self.status_direct()
    }

    fn status_via_daemon(&self) -> Result<Option<WifiStatus>> {
        let result = crate::daemon::call("wifi_status", serde_json::json!({}))?;
        let status: Option<WifiStatus> = serde_json::from_value(
            result.get("status").cloned().unwrap_or(serde_json::Value::Null),
        )
        .map_err(|e| NetworkError::ParseError(e.to_string()))?;
        Ok(status)
    }

    /// Direct local status read via nmcli. Used as a fallback when the
    /// settings daemon is unreachable.
    pub fn status_direct(&self) -> Result<Option<WifiStatus>> {
        if !self.is_available() {
            return Err(NetworkError::NotAvailable);
        }

        let iface = self
            .interface()
            .ok_or(NetworkError::NotAvailable)?;

        let text = run(
            NMCLI,
            &[
                "--terse",
                "--escape",
                "yes",
                "-f",
                "ACTIVE,SSID,BSSID,SIGNAL,FREQ,SECURITY",
                "device",
                "wifi",
                "list",
                "ifname",
                &iface,
            ],
        )?;

        let mut status = WifiStatus {
            interface: iface.clone(),
            ..WifiStatus::default()
        };

        for line in text.lines().skip(1) {
            let fields = split_terse(line);
            if fields.first().map(String::as_str) == Some("yes") {
                status.ssid = Some(fields.get(1).cloned().unwrap_or_default());
                status.bssid = fields.get(2).and_then(|b| normalize_bssid(b.trim()));
                status.signal_pct = fields
                    .get(3)
                    .and_then(|s| s.trim().parse::<i32>().ok())
                    .unwrap_or(0);
                status.frequency_mhz =
                    fields.get(4).and_then(|f| f.trim().parse::<u32>().ok());
                status.security = fields.get(5).cloned().unwrap_or_default();
                break;
            }
        }

        if status.ssid.is_none() {
            return Ok(None);
        }

        status.state = run(
            NMCLI,
            &["--terse", "-f", "STATE", "device", "status"],
        )
        .ok()
        .and_then(|out| {
            out.lines()
                .map(split_terse)
                .find(|row| row.first().map(String::as_str) == Some(iface.as_str()))
                .and_then(|row| row.get(1).cloned())
        })
        .unwrap_or_else(|| "unknown".to_string());

        if let Ok(interfaces) = crate::localnet::LocalNetwork::new().interfaces() {
            status.ipv4 = interfaces
                .iter()
                .find(|i| i.name == iface)
                .and_then(|i| i.ipv4.first().map(|a| a.ip.to_string()));
        }

        Ok(Some(status))
    }

    // ---- async wrappers (read-only; daemon-first like the sync calls) ----

    pub async fn scan_async(&self, rescan: bool) -> Result<Vec<WifiNetwork>> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.scan(rescan))
            .await
            .map_err(|e| NetworkError::IoError(e.to_string()))?
    }

    pub async fn status_async(&self) -> Result<Option<WifiStatus>> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.status())
            .await
            .map_err(|e| NetworkError::IoError(e.to_string()))?
    }
}

impl Default for Wifi {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_terse_lines_with_escapes() {
        assert_eq!(
            split_terse(r"My\:Net\\Work:aa\:bb\:cc\:dd\:ee\:ff:82"),
            vec![
                "My:Net\\Work".to_string(),
                "aa:bb:cc:dd:ee:ff".to_string(),
                "82".to_string(),
            ]
        );
        assert_eq!(split_terse(""), Vec::<String>::new());
        assert_eq!(split_terse("a:b"), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn normalizes_bssids() {
        assert_eq!(
            normalize_bssid("A0:F3:C1:3B:6F:90"),
            Some("a0:f3:c1:3b:6f:90".to_string())
        );
        assert_eq!(normalize_bssid("a0:f3:c1"), None);
        assert_eq!(normalize_bssid(""), None);
        assert_eq!(normalize_bssid("zz:zz:zz:zz:zz:zz"), None);
    }

    #[test]
    fn parses_network_line() {
        let net = parse_network_line(r"HomeNet:a0\:f3\:c1\:3b\:6f\:90:82:2437:WPA2").unwrap();
        assert_eq!(net.ssid, "HomeNet");
        assert_eq!(net.bssid.as_deref(), Some("a0:f3:c1:3b:6f:90"));
        assert_eq!(net.signal_pct, 82);
        assert_eq!(net.channel(), Some(6));
        assert_eq!(net.security, "WPA2");

        let open = parse_network_line("Cafe::55:5180:").unwrap();
        assert_eq!(open.security, "OPEN");
        assert_eq!(open.bssid, None);
        assert_eq!(open.signal_pct, 55);
    }

    #[test]
    fn rejects_broken_network_lines() {
        assert!(parse_network_line("").is_none());
        assert!(parse_network_line("only:ssid").is_none());
        assert!(parse_network_line("S:bssid:notanumber:2437:WPA2").is_none());
    }

    #[test]
    fn parses_list_sorted_and_deduped() {
        let sample = "SSID:BSSID:SIGNAL:FREQ:SECURITY\n\
HomeNet:a0\\:f3\\:c1\\:3b\\:6f\\:90:64:2437:WPA2\n\
HomeNet:a0\\:f3\\:c1\\:3b\\:6f\\:90:82:2437:WPA2\n\
Cafe:00\\:1c\\:42\\:1f\\:65\\:e9:55:5180:\n\
Weak:11\\:22\\:33\\:44\\:55\\:66:12:2412:WEP\n\
broken-line-without-fields\n";
        let list = parse_network_list(sample);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].ssid, "HomeNet");
        assert_eq!(list[0].signal_pct, 82);
        assert_eq!(list[1].ssid, "Cafe");
        assert_eq!(list[2].security, "WEP");
    }

    #[test]
    fn maps_frequencies_to_channels() {
        assert_eq!(freq_to_channel(2412), Some(1));
        assert_eq!(freq_to_channel(2437), Some(6));
        assert_eq!(freq_to_channel(2484), Some(14));
        assert_eq!(freq_to_channel(5180), Some(36));
        assert_eq!(freq_to_channel(5500), Some(100));
        assert_eq!(freq_to_channel(9999), None);
    }
}
