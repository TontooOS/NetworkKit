//! C FFI exports for NetworkKit.
//!
//! All functions are blocking - call them from a worker thread.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use foundation::serialization::JsonValue;

fn set_error(error_out: *mut *mut c_char, message: &str) {
    if error_out.is_null() {
        return;
    }
    if let Ok(c) = CString::new(message.to_owned()) {
        unsafe { *error_out = c.into_raw() };
    }
}

fn json_ptr(json: &str) -> *mut c_char {
    CString::new(json).unwrap_or_default().into_raw()
}

fn opt_str(value: &Option<String>) -> JsonValue {
    match value {
        Some(text) => JsonValue::Str(text.clone()),
        None => JsonValue::Null,
    }
}

fn json_array(items: Vec<String>) -> String {
    format!("[{}]", items.join(","))
}

/// The framework version as a static C string.
#[no_mangle]
pub extern "C" fn tontoo_networkkit_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

/// Scan for WiFi networks. Returns a JSON array or null.
///
/// # Safety
///
/// `error_out`, when not null, must point to a writable `char*`.
#[no_mangle]
pub unsafe extern "C" fn tontoo_networkkit_scan_wifi(
    error_out: *mut *mut c_char,
) -> *mut c_char {
    match crate::scan_wifi() {
        Ok(networks) => {
            let items: Vec<String> = networks
                .iter()
                .map(|n| {
                    JsonValue::Object(vec![
                        ("ssid".to_string(), JsonValue::Str(n.ssid.clone())),
                        ("bssid".to_string(), opt_str(&n.bssid)),
                        ("signal_pct".to_string(), JsonValue::Integer(n.signal_pct as i64)),
                        (
                            "frequency_mhz".to_string(),
                            n.frequency_mhz
                                .map(|f| JsonValue::Integer(f as i64))
                                .unwrap_or(JsonValue::Null),
                        ),
                        ("security".to_string(), JsonValue::Str(n.security.clone())),
                        ("known".to_string(), JsonValue::Bool(n.known)),
                    ])
                    .stringify(false)
                })
                .collect();
            json_ptr(&json_array(items))
        }
        Err(_) => {
            set_error(error_out, "wifi scan failed");
            std::ptr::null_mut()
        }
    }
}

/// Current WiFi connection status. Returns a JSON object or null when not
/// connected to any network.
///
/// # Safety
///
/// `error_out`, when not null, must point to a writable `char*`.
#[no_mangle]
pub unsafe extern "C" fn tontoo_networkkit_wifi_status(
    error_out: *mut *mut c_char,
) -> *mut c_char {
    match crate::wifi_status() {
        Ok(Some(status)) => {
            let value = JsonValue::Object(vec![
                ("interface".to_string(), JsonValue::Str(status.interface.clone())),
                ("ssid".to_string(), opt_str(&status.ssid)),
                ("bssid".to_string(), opt_str(&status.bssid)),
                ("signal_pct".to_string(), JsonValue::Integer(status.signal_pct as i64)),
                (
                    "frequency_mhz".to_string(),
                    status
                        .frequency_mhz
                        .map(|f| JsonValue::Integer(f as i64))
                        .unwrap_or(JsonValue::Null),
                ),
                ("security".to_string(), JsonValue::Str(status.security.clone())),
                ("state".to_string(), JsonValue::Str(status.state.clone())),
                ("ipv4".to_string(), opt_str(&status.ipv4)),
            ]);
            json_ptr(&value.stringify(false))
        }
        Ok(None) => std::ptr::null_mut(),
        Err(_) => {
            set_error(error_out, "wifi status failed");
            std::ptr::null_mut()
        }
    }
}

/// List local network interfaces. Returns a JSON array or null.
///
/// # Safety
///
/// `error_out`, when not null, must point to a writable `char*`.
#[no_mangle]
pub unsafe extern "C" fn tontoo_networkkit_local_interfaces(
    error_out: *mut *mut c_char,
) -> *mut c_char {
    match crate::local_interfaces() {
        Ok(interfaces) => {
            let items: Vec<String> = interfaces
                .iter()
                .map(|i| {
                    let addresses = |list: &[crate::localnet::Address]| {
                        JsonValue::Array(
                            list.iter()
                                .map(|a| {
                                    JsonValue::Object(vec![
                                        ("ip".to_string(), JsonValue::Str(a.ip.to_string())),
                                        (
                                            "prefix_len".to_string(),
                                            JsonValue::Integer(a.prefix_len as i64),
                                        ),
                                    ])
                                })
                                .collect(),
                        )
                    };
                    JsonValue::Object(vec![
                        ("name".to_string(), JsonValue::Str(i.name.clone())),
                        ("index".to_string(), JsonValue::Integer(i.index as i64)),
                        ("mac".to_string(), opt_str(&i.mac)),
                        ("state".to_string(), JsonValue::Str(i.state.clone())),
                        ("up".to_string(), JsonValue::Bool(i.up)),
                        ("wireless".to_string(), JsonValue::Bool(i.wireless)),
                        ("ipv4".to_string(), addresses(&i.ipv4)),
                        ("ipv6".to_string(), addresses(&i.ipv6)),
                    ])
                    .stringify(false)
                })
                .collect();
            json_ptr(&json_array(items))
        }
        Err(_) => {
            set_error(error_out, "interface listing failed");
            std::ptr::null_mut()
        }
    }
}

