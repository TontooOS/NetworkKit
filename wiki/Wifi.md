# WiFi

The `Wifi` client drives wireless hardware through NetworkManager (`nmcli`).
It scans for access points, reports the current connection with signal
strength and IP address and connects or disconnects networks.

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

Runs `nmcli --terse --escape yes -f SSID,BSSID,SIGNAL,FREQ,SECURITY device wifi list`
and triggers an active rescan when requested.

```rust
pub struct WifiNetwork {
    pub ssid: String,
    pub bssid: Option<String>,
    pub signal_pct: i32,
    pub frequency_mhz: Option<u32>,
    pub security: String,
}
```

Behavior:

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

Collects details about the active connection:

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

## connect

```rust
pub fn connect(&self, ssid: &str, password: Option<&str>, hidden: bool) -> Result<WifiStatus>
```

Blocks until NetworkManager finishes the association. Pass `None` as password
for open networks; `hidden = true` adds the `hidden yes` flags. The returned
status is verified against the requested SSID; on mismatch the method returns
`Err(NetworkError::CommandFailed)`.

## disconnect

```rust
pub fn disconnect(&self) -> Result<()>
```

Runs `nmcli device disconnect <interface>`.

## Async API

Blocking calls run inside `tokio::task::spawn_blocking`:

```rust
pub async fn scan_async(&self, rescan: bool) -> Result<Vec<WifiNetwork>>
pub async fn status_async(&self) -> Result<Option<WifiStatus>>
pub async fn connect_async(
    &self,
    ssid: &str,
    password: Option<&str>,
    hidden: bool,
) -> Result<WifiStatus>
```

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
