//! C FFI exports for NetworkKit.
//!
//! All functions are blocking - call them from a worker thread.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

use serde_json::{json, Value};

fn set_error(error_out: *mut *mut c_char, message: &str) {
    if error_out.is_null() {
        return;
    }
    if let Ok(c) = CString::new(message.to_owned()) {
        unsafe { *error_out = c.into_raw() };
    }
}

fn json_ptr(value: &Value) -> *mut c_char {
    CString::new(value.to_string())
        .unwrap_or_default()
        .into_raw()
}

fn opt_str(value: &Option<String>) -> Value {
    value.clone().map(Value::String).unwrap_or(Value::Null)
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
        Ok(networks) => json_ptr(&Value::Array(
            networks
                .iter()
                .map(|n| {
                    json!({
                        "ssid": n.ssid,
                        "bssid": opt_str(&n.bssid),
                        "signal_pct": n.signal_pct,
                        "frequency_mhz": n.frequency_mhz,
                        "security": n.security,
                        "known": n.known,
                    })
                })
                .collect(),
        )),
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
        Ok(Some(status)) => json_ptr(&json!({
            "interface": status.interface,
            "ssid": opt_str(&status.ssid),
            "bssid": opt_str(&status.bssid),
            "signal_pct": status.signal_pct,
            "frequency_mhz": status.frequency_mhz,
            "security": status.security,
            "state": status.state,
            "ipv4": opt_str(&status.ipv4),
        })),
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
        Ok(interfaces) => json_ptr(&Value::Array(
            interfaces
                .iter()
                .map(|i| {
                    let addresses = |list: &[crate::localnet::Address]| {
                        Value::Array(
                            list.iter()
                                .map(|a| json!({ "ip": a.ip.to_string(), "prefix_len": a.prefix_len }))
                                .collect(),
                        )
                    };
                    json!({
                        "name": i.name,
                        "index": i.index,
                        "mac": opt_str(&i.mac),
                        "state": i.state,
                        "up": i.up,
                        "wireless": i.wireless,
                        "ipv4": addresses(&i.ipv4),
                        "ipv6": addresses(&i.ipv6),
                    })
                })
                .collect(),
        )),
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
        Ok(neighbors) => json_ptr(&Value::Array(
            neighbors
                .iter()
                .map(|n| {
                    json!({
                        "ip": n.ip.to_string(),
                        "mac": opt_str(&n.mac),
                        "device": n.device,
                        "state": n.state,
                    })
                })
                .collect(),
        )),
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
        Ok(devices) => json_ptr(&Value::Array(
            devices
                .iter()
                .map(|d| device_json(d))
                .collect(),
        )),
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
        Ok(devices) => json_ptr(&Value::Array(
            devices
                .iter()
                .map(|d| device_json(d))
                .collect(),
        )),
        Err(_) => {
            set_error(error_out, "bluetooth listing failed");
            std::ptr::null_mut()
        }
    }
}

fn device_json(device: &crate::bluetooth::Device) -> Value {
    json!({
        "address": device.address,
        "name": opt_str(&device.name),
        "paired": device.paired,
        "trusted": device.trusted,
    })
}

fn http_response_json(resp: &crate::http::HttpResponse) -> Value {
    json!({
        "status": resp.status,
        "headers": resp.headers.iter().map(|(k, v)| json!({"name": k, "value": v})).collect::<Vec<_>>(),
        "body": String::from_utf8_lossy(&resp.body).to_string(),
        "url": resp.url,
    })
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
        Ok(resp) => json_ptr(&http_response_json(&resp)),
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
        Ok(resp) => json_ptr(&http_response_json(&resp)),
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
