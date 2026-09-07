pub mod mdns;

use crate::types::{NetworkError, Result};
use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

/// Maximum number of hosts probed during one TCP sweep.
const MAX_SWEEP_HOSTS: usize = 1024;
/// How many parallel TCP probe threads are used per sweep chunk.
const SWEEP_CHUNK: usize = 64;

#[derive(Debug, Clone)]
pub struct Address {
    pub ip: IpAddr,
    pub prefix_len: u8,
}

#[derive(Debug, Clone, Default)]
pub struct Interface {
    pub name: String,
    pub index: u32,
    pub mac: Option<String>,
    pub state: String,
    pub up: bool,
    pub wireless: bool,
    pub ipv4: Vec<Address>,
    pub ipv6: Vec<Address>,
}

#[derive(Debug, Clone)]
pub struct Neighbor {
    pub ip: IpAddr,
    pub mac: Option<String>,
    pub device: String,
    pub state: String,
}

#[derive(Debug, Clone)]
pub struct DiscoveryReply {
    pub from: IpAddr,
    pub data: Vec<u8>,
}

impl DiscoveryReply {
    pub fn data_utf8(&self) -> Option<&str> {
        std::str::from_utf8(&self.data).ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverySource {
    UdpBroadcast,
    TcpProbe,
    Mdns,
}

#[derive(Debug, Clone)]
pub struct DiscoveredHost {
    pub address: IpAddr,
    pub port: Option<u16>,
    pub name: Option<String>,
    pub source: DiscoverySource,
}

#[derive(Deserialize)]
struct IpAddressJson {
    #[serde(default)]
    ifindex: u32,
    #[serde(default)]
    ifname: String,
    #[serde(default)]
    operstate: String,
    #[serde(default)]
    flags: Vec<String>,
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    addr_info: Vec<AddrInfoJson>,
}

#[derive(Deserialize)]
struct AddrInfoJson {
    #[serde(default)]
    local: String,
    #[serde(default)]
    prefixlen: u8,
}

/// Computes the directed broadcast address of an IPv4 subnet.
///
/// Returns `None` when the prefix length exceeds 32.
pub fn broadcast_address(address: Ipv4Addr, prefix_len: u8) -> Option<Ipv4Addr> {
    if prefix_len > 32 {
        return None;
    }
    let bits = u32::from(address);
    let mask = if prefix_len == 0 {
        0
    } else {
        u32::MAX << (32 - prefix_len as u32)
    };
    Some(Ipv4Addr::from((bits & mask) | !mask))
}

/// Lists every host address of the /24 slice containing `address`.
///
/// Prefixes are ignored, only the /24 slice of the address is swept so runs
/// stay bounded; network and broadcast addresses are excluded.
pub fn subnet_hosts(address: Ipv4Addr) -> Vec<Ipv4Addr> {
    let base = u32::from(address) & 0xFFFFFF00;
    (1..255).map(|i| Ipv4Addr::from(base + i)).collect()
}

fn is_wireless(name: &str) -> bool {
    std::fs::metadata(format!("/sys/class/net/{}/wireless", name)).is_ok()
        || std::fs::metadata(format!("/sys/class/net/{}/phy80211", name)).is_ok()
}

/// Parses `ip -j address` output into [`Interface`] values.
///
/// Pure parsing without any filesystem access; [`LocalNetwork::interfaces`]
/// enriches the result with sysfs state such as `wireless`.
pub fn parse_ip_json(text: &str) -> Result<Vec<Interface>> {
    let parsed: Vec<IpAddressJson> =
        serde_json::from_str(text).map_err(|e| NetworkError::ParseError(e.to_string()))?;

    Ok(parsed
        .into_iter()
        .map(|entry| {
            let mut interface = Interface {
                name: entry.ifname.clone(),
                index: entry.ifindex,
                mac: entry.address.filter(|m| m.contains(':')),
                state: entry.operstate.clone(),
                up: entry.flags.iter().any(|f| f == "UP"),
                ..Interface::default()
            };

            for info in entry.addr_info {
                let Some(ip) = info.local.parse::<IpAddr>().ok() else {
                    continue;
                };
                let address = Address {
                    ip,
                    prefix_len: info.prefixlen,
                };
                match address.ip {
                    IpAddr::V4(_) => interface.ipv4.push(address),
                    IpAddr::V6(_) => interface.ipv6.push(address),
                }
            }

            interface
        })
        .collect())
}

/// Parses one line of `ip neighbour show` output.
pub fn parse_neighbor_line(line: &str) -> Option<Neighbor> {
    const STATES: &[&str] = &[
        "INCOMPLETE", "REACHABLE", "STALE", "DELAY", "PROBE", "FAILED",
        "PERMANENT", "NOARP", "NONE",
    ];

    let mut tokens = line.split_whitespace();
    let ip = tokens.next()?.parse::<IpAddr>().ok()?;
    let mut device = String::new();
    let mut mac: Option<String> = None;
    let mut state = String::new();

    while let Some(token) = tokens.next() {
        match token {
            "dev" => device = tokens.next().unwrap_or_default().to_string(),
            "lladdr" => mac = tokens.next().map(str::to_lowercase),
            other => {
                if STATES.contains(&other.to_ascii_uppercase().as_str()) {
                    state = other.to_ascii_uppercase();
                }
            }
        }
    }

    if device.is_empty() {
        return None;
    }

    Some(Neighbor {
        ip,
        mac,
        device,
        state,
    })
}

#[derive(Debug, Clone)]
pub struct LocalNetwork;

impl LocalNetwork {
    pub fn new() -> Self {
        Self
    }

    /// Enumerates all network interfaces via `ip -j address`.
    pub fn interfaces(&self) -> Result<Vec<Interface>> {
        let mut interfaces = crate::util::run("ip", &["-j", "address"])
            .and_then(|text| parse_ip_json(&text))?;
        for interface in &mut interfaces {
            interface.wireless = is_wireless(&interface.name);
        }
        Ok(interfaces)
    }

    /// Reads the kernel neighbor table (ARP cache) via `ip neighbour show`.
    pub fn neighbors(&self) -> Result<Vec<Neighbor>> {
        let text = crate::util::run("ip", &["neighbour", "show"])?;
        Ok(text
            .lines()
            .filter_map(parse_neighbor_line)
            .filter(|n| n.state != "FAILED")
            .collect())
    }

    /// Sends a UDP datagram to the broadcast address of every up interface and
    /// collects answers until `timeout` elapsed.
    ///
    /// TontooServices can listen on such a port to announce themselves, so this
    /// needs no special privileges.
    pub fn broadcast_discovery(
        &self,
        port: u16,
        payload: &[u8],
        timeout: Duration,
    ) -> Result<Vec<DiscoveryReply>> {
        let socket = UdpSocket::bind(("0.0.0.0", 0)).map_err(NetworkError::from_io)?;
        socket.set_broadcast(true).map_err(NetworkError::from_io)?;
        socket.set_read_timeout(Some(Duration::from_millis(200)))
            .map_err(NetworkError::from_io)?;

        let mut sent_any = false;
        for interface in self.interfaces()? {
            for address in &interface.ipv4 {
                if !interface.up || address.ip.is_loopback() {
                    continue;
                }
                let IpAddr::V4(v4) = address.ip else {
                    continue;
                };
                let Some(broadcast) = broadcast_address(v4, address.prefix_len) else {
                    continue;
                };
                let target = SocketAddr::from((broadcast, port));
                if socket.send_to(payload, target).is_ok() {
                    sent_any = true;
                }
            }
        }

        if !sent_any {
            return Ok(Vec::new());
        }

        let mut replies = Vec::new();
        let deadline = Instant::now() + timeout;
        let mut buffer = [0u8; 2048];

        while Instant::now() < deadline {
            match socket.recv_from(&mut buffer) {
                Ok((len, source)) => {
                    replies.push(DiscoveryReply {
                        from: source.ip(),
                        data: buffer[..len].to_vec(),
                    });
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(e) => return Err(NetworkError::from_io(e)),
            }
        }

        Ok(replies)
    }

    /// Probes every host of each connected /24 subnet on `port` using parallel
    /// TCP connect attempts.
    pub fn tcp_sweep(&self, port: u16, timeout_per_host: Duration) -> Result<Vec<DiscoveredHost>> {
        let mut targets: Vec<IpAddr> = Vec::new();

        for interface in self.interfaces()? {
            if !interface.up {
                continue;
            }
            for address in &interface.ipv4 {
                let IpAddr::V4(v4) = address.ip else {
                    continue;
                };
                if v4.is_loopback() {
                    continue;
                }
                for host in subnet_hosts(v4) {
                    targets.push(IpAddr::V4(host));
                }
            }
        }

        targets.sort_by_key(|a| match a {
            IpAddr::V4(v4) => u32::from(*v4),
            IpAddr::V6(_) => u32::MAX,
        });
        targets.dedup();

        if targets.len() > MAX_SWEEP_HOSTS {
            targets.truncate(MAX_SWEEP_HOSTS);
        }

        Ok(sweep_chunked(&targets, port, timeout_per_host))
    }

    /// Sends an mDNS PTR query for a service type (e.g. `_http._tcp`) and
    /// merges PTR/A records from all responders.
    pub fn mdns_query(&self, service: &str, timeout: Duration) -> Result<Vec<DiscoveredHost>> {
        use std::net::ToSocketAddrs;

        let query = mdns::build_query(service, rand_id()).ok_or_else(|| {
            NetworkError::ParseError(format!("invalid service name {}", service))
        })?;

        let target: SocketAddr = (mdns::MDNS_GROUP, mdns::MDNS_PORT)
            .to_socket_addrs()
            .map_err(NetworkError::from_io)?
            .next()
            .ok_or(NetworkError::NotAvailable)?;

        let socket = UdpSocket::bind(("0.0.0.0", 0)).map_err(NetworkError::from_io)?;
        socket.set_read_timeout(Some(Duration::from_millis(200)))
            .map_err(NetworkError::from_io)?;
        socket.send_to(&query, target).map_err(NetworkError::from_io)?;

        let mut records = Vec::new();
        let deadline = Instant::now() + timeout;
        let mut buffer = [0u8; 4096];

        while Instant::now() < deadline {
            match socket.recv_from(&mut buffer) {
                Ok((len, _)) => records.extend(mdns::parse_response(&buffer[..len])),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    continue;
                }
                Err(e) => return Err(NetworkError::from_io(e)),
            }
        }

        Ok(merge_mdns_records(records))
    }

    /// Checks whether a single host accepts a TCP connection on `port`.
    pub fn is_reachable(&self, address: IpAddr, port: u16, timeout: Duration) -> bool {
        let target = SocketAddr::new(address, port);
        std::net::TcpStream::connect_timeout(&target, timeout).is_ok()
    }

    // ---- async wrappers ----

    pub async fn interfaces_async(&self) -> Result<Vec<Interface>> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.interfaces())
            .await
            .map_err(|e| NetworkError::IoError(e.to_string()))?
    }

