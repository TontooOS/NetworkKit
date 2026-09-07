use networkkit::Wifi;

fn main() {
    let wifi = Wifi::new();

    if !wifi.is_available() {
        println!("Wireless interface or nmcli not available");
        return;
    }

    if let Some(blocked) = wifi.radio_blocked() {
        println!("rfkill blocked: {}", blocked);
    }

    println!("Interface: {:?}", wifi.interface());

    match wifi.status() {
        Ok(Some(status)) => {
            println!(
                "Connected to {:?} ({}), signal {}%, ip {:?}, state {}",
                status.ssid.as_deref().unwrap_or("?"),
                status.bssid.as_deref().unwrap_or("?"),
                status.signal_pct,
                status.ipv4.as_deref().unwrap_or("-"),
                status.state
            );
        }
        Ok(None) => println!("Not connected"),
        Err(e) => eprintln!("status: {}", e),
    }

    println!("\nScanning networks ...");
    match wifi.scan(true) {
        Ok(networks) => {
            if networks.is_empty() {
                println!("No networks found");
                return;
            }
            for network in &networks {
                println!(
                    "  {:<32} {}% ch={:?} {}",
                    network.ssid,
                    network.signal_pct,
                    network.channel(),
                    network.security
                );
            }
        }
        Err(e) => eprintln!("scan: {}", e),
    }
}
