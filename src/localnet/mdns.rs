use std::net::Ipv4Addr;

/// Multicast group and port used by mDNS (RFC 6762).
pub const MDNS_GROUP: &str = "224.0.0.251";
pub const MDNS_PORT: u16 = 5353;

const TYPE_PTR: u16 = 12;
const TYPE_A: u16 = 1;

/// Builds a DNS query packet asking for PTR records of `name`.
///
/// The name is given without the trailing `.local` root, e.g. `_http._tcp`;
/// the `.local` label is appended automatically.
pub fn build_query(name: &str, id: u16) -> Option<Vec<u8>> {
    let full = format!("{}.local", name.trim_end_matches('.'));
    let mut packet = Vec::with_capacity(64);

    packet.extend_from_slice(&id.to_be_bytes());
    packet.extend_from_slice(&[0x00, 0x00]); // standard query, no recursion
    packet.extend_from_slice(&[0x00, 0x01]); // one question
    packet.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00]); // no rr/ns/ar

    for label in full.split('.') {
        if label.is_empty() || label.len() > 63 {
            return None;
        }
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0);

    packet.extend_from_slice(&TYPE_PTR.to_be_bytes());
    packet.extend_from_slice(&[0x00, 0x01]); // IN

    Some(packet)
}

/// One parsed resource record of an mDNS response.
#[derive(Debug, Clone, PartialEq)]
pub struct MdnsRecord {
    pub owner: String,
    pub record_type: u16,
    /// Target name for PTR records.
    pub ptr_target: Option<String>,
    /// Address for A records.
    pub ipv4: Option<Ipv4Addr>,
}

fn read_name(buf: &[u8], start: usize) -> Option<(String, usize)> {
    let mut labels = Vec::new();
    let mut offset = start;
    let mut jumped = false;
    let mut next_after_jump = 0usize;
    let mut hops = 0usize;

    loop {
        if offset >= buf.len() || hops > 32 {
            return None;
        }

        match buf[offset] {
            0 => {
                offset += 1;
                break;
            }
            len if len & 0xC0 == 0xC0 => {
                if offset + 1 >= buf.len() {
                    return None;
                }
                let pointer = (((len & 0x3F) as usize) << 8) | buf[offset + 1] as usize;
                if !jumped {
                    next_after_jump = offset + 2;
                    jumped = true;
                }
                offset = pointer;
                hops += 1;
            }
            len => {
                let end = offset + 1 + len as usize;
                if end > buf.len() {
                    return None;
                }
                labels.push(String::from_utf8_lossy(&buf[offset + 1..end]).into_owned());
                offset = end;
            }
        }
    }

    let next = if jumped { next_after_jump } else { offset };
    Some((labels.join("."), next))
}

