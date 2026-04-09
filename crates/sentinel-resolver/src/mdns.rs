use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use dashmap::DashMap;
use tokio::net::UdpSocket;
use tracing::{debug, info, warn};

const MDNS_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_PORT: u16 = 5353;

/// Maps IP addresses to discovered device names.
pub type ClientNameMap = Arc<DashMap<IpAddr, DiscoveredClient>>;

#[derive(Debug, Clone)]
pub struct DiscoveredClient {
    pub hostname: String,
    pub device_type: DeviceType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    AppleTv,
    Chromecast,
    Printer,
    Speaker,
    Computer,
    Phone,
    Unknown,
}

impl DeviceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AppleTv => "apple_tv",
            Self::Chromecast => "chromecast",
            Self::Printer => "printer",
            Self::Speaker => "speaker",
            Self::Computer => "computer",
            Self::Phone => "phone",
            Self::Unknown => "unknown",
        }
    }

    fn from_service(service: &str) -> Self {
        let s = service.to_lowercase();
        if s.contains("_airplay._tcp") || s.contains("_raop._tcp") {
            return Self::AppleTv;
        }
        if s.contains("_googlecast._tcp") {
            return Self::Chromecast;
        }
        if s.contains("_ipp._tcp") || s.contains("_printer._tcp") || s.contains("_pdl-datastream._tcp") {
            return Self::Printer;
        }
        if s.contains("_sonos._tcp") || s.contains("_spotify-connect._tcp") {
            return Self::Speaker;
        }
        if s.contains("_smb._tcp") || s.contains("_afpovertcp._tcp") || s.contains("_ssh._tcp") {
            return Self::Computer;
        }
        if s.contains("_companion-link._tcp") {
            return Self::Phone;
        }
        Self::Unknown
    }
}

/// Spawn a background task that listens for mDNS announcements and
/// populates a shared `ClientNameMap`.
pub async fn run_mdns_listener(client_map: ClientNameMap) -> anyhow::Result<()> {
    let socket = match bind_mdns_socket().await {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "mDNS listener failed to bind — client discovery disabled");
            return Ok(());
        }
    };
    info!("mDNS listener active on 0.0.0.0:{}", MDNS_PORT);

    let mut buf = vec![0u8; 4096];
    loop {
        let (len, src) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                debug!(error = %e, "mDNS recv error");
                continue;
            }
        };

        if len < 12 {
            continue;
        }

        if let Some(info) = parse_mdns_response(&buf[..len]) {
            let ip = src.ip();
            debug!(ip = %ip, hostname = %info.hostname, device_type = ?info.device_type, "mDNS discovery");
            client_map.insert(ip, info);
        }
    }
}

async fn bind_mdns_socket() -> anyhow::Result<UdpSocket> {
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.set_nonblocking(true)?;

    let bind_addr: SocketAddr = (Ipv4Addr::UNSPECIFIED, MDNS_PORT).into();
    socket.bind(&bind_addr.into())?;
    socket.join_multicast_v4(&MDNS_ADDR, &Ipv4Addr::UNSPECIFIED)?;

    Ok(UdpSocket::from_std(socket.into())?)
}

