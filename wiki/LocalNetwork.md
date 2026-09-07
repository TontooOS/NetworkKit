# Local Network

The `LocalNetwork` client discovers everything around the machine using plain
Linux facilities: iproute2 for interfaces and the neighbor table, UDP
broadcasts for service discovery, hand-rolled mDNS queries over multicast and
parallel TCP probes. No daemon, no extra dependency.

## interfaces

```rust
pub fn interfaces(&self) -> Result<Vec<Interface>>
```

Parses `ip -j address` (iproute2 JSON mode) and enriches each entry with the
sysfs wireless flag:

```rust
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

pub struct Address {
    pub ip: IpAddr,
    pub prefix_len: u8,
}
```

- `up` reflects the interface flags, not the cable state.
- `mac` is `None` on interfaces without a hardware address.
- Returns `Err(NetworkError::ParseError)` when iproute2 emits invalid JSON.

## neighbors

```rust
pub fn neighbors(&self) -> Result<Vec<Neighbor>>
```

Reads the kernel neighbor table via `ip neighbour show`:

```rust
pub struct Neighbor {
    pub ip: IpAddr,
    pub mac: Option<String>,
    pub device: String,
    pub state: String,
}
```

- MAC addresses are lower-cased; entries without `lladdr` keep `None`.
- `FAILED` entries are filtered out.

## broadcast_discovery

```rust
pub fn broadcast_discovery(
    &self,
    port: u16,
    payload: &[u8],
    timeout: Duration,
) -> Result<Vec<DiscoveryReply>>
```

Sends one datagram to the directed broadcast address of every up, non-loopback
IPv4 subnet and collects answers until `timeout` elapsed.

```rust
pub struct DiscoveryReply {
    pub from: IpAddr,
    pub data: Vec<u8>,
}
```

- Broadcast addresses are computed from the prefix length (`broadcast_address`).
- The reply payload can be read as UTF-8 via `reply.data_utf8()`.
- TontooServices can listen on such a port to announce themselves; this needs
  no special privileges.

## tcp_sweep

```rust
pub fn tcp_sweep(&self, port: u16, timeout_per_host: Duration) -> Result<Vec<DiscoveredHost>>
```

Probes every host of the /24 slices of all connected IPv4 subnets with
parallel `TcpStream::connect_timeout` attempts.

```rust
pub enum DiscoverySource { UdpBroadcast, TcpProbe, Mdns }

pub struct DiscoveredHost {
    pub address: IpAddr,
    pub port: Option<u16>,
    pub name: Option<String>,
    pub source: DiscoverySource,
}
```

- Sweeps are capped at 1024 hosts and run in chunks of 64 threads.
- Network and broadcast addresses of each /24 are excluded.

## mdns_query

```rust
pub fn mdns_query(&self, service: &str, timeout: Duration) -> Result<Vec<DiscoveredHost>>
```

Sends a PTR query for `<service>.local` to the mDNS group `224.0.0.251:5353`
and merges every responder record:

- PTR answers provide the instance name (first DNS label).
- A records provide the responder IPv4 address.
- Records are matched by owner/target name; malformed packets are ignored.

The query packet is built by `localnet::mdns::build_query`, answers are parsed
by `localnet::mdns::parse_response`; both functions are public and unit
tested without network access.

## is_reachable

```rust
pub fn is_reachable(&self, address: IpAddr, port: u16, timeout: Duration) -> bool
```

Single TCP probe against one host.

## Async API

Blocking calls run inside `tokio::task::spawn_blocking`:

```rust
pub async fn interfaces_async(&self) -> Result<Vec<Interface>>
pub async fn broadcast_discovery_async(
    &self,
    port: u16,
    payload: Vec<u8>,
    timeout: Duration,
) -> Result<Vec<DiscoveryReply>>
pub async fn mdns_query_async(
    &self,
    service: &str,
    timeout: Duration,
) -> Result<Vec<DiscoveredHost>>
```

## Usage / Example

```rust
use std::time::Duration;
use networkkit::LocalNetwork;

let lan = LocalNetwork::new();

for iface in lan.interfaces().unwrap_or_default() {
    println!("{} up={} wifi={}", iface.name, iface.up, iface.wireless);
}

for neighbor in lan.neighbors().unwrap_or_default() {
    println!("{} -> {:?}", neighbor.ip, neighbor.mac);
}

for host in lan.tcp_sweep(22, Duration::from_millis(300)).unwrap_or_default() {
    println!("{} has ssh open", host.address);
}
```

## Cross References

- [Wifi.md](Wifi.md) – uses the same interface enumeration
- [Bluetooth.md](Bluetooth.md) – same error type on every domain
