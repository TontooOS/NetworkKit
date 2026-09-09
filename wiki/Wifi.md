# WiFi

The `Wifi` client is read-only. It reports nearby access points and the
current connection. The settings daemon owns WiFi control: `scan` and
`status` query the daemon first (which marks known networks) and fall back
to direct local reads when the daemon is unreachable. Connect, disconnect,
enable, disable and forget exist only in the daemon protocol and have no
client here; only the Settings app may use them.

## Availability

```rust
pub fn is_available(&self) -> bool
```

- True when a wireless interface exists under `/sys/class/net`
  (`wireless`/`phy80211` sysfs entry) **and** `nmcli` is installed.
- Every other method returns `NetworkError::NotAvailable` when this is false.

## interface / radio_blocked

```rust
pub fn interface(&self) -> Option<String>
pub fn radio_blocked(&self) -> Option<bool>
```

- `interface` returns the first wireless interface name (usually `wlan0`).
- `radio_blocked` reads rfkill type `wlan`; `None` means no entry exists.

## scan

```rust
pub fn scan(&self, rescan: bool) -> Result<Vec<WifiNetwork>>
```

Queries the daemon op `wifi_list` first; on any daemon error falls back to
a direct local scan (`scan_direct`). The daemon result flags stored known
networks (`known: true`); direct scans always report `known: false`.

```rust
pub struct WifiNetwork {
    pub ssid: String,
    pub bssid: Option<String>,
    pub signal_pct: i32,
    pub frequency_mhz: Option<u32>,
    pub security: String,
    pub known: bool,
}
```

Direct scan behavior (`scan_direct`):

- Terse lines are split escape-aware; literal colons arrive as `\:`.
- Networks are deduplicated per SSID + BSSID, keeping the strongest signal.
- The result is sorted by signal percentage, descending.
- Empty security fields become `"OPEN"`.
- `channel()` maps the frequency to its channel number (2.4 GHz, channel 14
  and 5 GHz bands).

Returns `Err(NetworkError::CommandFailed)` when `nmcli` exits non-zero.

## status

```rust
pub fn status(&self) -> Result<Option<WifiStatus>>
```

Queries the daemon op `wifi_status` first; on any daemon error falls back
to a direct local read (`status_direct`). Collects details about the active
connection:

```rust
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
```

- The active network is the row of `device wifi list` whose `ACTIVE` field is
  `yes`.
- `ipv4` comes from the matching interface in the iproute2 enumeration.
- `Ok(None)` means the radio is up but no network is connected.
- `state` is read from `nmcli --terse -f STATE device status` and falls back
  to `"unknown"`.

## Write operations (daemon-only)

`connect`, `disconnect` and `connect_async` were removed from the public
API. The daemon implements `wifi_connect`, `wifi_disconnect`, `wifi_enable`,
`wifi_disable` and `wifi_forget` in its socket protocol without a public
client; only the Settings app (`com.tontoo.systemsettings`) calls them.

## Async API

Blocking calls run inside `tokio::task::spawn_blocking`:

```rust
pub async fn scan_async(&self, rescan: bool) -> Result<Vec<WifiNetwork>>
pub async fn status_async(&self) -> Result<Option<WifiStatus>>
```

## Daemon client

```rust
pub fn socket_path() -> PathBuf
pub fn daemon_available() -> bool
pub fn call(op: &str, params: Value) -> Result<Value>
```

- `socket_path` honors `SETTINGS_SOCKET`, else `/run/tontoo-settings.sock`.
- `call` sends one newline-delimited JSON frame and returns the `result`
  of a success frame. Missing socket maps to `NotAvailable`, a daemon
  error frame to `CommandFailed`, malformed frames to `ParseError`.

## Usage / Example

```rust
use networkkit::Wifi;

let wifi = Wifi::new();
for net in wifi.scan(true).unwrap_or_default() {
    println!("{:<32} {}% {}", net.ssid, net.signal_pct,
        if net.security == "OPEN" { "open" } else { "secured" });
}

if let Some(status) = wifi.status().unwrap_or(None) {
    println!("connected to {} ({:?})", status.ssid.unwrap_or_default(), status.ipv4);
}
```

## Cross References

- [Bluetooth.md](Bluetooth.md) – same discovery/polling pattern
- [LocalNetwork.md](LocalNetwork.md) – interface enumeration behind `ipv4`
