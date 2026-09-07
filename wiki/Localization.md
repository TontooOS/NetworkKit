# Localization

NetworkKit ships built-in language files for all error messages, following the
same scheme as CoreLocation. The files live in `lang/en_us.json` and
`lang/de_de.json` and are compiled into the binary with `include_str!`.

## Message Keys

| Key | Error variant | en_us text |
|---|---|---|
| `not_available` | `NetworkError::NotAvailable` | Network hardware or tool not available |
| `permission_denied` | `NetworkError::PermissionDenied` | Permission denied |
| `timeout` | `NetworkError::Timeout` | Operation timed out |
| `command_failed` | `NetworkError::CommandFailed` | Command failed: {} |
| `parse_error` | `NetworkError::ParseError` | Parse error: {} |
| `io_error` | `NetworkError::IoError` | I/O error: {} |

## Locale Detection

```rust
pub fn current_locale() -> &'static str
```

Checks the environment variables `LC_ALL`, `LC_MESSAGES` and `LANG` in this
order. A value starting with `de` selects `de_de`, everything else falls back
to `en_us`. Detection happens once; the parsed message table is cached in a
static.

## Accessing Messages

```rust
pub fn t(key: &str) -> String
pub fn t_fmt(key: &str, arg: &str) -> String
```

- `t` returns the translated message or the key itself when unknown.
- `t_fmt` replaces the first `{}` placeholder with the argument.
- `Display` for every `NetworkError` variant uses these functions, so
  `format!("{}", err)` is always localized.

## Usage / Example

```rust
use networkkit::{lang, Wifi};

let wifi = Wifi::new();
if let Err(err) = wifi.scan(false) {
    println!("{}", err);
    // "Netzwerkhardware oder Werkzeug nicht verfügbar" on de_DE systems
}

assert_eq!(lang::t("permission_denied"), "Permission denied");
```

## Cross References

- [MAIN.md](MAIN.md) – feature index
