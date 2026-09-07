use crate::types::{NetworkError, Result};
use crate::util::{rfkill_blocked, run, sleep_ms, tool_available};

pub const BLUETOCTL: &str = "bluetoothctl";
const SYS_CLASS_BLUETOOTH: &str = "/sys/class/bluetooth";

#[derive(Debug, Clone)]
pub struct Adapter {
    pub name: String,
    pub address: String,
    pub alias: String,
    pub powered: bool,
    pub discovering: bool,
    pub pairable: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Device {
    pub address: String,
    pub name: Option<String>,
    pub paired: bool,
    pub trusted: bool,
    pub connected: bool,
    pub rssi_pct: Option<i32>,
}

/// Converts an RSSI value in dBm to a rough signal percentage.
///
/// Values at or above -50 dBm map to 100 percent, values at or below -100 dBm
/// to 0 percent; everything in between is linear.
pub fn rssi_to_pct(rssi: i32) -> i32 {
    (((rssi + 100) * 2) as f64).clamp(0.0, 100.0) as i32
}

/// Parses one `bluetoothctl` device event line.
///
/// Accepts plain `Device AA:BB:CC:DD:EE:FF Name`, `[NEW] Device ...` listings
/// and `[CHG] Device ... Property: Value` changes. Returns the address plus
/// either a name or an RSSI percentage when the line carries one.
pub fn parse_device_event(line: &str) -> Option<Device> {
    let trimmed = line.trim();
    let body = trimmed
        .strip_prefix("[NEW] ")
        .or_else(|| trimmed.strip_prefix("[CHG] "))
        .or_else(|| trimmed.strip_prefix("[DEL] "))
        .unwrap_or(trimmed);

    let rest = body.strip_prefix("Device ").unwrap_or(body);
    let mut parts = rest.splitn(2, ' ');
    let address = normalize_address(parts.next()?)?;

    if let Some(property) = parts.next() {
        if let Some(rssi) = property.trim().strip_prefix("RSSI:") {
            let dbm = rssi.trim().parse::<i32>().ok()?;
            return Some(Device {
                address,
                rssi_pct: Some(rssi_to_pct(dbm)),
                ..Device::default()
            });
        }
        if property.trim().starts_with("Connected:") {
            return None;
        }
        return Some(Device {
            address,
            name: Some(property.trim().to_string()),
            ..Device::default()
        });
    }

    Some(Device::default())
}

fn normalize_address(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    let sep = if raw.contains(':') {
        ':'
    } else if raw.contains('-') {
        '-'
    } else {
        return None;
    };

    let parts: Vec<&str> = raw.split(sep).collect();
    if parts.len() != 6 {
        return None;
    }

    let mut groups = Vec::with_capacity(6);
    for part in parts {
        if part.len() != 2 || !part.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        groups.push(part.to_ascii_uppercase());
    }

    Some(groups.join(":"))
}

/// Parses `bluetoothctl -- info <address>` output into a [`Device`].
pub fn parse_info_output(text: &str) -> Device {
    let mut device = Device::default();

    for line in text.lines() {
        let line = line.trim();
        if device.address.is_empty() {
            if let Some(rest) = line.strip_prefix("Device ") {
                let token = rest.split_whitespace().next().unwrap_or_default();
                if let Some(addr) = normalize_address(token) {
                    device.address = addr;
                }
                continue;
            }
        }

        if let Some(value) = line.strip_prefix("Name:") {
            device.name.get_or_insert_with(|| value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("Alias:") {
            device
                .name
                .get_or_insert_with(|| value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("Paired:") {
            device.paired = value.trim() == "yes";
        } else if let Some(value) = line.strip_prefix("Trusted:") {
            device.trusted = value.trim() == "yes";
        } else if let Some(value) = line.strip_prefix("Connected:") {
            device.connected = value.trim() == "yes";
        } else if let Some(value) = line.strip_prefix("RSSI:") {
            if let Ok(dbm) = value.trim().parse::<i32>() {
                device.rssi_pct = Some(rssi_to_pct(dbm));
            }
        }
    }

    device
}

/// Parses `bluetoothctl show` output into an [`Adapter`].
pub fn parse_show_output(text: &str, name: &str) -> Adapter {
    let mut adapter = Adapter {
        name: name.to_string(),
        address: String::new(),
        alias: String::new(),
        powered: false,
        discovering: false,
        pairable: false,
    };

    for line in text.lines() {
        let line = line.trim();
        if adapter.address.is_empty() {
            if let Some(rest) = line.strip_prefix("Controller ") {
                let token = rest.split_whitespace().next().unwrap_or_default();
                if let Some(addr) = normalize_address(token) {
                    adapter.address = addr;
                }
                continue;
            }
        }
        if let Some(value) = line.strip_prefix("Alias:") {
            adapter.alias = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("Powered:") {
            adapter.powered = value.trim() == "yes";
        } else if let Some(value) = line.strip_prefix("Discovering:") {
            adapter.discovering = value.trim() == "yes";
        } else if let Some(value) = line.strip_prefix("Pairable:") {
            adapter.pairable = value.trim() == "yes";
        }
    }

    adapter
}

/// Runs one interactive `bluetoothctl` session.
///
/// Each tuple is a stdin line followed by how long to wait after it. An empty
/// line only sleeps, which keeps a scan running without sending a command.
/// A final `quit` is always appended so the process exits and its full output
/// can be captured.
fn run_session(commands: &[(&str, u64)]) -> Result<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(BLUETOCTL)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                NetworkError::NotAvailable
            } else {
                NetworkError::from_io(e)
            }
        })?;

    {
        let stdin = child.stdin.as_mut().expect("stdin was piped");
        for (line, delay_ms) in commands {
            if !line.is_empty() {
                writeln!(stdin, "{}", line).map_err(NetworkError::from_io)?;
                let _ = stdin.flush();
            }
            sleep_ms(*delay_ms);
        }
        writeln!(stdin, "quit").map_err(NetworkError::from_io)?;
        let _ = stdin.flush();
    }

    let output = child.wait_with_output().map_err(NetworkError::from_io)?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn adapter_name() -> String {
    std::fs::read_dir(SYS_CLASS_BLUETOOTH)
        .ok()
        .and_then(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .find(|n| n.starts_with("hci") && !n.contains(':'))
        })
        .unwrap_or_else(|| "hci".to_string())
}

#[derive(Debug, Clone)]
pub struct Bluetooth;

impl Bluetooth {
    pub fn new() -> Self {
        Self
    }

    /// True when an adapter exists in `/sys/class/bluetooth` and BlueZ userland
    /// (`bluetoothctl`) is installed.
    pub fn is_available(&self) -> bool {
        crate::util::sys_exists(SYS_CLASS_BLUETOOTH)
            && std::fs::read_dir(SYS_CLASS_BLUETOOTH)
                .map(|mut entries| entries.next().is_some())
                .unwrap_or(false)
            && tool_available(BLUETOCTL)
    }

    /// Returns whether the bluetooth radio is blocked via rfkill.
    ///
    /// `None` means no rfkill entry of type `bluetooth` exists.
    pub fn radio_blocked(&self) -> Option<bool> {
        rfkill_blocked("bluetooth")
    }

    /// Reads the default controller via `bluetoothctl show`.
    pub fn adapter(&self) -> Result<Adapter> {
        if !self.is_available() {
            return Err(NetworkError::NotAvailable);
        }

        let text = run(BLUETOCTL, &["--", "show"])?;
        Ok(parse_show_output(&text, &adapter_name()))
    }

    /// Powers the default controller on or off.
    pub fn set_powered(&self, powered: bool) -> Result<()> {
        if !self.is_available() {
            return Err(NetworkError::NotAvailable);
        }

        let state = if powered { "on" } else { "off" };
        run_session(&[(format!("power {}", state).as_str(), 400)])?;
        if self.adapter()?.powered != powered {
            return Err(NetworkError::PermissionDenied);
        }
        Ok(())
    }

    /// Lists paired or connected devices without scanning.
    pub fn devices(&self, filter: DeviceFilter) -> Result<Vec<Device>> {
        if !self.is_available() {
            return Err(NetworkError::NotAvailable);
        }

        let arg = match filter {
            DeviceFilter::All => "devices",
            DeviceFilter::Paired => "devices Paired",
            DeviceFilter::Connected => "devices Connected",
        };

        let text = run_session(&[(arg, 300)])?;
        Ok(collect_devices(&text))
    }

    /// Scans for nearby devices for `timeout_secs` seconds and returns every
    /// unique address that appeared during the scan, enriched where possible.
    pub fn discover(&self, timeout_secs: u64) -> Result<Vec<Device>> {
        if !self.is_available() {
            return Err(NetworkError::NotAvailable);
        }

        let commands = vec![
            ("scan on".to_string(), 500),
            (String::new(), timeout_secs * 1000),
            ("scan off".to_string(), 400),
            ("devices".to_string(), 300),
        ];
        let refs: Vec<(&str, u64)> = commands
            .iter()
            .map(|(c, d)| (c.as_str(), *d))
            .collect();

        let text = run_session(&refs)?;

        let mut merged: std::collections::HashMap<String, Device> =
            std::collections::HashMap::new();
        for line in text.lines() {
            if let Some(event) = parse_device_event(line) {
                let entry = merged
                    .entry(event.address.clone())
                    .or_insert_with(|| Device {
                        address: event.address.clone(),
                        ..Device::default()
                    });
                if event.name.is_some() {
                    entry.name = event.name;
                }
                if event.rssi_pct.is_some() {
                    entry.rssi_pct = event.rssi_pct;
                }
            }
        }

        let mut devices: Vec<Device> = merged.into_values().collect();
        devices.sort_by(|a, b| b.rssi_pct.unwrap_or(-1).cmp(&a.rssi_pct.unwrap_or(-1)));
        Ok(devices)
    }

    /// Fetches detailed information about a single device.
    pub fn device_info(&self, address: &str) -> Result<Device> {
        if !self.is_available() {
            return Err(NetworkError::NotAvailable);
        }

        let text = run(BLUETOCTL, &["--", "info", address])?;
        let device = parse_info_output(&text);
        if device.address.is_empty() {
            return Err(NetworkError::ParseError(format!(
                "no such device {}",
                address
            )));
        }
        Ok(device)
    }

    /// Pairs with a device and waits up to `timeout_secs` for the pairing to
    /// complete. Devices are trusted afterwards.
    pub fn pair(&self, address: &str, timeout_secs: u64) -> Result<Device> {
        if !self.is_available() {
            return Err(NetworkError::NotAvailable);
        }

        run_session(&[
            (format!("pair {}", address).as_str(), 300),
            (format!("trust {}", address).as_str(), 200),
        ])?;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
        while std::time::Instant::now() < deadline {
            let device = self.device_info(address)?;
            if device.paired {
                return Ok(device);
            }
            sleep_ms(500);
        }

        Err(NetworkError::Timeout)
    }

    /// Opens a connection to a paired device and verifies it came up.
    pub fn connect(&self, address: &str, timeout_secs: u64) -> Result<Device> {
        if !self.is_available() {
            return Err(NetworkError::NotAvailable);
        }

        run_session(&[(format!("connect {}", address).as_str(), 300)])?;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
        while std::time::Instant::now() < deadline {
            let device = self.device_info(address)?;
            if device.connected {
                return Ok(device);
            }
            sleep_ms(500);
        }

        Err(NetworkError::Timeout)
    }

    /// Disconnects a device.
    pub fn disconnect(&self, address: &str) -> Result<()> {
        if !self.is_available() {
            return Err(NetworkError::NotAvailable);
        }

        run_session(&[(format!("disconnect {}", address).as_str(), 600)])?;
        Ok(())
    }

    /// Removes a paired device from the adapter.
    pub fn remove_device(&self, address: &str) -> Result<()> {
        if !self.is_available() {
            return Err(NetworkError::NotAvailable);
        }

        run_session(&[(format!("remove {}", address).as_str(), 600)])?;
        Ok(())
    }

    // ---- async wrappers ----

    pub async fn adapter_async(&self) -> Result<Adapter> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.adapter())
            .await
            .map_err(|e| NetworkError::IoError(e.to_string()))?
    }