    pub async fn broadcast_discovery_async(
        &self,
        port: u16,
        payload: Vec<u8>,
        timeout: Duration,
    ) -> Result<Vec<DiscoveryReply>> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.broadcast_discovery(port, &payload, timeout))
            .await
            .map_err(|e| NetworkError::IoError(e.to_string()))?
    }

    pub async fn mdns_query_async(
        &self,
        service: &str,
        timeout: Duration,
    ) -> Result<Vec<DiscoveredHost>> {
        let this = self.clone();
        let service = service.to_string();
        tokio::task::spawn_blocking(move || this.mdns_query(&service, timeout))
            .await
            .map_err(|e| NetworkError::IoError(e.to_string()))?
    }
}

impl Default for LocalNetwork {
    fn default() -> Self {
        Self::new()
    }
}

fn rand_id() -> u16 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u16)
        .unwrap_or(0);
    nanos ^ (std::process::id() as u16)
}

fn merge_mdns_records(records: Vec<mdns::MdnsRecord>) -> Vec<DiscoveredHost> {
    let mut merged: std::collections::HashMap<String, DiscoveredHost> =
        std::collections::HashMap::new();

    for record in records {
        let key = record.ptr_target.clone().unwrap_or_else(|| record.owner.clone());
        let entry = merged.entry(key).or_insert_with(|| DiscoveredHost {
            address: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: None,
            name: None,
            source: DiscoverySource::Mdns,
        });

        if let Some(target) = record.ptr_target {
            entry.name.get_or_insert_with(|| {
                target
                    .split('.')
                    .next()
                    .unwrap_or_default()
                    .to_string()
            });
        } else if let Some(ip) = record.ipv4 {
            entry.address = IpAddr::V4(ip);
        }
    }

    let mut hosts: Vec<DiscoveredHost> = merged
        .into_values()
        .filter(|host| host.address != IpAddr::V4(Ipv4Addr::UNSPECIFIED) || host.name.is_some())
        .collect();
    hosts.sort_by(|a, b| a.name.as_deref().cmp(&b.name.as_deref()));
    hosts
}

