use std::time::Duration;
use networkkit::LocalNetwork;

fn main() {
    let lan = LocalNetwork::new();

    println!("Interfaces:");
    for interface in lan.interfaces().unwrap_or_default() {
        let ips: Vec<String> = interface
            .ipv4
            .iter()
            .chain(interface.ipv6.iter())
            .map(|a| format!("{}/{}", a.ip, a.prefix_len))
            .collect();
        println!(
            "  {:<12} up={} wifi={} mac={:?} {}",
            interface.name,
            interface.up,
            interface.wireless,
            interface.mac.as_deref().unwrap_or("-"),
            ips.join(" ")
        );
    }

    println!("\nNeighbors:");
    for neighbor in lan.neighbors().unwrap_or_default() {
        println!(
            "  {:<40} {:?} dev={} {}",
            neighbor.ip,
            neighbor.mac.as_deref().unwrap_or("-"),
            neighbor.device,
            neighbor.state
        );
    }

    println!("\nUDP broadcast discovery on port 53123 ...");
    for reply in lan
        .broadcast_discovery(53123, b"TONTOO-DISCOVER", Duration::from_secs(2))
        .unwrap_or_default()
    {
        println!("  reply from {}: {:?}", reply.from, reply.data_utf8());
    }

    println!("\nmDNS query for _http._tcp ...");
    for host in lan
        .mdns_query("_http._tcp", Duration::from_secs(3))
        .unwrap_or_default()
    {
        println!(
            "  {} ({:?}) port={:?}",
            host.address,
            host.name.as_deref().unwrap_or("?"),
            host.port
        );
    }

    println!("\nTCP sweep on port 22 ...");
    for host in lan
        .tcp_sweep(22, Duration::from_millis(300))
        .unwrap_or_default()
    {
        println!("  {} open", host.address);
    }

    let _ = lan;
}
