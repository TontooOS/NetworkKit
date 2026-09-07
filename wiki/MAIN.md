# NetworkKit – Wiki

NetworkKit is the network framework for TontooOS. It covers Bluetooth, WiFi and
local network discovery by using the native Linux stack (BlueZ, NetworkManager,
iproute2, raw UDP/mDNS sockets) without any cloud dependency.

- Repository: https://github.com/TontooOS/Libs
- License: TCL
- Version: 26.1.0

## Feature Index

| Feature | File | Description |
|---|---|---|
| Main index | [MAIN.md](MAIN.md) | This page |
| Rules | [RULE.md](RULE.md) | Development and usage rules |
| Bluetooth | [Bluetooth.md](Bluetooth.md) | `Bluetooth` client, adapters, discovery, pairing, connections |
| WiFi | [Wifi.md](Wifi.md) | `Wifi` client, scanning, connection status, connect/disconnect |
| Local Network | [LocalNetwork.md](LocalNetwork.md) | Interfaces, neighbor table, UDP broadcast, mDNS and TCP sweeps |
| Localization | [Localization.md](Localization.md) | Error message localization via `lang/en_us.json` and `lang/de_de.json` |

## Quick Start

```rust
use networkkit::{scan_wifi, discover_bluetooth, local_interfaces};

let networks = scan_wifi().unwrap();
for net in &networks {
    println!("{} ({}%)", net.ssid, net.signal_pct);
}

let devices = discover_bluetooth(10).unwrap();
println!("{} bluetooth devices found", devices.len());

for iface in local_interfaces().unwrap() {
    println!("{} up={}", iface.name, iface.up);
}
```

See [Wifi.md](Wifi.md), [Bluetooth.md](Bluetooth.md) and
[LocalNetwork.md](LocalNetwork.md) for details.

## Permissions

Permission enforcement is not part of NetworkKit. Hardware access goes through
the regular Linux facilities (`bluetoothctl`, `nmcli`, `ip`, plain sockets), so
the FishPerms system service can gate every path at the OS level. NetworkKit
contains no permission code; denied operations surface as
`NetworkError::PermissionDenied` mapped from the operating system.

## Changelog

- 2026-08-24: Initial wiki with Bluetooth, WiFi, Local Network and
  Localization pages.