fn sweep_chunked(targets: &[IpAddr], port: u16, timeout_per_host: Duration) -> Vec<DiscoveredHost> {
    let mut found = Vec::new();

    for chunk in targets.chunks(SWEEP_CHUNK) {
        std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|target| {
                    scope.spawn(move || {
                        std::net::TcpStream::connect_timeout(
                            &SocketAddr::new(*target, port),
                            timeout_per_host,
                        )
                        .is_ok()
                        .then(|| DiscoveredHost {
                            address: *target,
                            port: Some(port),
                            name: None,
                            source: DiscoverySource::TcpProbe,
                        })
                    })
                })
                .collect();

            for handle in handles {
                if let Ok(Some(host)) = handle.join() {
                    found.push(host);
                }
            }
        });
    }

    found.sort_by_key(|h| h.address);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_broadcast_addresses() {
        assert_eq!(
            broadcast_address(Ipv4Addr::new(192, 168, 1, 10), 24),
            Some(Ipv4Addr::new(192, 168, 1, 255))
        );
        assert_eq!(
            broadcast_address(Ipv4Addr::new(10, 0, 128, 200), 17),
            Some(Ipv4Addr::new(10, 0, 255, 255))
        );
        assert_eq!(
            broadcast_address(Ipv4Addr::new(192, 168, 1, 1), 0),
            Some(Ipv4Addr::new(255, 255, 255, 255))
        );
        assert_eq!(broadcast_address(Ipv4Addr::new(1, 1, 1, 1), 33), None);
    }

    #[test]
    fn enumerates_subnet_hosts() {
        let hosts = subnet_hosts(Ipv4Addr::new(192, 168, 1, 7));
        assert_eq!(hosts.len(), 254);
        assert_eq!(hosts[0], Ipv4Addr::new(192, 168, 1, 1));
        assert_eq!(hosts[253], Ipv4Addr::new(192, 168, 1, 254));

        let hosts = subnet_hosts(Ipv4Addr::new(10, 20, 30, 40));
        assert_eq!(hosts[0], Ipv4Addr::new(10, 20, 30, 1));
    }

    #[test]
    fn parses_ip_json_output() {
        let sample = r#"[
          {
            "ifindex": 1,
            "ifname": "lo",
            "flags": ["LOOPBACK","UP","LOWER_UP"],
            "mtu": 65536,
            "operstate": "UNKNOWN",
            "addr_info": [
              {"family":"inet","local":"127.0.0.1","prefixlen":8,"scope":"host"}
            ]
          },
          {
            "ifindex": 2,
            "ifname": "wlan0",
            "flags": ["BROADCAST","MULTICAST","UP","LOWER_UP"],
            "operstate": "UP",
            "address": "aa:f3:c1:3b:6f:90",
            "addr_info": [
              {"family":"inet","local":"192.168.1.23","prefixlen":24,"scope":"global"},
              {"family":"inet6","local":"fe80::1234","prefixlen":64,"scope":"link"}
            ]
          },
          {
            "ifindex": 3,
            "ifname": "eth0",
            "flags": ["BROADCAST","MULTICAST"],
            "operstate": "DOWN"
          }
        ]"#;

        let interfaces = parse_ip_json(sample).unwrap();
        assert_eq!(interfaces.len(), 3);

        let lo = &interfaces[0];
        assert_eq!(lo.name, "lo");
        assert!(lo.up);
        assert!(!lo.wireless);
        assert!(lo.mac.is_none());
        assert_eq!(lo.ipv4.len(), 1);

        let wlan = &interfaces[1];
        assert_eq!(wlan.mac.as_deref(), Some("aa:f3:c1:3b:6f:90"));
        assert!(wlan.up);
        assert!(!wlan.wireless);
        assert_eq!(wlan.ipv4[0].prefix_len, 24);
        assert_eq!(wlan.ipv6[0].ip.to_string(), "fe80::1234");

        let eth = &interfaces[2];
        assert!(!eth.up);
        assert!(eth.ipv4.is_empty());
        assert!(!eth.wireless);
    }

    #[test]
    fn rejects_invalid_ip_json() {
        assert!(parse_ip_json("not json").is_err());
        assert!(parse_ip_json("[]").is_ok());
    }

    #[test]
    fn parses_neighbor_lines() {
        let reachable =
            parse_neighbor_line("192.168.1.1 dev wlan0 lladdr AA:BB:CC:DD:EE:FF REACHABLE")
                .unwrap();
        assert_eq!(reachable.ip.to_string(), "192.168.1.1");
        assert_eq!(reachable.device, "wlan0");
        assert_eq!(reachable.mac.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        assert_eq!(reachable.state, "REACHABLE");

        let stale = parse_neighbor_line("fe80::1 dev eth0 lladdr 11:22:33:44:55:66 router STALE")
            .unwrap();
        assert_eq!(stale.ip.to_string(), "fe80::1");
        assert_eq!(stale.state, "STALE");

        let failed_no_mac = parse_neighbor_line("192.168.1.99 dev eth0 INCOMPLETE").unwrap();
        assert_eq!(failed_no_mac.mac, None);
        assert_eq!(failed_no_mac.state, "INCOMPLETE");

        assert!(parse_neighbor_line("").is_none());
        assert!(parse_neighbor_line("not-an-ip dev eth0 STALE").is_none());
    }
}
