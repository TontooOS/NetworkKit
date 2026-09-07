# Tontoo NetworkKit

A Framework for Bluetooth, WiFi and Local Network Discovery on Linux.

## Made for TontooOS

Explore more at https://github.com/TontooOS/Libs

## Adding to Your Project

Add to your `Cargo.toml`:

```toml
[dependencies]
sdk = { path = "/Library/System/sdk", features = ["NetworkKit"] }
```

Then at the crate root:

```rust
sdk::preinclude!();
use NetworkKit::{ /* ... */ };
```

## Documentation

See the wiki: [wiki/MAIN.md](wiki/MAIN.md)

## Permissions

NetworkKit uses the regular Linux stack (`bluetoothctl`, `nmcli`, `ip`,
plain sockets) and contains no permission logic itself. Access control is
enforced by the FishPerms system service at the OS level; denied operations
surface as `NetworkError::PermissionDenied`.

## License

TCL v26.1
