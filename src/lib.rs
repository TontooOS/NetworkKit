pub mod bluetooth;
pub mod daemon;
pub mod http;
pub mod lang;
pub mod localnet;
pub mod types;
pub mod util;
pub mod wifi;

pub use bluetooth::{Adapter, Bluetooth, Device, DeviceFilter};
pub use http::{HttpClient, HttpMethod, HttpRequest, HttpResponse};
pub use localnet::{
    Address, DiscoveryReply, DiscoveredHost, DiscoverySource, Interface, LocalNetwork, Neighbor,
};
pub use types::{NetworkError, Result};
pub use wifi::{Wifi, WifiNetwork, WifiStatus};

/// Shared handle to all three network domains.
#[derive(Debug, Clone, Default)]
pub struct NetworkKit {
    pub bluetooth: Bluetooth,
    pub wifi: Wifi,
    pub local_network: LocalNetwork,
}

impl NetworkKit {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Performs a blocking HTTP GET and returns the response.
pub fn http_get(url: &str) -> Result<HttpResponse> {
    http::get(url)
}

/// Performs a blocking HTTP POST with raw bytes.
pub fn http_post(url: &str, body: &[u8]) -> Result<HttpResponse> {
    http::post(url, body)
}

/// Scans nearby WiFi networks (active rescan).
pub fn scan_wifi() -> Result<Vec<WifiNetwork>> {
    Wifi::new().scan(true)
}

/// Returns the currently connected WiFi network details.
pub fn wifi_status() -> Result<Option<WifiStatus>> {
    Wifi::new().status()
}

/// Starts a Bluetooth discovery for the given duration.
pub fn discover_bluetooth(timeout_secs: u64) -> Result<Vec<Device>> {
    Bluetooth::new().discover(timeout_secs)
}

/// Lists paired Bluetooth devices.
pub fn paired_bluetooth_devices() -> Result<Vec<Device>> {
    Bluetooth::new().devices(DeviceFilter::Paired)
}

/// Enumerates local interfaces via iproute2.
pub fn local_interfaces() -> Result<Vec<Interface>> {
    LocalNetwork::new().interfaces()
}

/// Reads the kernel neighbor table.
pub fn neighbors() -> Result<Vec<Neighbor>> {
    LocalNetwork::new().neighbors()
}

mod ffi;