    pub async fn discover_async(&self, timeout_secs: u64) -> Result<Vec<Device>> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.discover(timeout_secs))
            .await
            .map_err(|e| NetworkError::IoError(e.to_string()))?
    }

    pub async fn connect_async(&self, address: &str, timeout_secs: u64) -> Result<Device> {
        let this = self.clone();
        let address = address.to_string();
        tokio::task::spawn_blocking(move || this.connect(&address, timeout_secs))
            .await
            .map_err(|e| NetworkError::IoError(e.to_string()))?
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeviceFilter {
    All,
    Paired,
    Connected,
}

impl Default for Bluetooth {
    fn default() -> Self {
        Self::new()
    }
}

fn collect_devices(text: &str) -> Vec<Device> {
    text.lines()
        .filter_map(parse_device_event)
        .filter(|d| !d.address.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_addresses() {
        assert_eq!(
            normalize_address("aa:bb:cc:dd:ee:ff"),
            Some("AA:BB:CC:DD:EE:FF".to_string())
        );
        assert_eq!(
            normalize_address("AA-BB-CC-DD-EE-FF"),
            Some("AA:BB:CC:DD:EE:FF".to_string())
        );
        assert_eq!(normalize_address("nope"), None);
        assert_eq!(normalize_address(""), None);
    }

    #[test]
    fn parses_plain_and_new_device_lines() {
        let plain = parse_device_event("Device AA:BB:CC:DD:EE:FF Galaxy Buds").unwrap();
        assert_eq!(plain.address, "AA:BB:CC:DD:EE:FF");
        assert_eq!(plain.name.as_deref(), Some("Galaxy Buds"));

        let new = parse_device_event("[NEW] Device 00:11:22:33:44:55 ThinkPad Keyboard").unwrap();
        assert_eq!(new.address, "00:11:22:33:44:55");
        assert_eq!(new.name.as_deref(), Some("ThinkPad Keyboard"));
    }

    #[test]
    fn parses_rssi_change_events() {
        let chg = parse_device_event("[CHG] Device AA:BB:CC:DD:EE:FF RSSI: -63").unwrap();
        assert_eq!(chg.address, "AA:BB:CC:DD:EE:FF");
        assert_eq!(chg.rssi_pct, Some(74));
        assert!(chg.name.is_none());

        assert!(parse_device_event("[CHG] Device AA:BB:CC:DD:EE:FF Connected: yes").is_none());
    }

    #[test]
    fn rssi_conversion_clamps() {
        assert_eq!(rssi_to_pct(-40), 100);
        assert_eq!(rssi_to_pct(-75), 50);
        assert_eq!(rssi_to_pct(-110), 0);
    }

    #[test]
    fn parses_info_output() {
        let sample = "\
Device AA:BB:CC:DD:EE:FF (public)
\tName: Pixel 7
\tAlias: Pixel 7
\tPaired: yes
\tBonded: yes
\tTrusted: no
\tBlocked: no
\tLegacyPairing: no
\tConnected: yes
\tRSSI: -52
";

        let device = parse_info_output(sample);
        assert_eq!(device.address, "AA:BB:CC:DD:EE:FF");
        assert_eq!(device.name.as_deref(), Some("Pixel 7"));
        assert!(device.paired);
        assert!(!device.trusted);
        assert!(device.connected);
        assert_eq!(device.rssi_pct, Some(96));
    }

    #[test]
    fn parses_show_output() {
        let sample = "\
Controller AA:BB:CC:DD:EE:FF (public)
\tName: BlueZ 5.66
\tAlias: tontoo-box
\tClass: 0x00000000
\tPowered: yes
\tPairable: yes
\tUUIDs: Headset Audio Gateway
\tModalias: usb:v1D6Bp0246d0540
\tDiscovering: no
";

        let adapter = parse_show_output(sample, "hci0");
        assert_eq!(adapter.address, "AA:BB:CC:DD:EE:FF");
        assert_eq!(adapter.alias, "tontoo-box");
        assert_eq!(adapter.name, "hci0");
        assert!(adapter.powered);
        assert!(adapter.pairable);
        assert!(!adapter.discovering);
    }

    #[test]
    fn collect_dedupes_nothing_here_but_parses_lines() {
        let sample = "Device AA:BB:CC:DD:EE:FF One\nDevice 11:22:33:44:55:66 Two\njunk\n";
        let devices = collect_devices(sample);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[1].address, "11:22:33:44:55:66");
    }
}