fn read_u16(buf: &[u8], offset: usize) -> Option<u16> {
    buf.get(offset..offset + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
}

/// Parses all resource records from an mDNS response packet.
///
/// Questions are skipped; answers, additional and authority sections are
/// collected. Returns an empty vector for malformed packets instead of
/// failing hard.
pub fn parse_response(buf: &[u8]) -> Vec<MdnsRecord> {
    let mut records = Vec::new();
    if buf.len() < 12 {
        return records;
    }

    let qd = read_u16(buf, 4).unwrap_or(0);
    let an = read_u16(buf, 6).unwrap_or(0);
    let ns = read_u16(buf, 8).unwrap_or(0);
    let ar = read_u16(buf, 10).unwrap_or(0);

    let mut offset = 12usize;
    for _ in 0..qd {
        match read_name(buf, offset) {
            Some((_, next)) => offset = next + 4,
            None => return records,
        }
    }

    for _ in 0..(an + ns + ar) {
        let owner = match read_name(buf, offset) {
            Some((owner, next)) => {
                offset = next;
                owner
            }
            None => return records,
        };

        let (Some(record_type), Some(_class)) =
            (read_u16(buf, offset), read_u16(buf, offset + 2))
        else {
            return records;
        };
        let rdlength = read_u16(buf, offset + 8).unwrap_or(0) as usize;
        let rdata_start = offset + 10;
        let rdata_end = rdata_start.saturating_add(rdlength);
        if rdata_end > buf.len() {
            return records;
        }

        let mut record = MdnsRecord {
            owner,
            record_type,
            ptr_target: None,
            ipv4: None,
        };

        match record_type {
            TYPE_A => {
                if let Some(bytes) = buf.get(rdata_start..rdata_start + 4) {
                    record.ipv4 = Some(Ipv4Addr::new(
                        bytes[0], bytes[1], bytes[2], bytes[3],
                    ));
                }
            }
            TYPE_PTR => {
                if let Some((target, _)) = read_name(buf, rdata_start) {
                    record.ptr_target = Some(target);
                }
            }
            _ => {}
        }

        records.push(record);
        offset = rdata_end;
    }

    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_query_packet() {
        let packet = build_query("_http._tcp", 0x1234).unwrap();
        assert_eq!(&packet[..2], &[0x12, 0x34]);
        assert_eq!(&packet[2..4], &[0x00, 0x00]);
        assert_eq!(&packet[4..6], &[0x00, 0x01]);

        let question = &packet[12..];
        assert_eq!(question[0], 5);
        assert_eq!(&question[1..6], b"_http");
        assert!(packet.windows(4).any(|w| w == b"_tcp"));
        assert!(packet.windows(5).any(|w| w == b"local"));
        assert_eq!(&packet[packet.len() - 4..], &[0x00, 12, 0x00, 0x01]);
    }

    #[test]
    fn rejects_bad_labels() {
        assert!(build_query("", 1).is_none());
        assert!(build_query(&"a".repeat(80), 1).is_none());
    }

    #[test]
    fn parses_response_with_ptr_and_a() {
        let mut response = Vec::new();
        response.extend_from_slice(&[0xAB, 0xCD]); // id
        response.extend_from_slice(&[0x84, 0x00]); // response flags
        response.extend_from_slice(&[0x00, 0x00]); // no questions
        response.extend_from_slice(&[0x00, 0x02]); // two answers
        response.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // ns + ar counts

        // Answer 1: _http._tcp.local PTR printer._http._tcp.local
        response.push(5);
        response.extend_from_slice(b"_http");
        response.push(4);
        response.extend_from_slice(b"_tcp");
        response.push(5);
        response.extend_from_slice(b"local");
        response.push(0);
        response.extend_from_slice(&12u16.to_be_bytes()); // PTR
        response.extend_from_slice(&1u16.to_be_bytes()); // IN
        response.extend_from_slice(&120u32.to_be_bytes()); // TTL

        let target = vec![
            7u8, b'p', b'r', b'i', b'n', b't', b'e', b'r', 5, b'_', b'h', b't', b't', b'p', 4,
            b'_', b't', b'c', b'p', 5, b'l', b'o', b'c', b'a', b'l', 0,
        ];
        response.extend_from_slice(&(target.len() as u16).to_be_bytes());
        response.extend_from_slice(&target);

        // Answer 2: printer._http._tcp.local A 192.168.1.42
        response.push(7);
        response.extend_from_slice(b"printer");
        response.push(0xC0);
        response.push(12); // pointer to _http._tcp.local
        response.extend_from_slice(&1u16.to_be_bytes()); // A
        response.extend_from_slice(&1u16.to_be_bytes()); // IN
        response.extend_from_slice(&120u32.to_be_bytes());
        response.extend_from_slice(&4u16.to_be_bytes());
        response.extend_from_slice(&[192, 168, 1, 42]);

        let records = parse_response(&response);
        assert_eq!(records.len(), 2);

        assert_eq!(records[0].record_type, TYPE_PTR);
        assert_eq!(
            records[0].ptr_target.as_deref(),
            Some("printer._http._tcp.local")
        );

        assert_eq!(records[1].record_type, TYPE_A);
        assert_eq!(records[1].ipv4, Some(Ipv4Addr::new(192, 168, 1, 42)));
        assert_eq!(records[1].owner, "printer._http._tcp.local");
    }

    #[test]
    fn malformed_packets_return_empty() {
        assert!(parse_response(&[]).is_empty());
        assert!(parse_response(&[0, 1, 2]).is_empty());
        assert!(parse_response(&[0u8; 12]).is_empty());
    }
}
