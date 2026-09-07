use networkkit::{Bluetooth, DeviceFilter};

fn main() {
    let bt = Bluetooth::new();

    if !bt.is_available() {
        println!("Bluetooth adapter or bluetoothctl not available");
        return;
    }

    if let Some(blocked) = bt.radio_blocked() {
        println!("rfkill blocked: {}", blocked);
    }

    match bt.adapter() {
        Ok(adapter) => {
            println!(
                "Adapter {} ({}, {}) powered={} discovering={}",
                adapter.name, adapter.alias, adapter.address, adapter.powered, adapter.discovering
            );
        }
        Err(e) => {
            eprintln!("adapter: {}", e);
        }
    }

    println!("\nPaired devices:");
    for device in bt.devices(DeviceFilter::Paired).unwrap_or_default() {
        println!(
            "  {} {:?} paired={} connected={}",
            device.address,
            device.name.as_deref().unwrap_or("?"),
            device.paired,
            device.connected
        );
    }

    println!("\nScanning 10 seconds ...");
    let devices = bt.discover(10).unwrap_or_default();
    if devices.is_empty() {
        println!("No devices found");
        return;
    }

    println!("Found {} devices:", devices.len());
    for device in &devices {
        println!(
            "  {} {:?} rssi={}",
            device.address,
            device.name.as_deref().unwrap_or("(unknown)"),
            device
                .rssi_pct
                .map(|r| format!("{}%", r))
                .unwrap_or_else(|| "?".into())
        );
    }
}