/// Minimal mDNS response parser — extracts hostnames from PTR/SRV/A records.
fn parse_mdns_response(packet: &[u8]) -> Option<DiscoveredClient> {
    if packet.len() < 12 {
        return None;
    }

    let flags = u16::from_be_bytes([packet[2], packet[3]]);
    let is_response = flags & 0x8000 != 0;
    if !is_response {
        return None;
    }

    let ancount = u16::from_be_bytes([packet[6], packet[7]]) as usize;
    let arcount = u16::from_be_bytes([packet[10], packet[11]]) as usize;
    let qdcount = u16::from_be_bytes([packet[4], packet[5]]) as usize;

    // Skip questions
    let mut pos = 12;
    for _ in 0..qdcount {
        pos = skip_name(packet, pos)?;
        pos += 4; // qtype + qclass
        if pos > packet.len() {
            return None;
        }
    }

    let mut hostname: Option<String> = None;
    let mut device_type = DeviceType::Unknown;
    let mut service_names = Vec::new();

    let total_records = ancount + arcount;
    // Also handle the nscount (authority) records
    let nscount = u16::from_be_bytes([packet[8], packet[9]]) as usize;
    let all_records = total_records + nscount;

    for _ in 0..all_records {
        if pos >= packet.len() {
            break;
        }

        let (name, next_pos) = read_name(packet, pos)?;
        pos = next_pos;

        if pos + 10 > packet.len() {
            break;
        }

        let rtype = u16::from_be_bytes([packet[pos], packet[pos + 1]]);
        let rdlength = u16::from_be_bytes([packet[pos + 8], packet[pos + 9]]) as usize;
        pos += 10;

        if pos + rdlength > packet.len() {
            break;
        }

        match rtype {
            // PTR
            12 => {
                if let Some((ptr_name, _)) = read_name(packet, pos) {
                    service_names.push(name.clone());
                    if hostname.is_none() {
                        let clean = extract_hostname(&ptr_name);
                        if !clean.is_empty() {
                            hostname = Some(clean);
                        }
                    }
                }
            }
            // SRV
            33 => {
                if rdlength >= 6 {
                    if let Some((srv_target, _)) = read_name(packet, pos + 6) {
                        let clean = extract_hostname(&srv_target);
                        if !clean.is_empty() {
                            hostname = Some(clean);
                        }
                    }
                }
                service_names.push(name.clone());
            }
            // A record — name is the hostname
            1 => {
                let clean = extract_hostname(&name);
                if !clean.is_empty() && hostname.is_none() {
                    hostname = Some(clean);
                }
            }
            _ => {}
        }

        pos += rdlength;
    }

    for svc in &service_names {
        let dt = DeviceType::from_service(svc);
        if dt != DeviceType::Unknown {
            device_type = dt;
            break;
        }
    }

    hostname.map(|h| DiscoveredClient {
        hostname: h,
        device_type,
    })
}

fn extract_hostname(name: &str) -> String {
    name.strip_suffix(".local")
        .or_else(|| name.strip_suffix(".local."))
        .unwrap_or(name)
        .split("._")
        .next()
        .unwrap_or(name)
        .to_string()
}

fn skip_name(packet: &[u8], mut pos: usize) -> Option<usize> {
    loop {
        if pos >= packet.len() {
            return None;
        }
        let b = packet[pos];
        if b & 0xC0 == 0xC0 {
            return Some(pos + 2);
        }
        if b == 0 {
            return Some(pos + 1);
        }
        pos += 1 + b as usize;
    }
}

fn read_name(packet: &[u8], mut pos: usize) -> Option<(String, usize)> {
    let mut labels = Vec::new();
    let mut jumped = false;
    let mut end_pos = pos;
    let mut seen = HashMap::new();

    loop {
        if pos >= packet.len() {
            return None;
        }

        if seen.contains_key(&pos) {
            break; // loop detection
        }
        seen.insert(pos, true);

        let b = packet[pos];

        if b & 0xC0 == 0xC0 {
            if pos + 1 >= packet.len() {
                return None;
            }
            if !jumped {
                end_pos = pos + 2;
            }
            pos = ((b as usize & 0x3F) << 8) | packet[pos + 1] as usize;
            jumped = true;
            continue;
        }

        if b == 0 {
            if !jumped {
                end_pos = pos + 1;
            }
            break;
        }

        pos += 1;
        let len = b as usize;
        if pos + len > packet.len() {
            return None;
        }
        labels.push(
            std::str::from_utf8(&packet[pos..pos + len])
                .unwrap_or("?")
                .to_string(),
        );
        pos += len;
    }

    Some((labels.join("."), end_pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_type_detection() {
        assert_eq!(
            DeviceType::from_service("_airplay._tcp.local"),
            DeviceType::AppleTv
        );
        assert_eq!(
            DeviceType::from_service("_googlecast._tcp.local"),
            DeviceType::Chromecast
        );
        assert_eq!(
            DeviceType::from_service("_ipp._tcp.local"),
            DeviceType::Printer
        );
        assert_eq!(
            DeviceType::from_service("random-service._tcp.local"),
            DeviceType::Unknown
        );
    }

    #[test]
    fn extract_hostname_strips_local() {
        assert_eq!(extract_hostname("my-macbook.local"), "my-macbook");
        assert_eq!(extract_hostname("my-macbook.local."), "my-macbook");
        assert_eq!(
            extract_hostname("My Device._http._tcp.local"),
            "My Device"
        );
    }
}