/// List IP neighbors. Returns a JSON array or null.
///
/// # Safety
///
/// `error_out`, when not null, must point to a writable `char*`.
#[no_mangle]
pub unsafe extern "C" fn tontoo_networkkit_neighbors(
    error_out: *mut *mut c_char,
) -> *mut c_char {
    match crate::neighbors() {
        Ok(neighbors) => {
            let items: Vec<String> = neighbors
                .iter()
                .map(|n| {
                    JsonValue::Object(vec![
                        ("ip".to_string(), JsonValue::Str(n.ip.to_string())),
                        ("mac".to_string(), opt_str(&n.mac)),
                        ("device".to_string(), JsonValue::Str(n.device.clone())),
                        ("state".to_string(), JsonValue::Str(n.state.clone())),
                    ])
                    .stringify(false)
                })
                .collect();
            json_ptr(&json_array(items))
        }
        Err(_) => {
            set_error(error_out, "neighbor discovery failed");
            std::ptr::null_mut()
        }
    }
}

/// Discover Bluetooth devices for `timeout_secs` seconds. Returns a JSON
/// array or null.
///
/// # Safety
///
/// `error_out`, when not null, must point to a writable `char*`.
#[no_mangle]
pub unsafe extern "C" fn tontoo_networkkit_discover_bluetooth(
    timeout_secs: u64,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    match crate::discover_bluetooth(timeout_secs) {
        Ok(devices) => {
            let items: Vec<String> =
                devices.iter().map(|d| device_json(d).stringify(false)).collect();
            json_ptr(&json_array(items))
        }
        Err(_) => {
            set_error(error_out, "bluetooth discovery failed");
            std::ptr::null_mut()
        }
    }
}

/// List paired Bluetooth devices. Returns a JSON array or null.
///
/// # Safety
///
/// `error_out`, when not null, must point to a writable `char*`.
#[no_mangle]
pub unsafe extern "C" fn tontoo_networkkit_paired_bluetooth_devices(
    error_out: *mut *mut c_char,
) -> *mut c_char {
    match crate::paired_bluetooth_devices() {
        Ok(devices) => {
            let items: Vec<String> =
                devices.iter().map(|d| device_json(d).stringify(false)).collect();
            json_ptr(&json_array(items))
        }
        Err(_) => {
            set_error(error_out, "bluetooth listing failed");
            std::ptr::null_mut()
        }
    }
}

fn device_json(device: &crate::bluetooth::Device) -> JsonValue {
    JsonValue::Object(vec![
        ("address".to_string(), JsonValue::Str(device.address.clone())),
        ("name".to_string(), opt_str(&device.name)),
        ("paired".to_string(), JsonValue::Bool(device.paired)),
        ("trusted".to_string(), JsonValue::Bool(device.trusted)),
    ])
}

fn http_response_json(resp: &crate::http::HttpResponse) -> JsonValue {
    JsonValue::Object(vec![
        ("status".to_string(), JsonValue::Integer(resp.status as i64)),
        (
            "headers".to_string(),
            JsonValue::Array(
                resp.headers
                    .iter()
                    .map(|(k, v)| {
                        JsonValue::Object(vec![
                            ("name".to_string(), JsonValue::Str(k.clone())),
                            ("value".to_string(), JsonValue::Str(v.clone())),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "body".to_string(),
            JsonValue::Str(String::from_utf8_lossy(&resp.body).to_string()),
        ),
        ("url".to_string(), JsonValue::Str(resp.url.clone())),
    ])
}

fn c_str(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr).to_str().ok().map(|s| s.to_owned()) }
}

/// Blocking HTTP GET. Returns a JSON object `{status, headers, body, url}`.
///
/// # Safety
///
/// `url` must be a valid NUL-terminated string. `error_out`, when not null,
/// must point to a writable `char*`.
#[no_mangle]
pub unsafe extern "C" fn tontoo_networkkit_http_get(
    url: *const c_char,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    let Some(url) = c_str(url) else {
        set_error(error_out, "invalid url pointer");
        return std::ptr::null_mut();
    };
    match crate::http_get(&url) {
        Ok(resp) => json_ptr(&http_response_json(&resp).stringify(false)),
        Err(e) => {
            set_error(error_out, &e.to_string());
            std::ptr::null_mut()
        }
    }
}

/// Blocking HTTP POST with a raw body.
///
/// # Safety
///
/// `url` must be a valid NUL-terminated string. `body`/`body_len` describe
/// the request body (may be null/0 for empty). `error_out` rules match
/// `tontoo_networkkit_http_get`.
#[no_mangle]
pub unsafe extern "C" fn tontoo_networkkit_http_post(
    url: *const c_char,
    body: *const u8,
    body_len: usize,
    error_out: *mut *mut c_char,
) -> *mut c_char {
    let Some(url) = c_str(url) else {
        set_error(error_out, "invalid url pointer");
        return std::ptr::null_mut();
    };
    let bytes: &[u8] = if body.is_null() || body_len == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(body, body_len) }
    };
    match crate::http_post(&url, bytes) {
        Ok(resp) => json_ptr(&http_response_json(&resp).stringify(false)),
        Err(e) => {
            set_error(error_out, &e.to_string());
            std::ptr::null_mut()
        }
    }
}

/// Free a string returned by this library.
///
/// # Safety
///
/// `s` must be a pointer returned by this API or null.
#[no_mangle]
pub unsafe extern "C" fn tontoo_networkkit_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}
