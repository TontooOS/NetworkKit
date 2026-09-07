# Bluetooth

The `Bluetooth` client talks to the default BlueZ controller through the
`bluetoothctl` userland. It reports adapter and rfkill state, lists paired or
connected devices, runs discovery scans and performs pairing, trust, connect
and removal operations.

## Availability

```rust
pub fn is_available(&self) -> bool
```

- True when `/sys/class/bluetooth` contains an adapter entry **and**
  `bluetoothctl` is installed.
- Every other method returns `NetworkError::NotAvailable` when this is false.

## radio_blocked

```rust
pub fn radio_blocked(&self) -> Option<bool>
```

Reads the kernel rfkill state for radios of type `bluetooth`.

| Return | Meaning |
|---|---|
| `Some(true)` | Soft block or hard block is active |
| `Some(false)` | Radio is unblocked |
| `None` | No rfkill entry of this type exists |

## adapter

```rust
pub fn adapter(&self) -> Result<Adapter>
```

Parses `bluetoothctl -- show` for the default controller.

```rust
pub struct Adapter {
    pub name: String,
    pub address: String,
    pub alias: String,
    pub powered: bool,
    pub discovering: bool,
    pub pairable: bool,
}
```

Returns `Err(NetworkError::NotAvailable)` without hardware and
`Err(NetworkError::CommandFailed)` when BlueZ answers with a failure.

## set_powered

```rust
pub fn set_powered(&self, powered: bool) -> Result<()>
```

Runs a `power on` / `power off` session against the default controller and
verifies the new state via [`adapter`](#adapter). Returns
`Err(NetworkError::PermissionDenied)` when the controller stays in its old
state (for example when PolicyKit denies the operation).

## devices

```rust
pub fn devices(&self, filter: DeviceFilter) -> Result<Vec<Device>>
```

Lists known devices without scanning.

```rust
pub enum DeviceFilter { All, Paired, Connected }
```

The result is built from `bluetoothctl devices [Paired|Connected]` lines;
names come from the listing itself, no per-device `info` round trips happen.

## discover

```rust
pub fn discover(&self, timeout_secs: u64) -> Result<Vec<Device>>
```

Starts `scan on`, keeps it running for `timeout_secs` seconds, stops the scan
and merges every device event that appeared during the session:

- `[NEW]`/plain `Device AA:BB:CC:DD:EE:FF Name` lines fill addresses and names.
- `[CHG] Device ... RSSI: -63` events update the signal strength.
- The list is sorted by RSSI percentage, strongest first.

RSSI percentages are computed linearly: -50 dBm maps to 100 percent, -100 dBm
to 0 percent. Devices already known to BlueZ appear even when they were never
seen during the scan window because the final `devices` listing is included.

## device_info

```rust
pub fn device_info(&self, address: &str) -> Result<Device>
```

Parses `bluetoothctl -- info <address>`:

```rust
pub struct Device {
    pub address: String,
    pub name: Option<String>,
    pub paired: bool,
    pub trusted: bool,
    pub connected: bool,
    pub rssi_pct: Option<i32>,
}
```

Returns `Err(NetworkError::ParseError)` when BlueZ does not know the address.

## pair / connect / disconnect / remove_device

```rust
pub fn pair(&self, address: &str, timeout_secs: u64) -> Result<Device>
pub fn connect(&self, address: &str, timeout_secs: u64) -> Result<Device>
pub fn disconnect(&self, address: &str) -> Result<()>
pub fn remove_device(&self, address: &str) -> Result<()>
```

- `pair` starts pairing, marks the device as trusted afterwards and polls
  `device_info` until `Paired: yes`; returns `Err(NetworkError::Timeout)`
  after `timeout_secs`.
- `connect` opens the connection and polls until `Connected: yes`.
- `disconnect` and `remove_device` fire one session each and return `Ok(())`
  regardless of whether the device existed.

## Async API

Blocking calls run inside `tokio::task::spawn_blocking`:

```rust
pub async fn adapter_async(&self) -> Result<Adapter>
pub async fn discover_async(&self, timeout_secs: u64) -> Result<Vec<Device>>
pub async fn connect_async(&self, address: &str, timeout_secs: u64) -> Result<Device>
```

## Usage / Example

```rust
use networkkit::{Bluetooth, DeviceFilter};

let bt = Bluetooth::new();
if !bt.is_available() {
    eprintln!("no bluetooth adapter");
    return;
}

for device in bt.discover(10).unwrap_or_default() {
    println!("{} {:?} {}%", device.address,
        device.name.as_deref().unwrap_or("?"),
        device.rssi_pct.unwrap_or(0));
}

if let Some(device) = bt.devices(DeviceFilter::Paired).unwrap_or_default().first() {
    bt.connect(&device.address, 15).unwrap();
}
```

## Cross References

- [MAIN.md](MAIN.md) – permission model note
- [LocalNetwork.md](LocalNetwork.md) – same error type on every domain
