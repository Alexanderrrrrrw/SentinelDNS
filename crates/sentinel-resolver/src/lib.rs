pub mod mdns;

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use hickory_resolver::config::{
    NameServerConfig, NameServerConfigGroup, Protocol, ResolverConfig, ResolverOpts,
};
use hickory_resolver::TokioAsyncResolver;
use sentinel_types::{
    normalize_domain, BlockMode, DnsAction, DnsLogRecord, DnsProtocol, PolicyDecision, PolicyEngine,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::Semaphore;
use tracing::{error, info, warn};

const MAX_CACHE_ENTRIES: usize = 50_000;
const CACHE_TTL: Duration = Duration::from_secs(300);
const MAX_CONCURRENT_DNS_TASKS: usize = 1024;

struct CacheEntry {
    chain: Vec<String>,
    inserted: Instant,
}

pub struct SentinelResolver {
    pub upstream: TokioAsyncResolver,
    cache: DashMap<String, CacheEntry>,
    policy: Arc<dyn PolicyEngine>,
    max_cname_depth: usize,
}

#[derive(Clone)]
pub struct ResolveResult {
    pub action: DnsAction,
    pub cname_chain: Vec<String>,
    pub response_time_ms: u64,
    pub block_mode: BlockMode,
}

/// Upstream protocol selection for DNS queries
#[derive(Debug, Clone)]
pub enum UpstreamConfig {
    /// Plain UDP+TCP DNS (default system resolvers)
    Default,
    /// DNS-over-TLS to the given server IP and TLS hostname
    Tls {
        addr: SocketAddr,
        tls_name: String,
    },
    /// DNS-over-HTTPS to the given server IP and TLS hostname
    Https {
        addr: SocketAddr,
        tls_name: String,
    },
}

impl SentinelResolver {
    pub fn new(policy: Arc<dyn PolicyEngine>, _block_mode: BlockMode) -> Self {
        Self::with_upstream(policy, _block_mode, &UpstreamConfig::Default)
    }

    pub fn with_upstream(
        policy: Arc<dyn PolicyEngine>,
        _block_mode: BlockMode,
        upstream_config: &UpstreamConfig,
    ) -> Self {
        let (config, opts) = match upstream_config {
            UpstreamConfig::Default => (ResolverConfig::default(), ResolverOpts::default()),
            UpstreamConfig::Tls { addr, tls_name } => {
                let mut ns = NameServerConfig::new(*addr, Protocol::Tls);
                ns.tls_dns_name = Some(tls_name.clone());
                let group = NameServerConfigGroup::from(vec![ns]);
                (
                    ResolverConfig::from_parts(None, vec![], group),
                    ResolverOpts::default(),
                )
            }
            UpstreamConfig::Https { addr, tls_name } => {
                let mut ns = NameServerConfig::new(*addr, Protocol::Https);
                ns.tls_dns_name = Some(tls_name.clone());
                let group = NameServerConfigGroup::from(vec![ns]);
                (
                    ResolverConfig::from_parts(None, vec![], group),
                    ResolverOpts::default(),
                )
            }
        };
        let upstream = TokioAsyncResolver::tokio(config, opts);
        Self {
            upstream,
            cache: DashMap::new(),
            policy,
            max_cname_depth: 8,
        }
    }

    pub async fn resolve_domain(&self, domain: &str) -> anyhow::Result<ResolveResult> {
        self.resolve_domain_for_client(domain, None).await
    }

    pub async fn resolve_domain_for_client(
        &self,
        domain: &str,
        client_ip: Option<&str>,
    ) -> anyhow::Result<ResolveResult> {
        let started = Instant::now();
        let normalized = normalize_domain(domain);

        if let Some(entry) = self.cache.get(&normalized) {
            if entry.inserted.elapsed() < CACHE_TTL {
                let decision =
                    self.policy
                        .decide_for_client(&normalized, &entry.chain, client_ip);
                let action = match decision.mode {
                    BlockMode::Allow => DnsAction::Cached,
                    _ => DnsAction::Blocked,
                };
                return Ok(ResolveResult {
                    action,
                    cname_chain: entry.chain.clone(),
                    response_time_ms: started.elapsed().as_millis() as u64,
                    block_mode: decision.mode,
                });
            }
            drop(entry);
            self.cache.remove(&normalized);
        }

        let (cname_chain, early_block) = self.walk_cname_chain(&normalized).await;

        // If the CNAME walk short-circuited on a blocked hop, use that decision
        let decision = if let Some(block_decision) = early_block {
            block_decision
        } else {
            self.policy
                .decide_for_client(&normalized, &cname_chain, client_ip)
        };
        let action = match decision.mode {
            BlockMode::Allow => DnsAction::Allowed,
            _ => DnsAction::Blocked,
        };

        self.evict_if_needed();
        self.cache.insert(
            normalized,
            CacheEntry {
                chain: cname_chain.clone(),
                inserted: Instant::now(),
            },
        );

        Ok(ResolveResult {
            action,
            cname_chain,
            response_time_ms: started.elapsed().as_millis() as u64,
            block_mode: decision.mode,
        })
    }

    /// Walk the CNAME chain, checking policy at every hop.
    /// Returns (chain, early_block) — if early_block is Some, a CNAME hop
    /// matched a blocklist and we short-circuited to avoid wasted upstream lookups.
    async fn walk_cname_chain(&self, domain: &str) -> (Vec<String>, Option<PolicyDecision>) {
        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        let mut current = domain.to_string();

        for _ in 0..self.max_cname_depth {
            if !visited.insert(current.clone()) {
                break;
            }
            chain.push(current.clone());

            // Short-circuit: check if this hop is blocked before making upstream queries
            if chain.len() > 1 {
                if let Some(decision) = self.policy.check_domain_quick(&current) {
                    if decision.mode != BlockMode::Allow {
                        return (chain, Some(decision));
                    }
                }
            }

            let lookup = self
                .upstream
                .lookup(
                    current.clone(),
                    hickory_resolver::proto::rr::RecordType::CNAME,
                )
                .await;
            let answer = match lookup {
                Ok(answer) => answer,
                Err(e) => {
                    if !e.to_string().contains("no records found") {
                        warn!(domain = %current, error = %e, "cname lookup failed");
                    }
                    break;
                }
            };

            match answer.iter().next().map(|r| r.to_string()) {
                Some(candidate) => current = normalize_domain(&candidate),
                None => break,
            }
        }

        (chain, None)
    }

    fn evict_if_needed(&self) {
        if self.cache.len() < MAX_CACHE_ENTRIES {
            return;
        }
        let now = Instant::now();
        self.cache
            .retain(|_, entry| now.duration_since(entry.inserted) < CACHE_TTL);
        if self.cache.len() >= MAX_CACHE_ENTRIES {
            let to_remove: Vec<String> = self
                .cache
                .iter()
                .take(MAX_CACHE_ENTRIES / 4)
                .map(|r| r.key().clone())
                .collect();
            for key in to_remove {
                self.cache.remove(&key);
            }
        }
    }

    pub fn into_log_record(
        client_id: &str,
        query_domain: &str,
        protocol: DnsProtocol,
        result: &ResolveResult,
    ) -> DnsLogRecord {
        DnsLogRecord {
            timestamp: chrono::Utc::now(),
            client_id: client_id.to_string(),
            query_domain: query_domain.to_string(),
            cname_chain: result.cname_chain.clone(),
            action: result.action,
            protocol,
            response_time_ms: result.response_time_ms,
        }
    }

    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }
}

pub type LogSender = tokio::sync::mpsc::Sender<DnsLogRecord>;

pub async fn run_dns_listener(
    bind_addr: SocketAddr,
    resolver: Arc<SentinelResolver>,
    log_tx: Option<LogSender>,
) -> anyhow::Result<()> {
    let socket = Arc::new(UdpSocket::bind(bind_addr).await?);
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DNS_TASKS));
    let log_tx = log_tx.map(Arc::new);
    info!("DNS listener bound on {}", bind_addr);

    let mut buf = vec![0u8; 512];
    loop {
        let (len, src) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                error!(error = %e, "failed to receive UDP packet");
                continue;
            }
        };

        let permit = match semaphore.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                warn!("DNS task limit reached, dropping packet from {}", src);
                continue;
            }
        };

        let packet = buf[..len].to_vec();
        let sock = Arc::clone(&socket);
        let resolver = Arc::clone(&resolver);
        let log_tx = log_tx.clone();

        tokio::spawn(async move {
            let _permit = permit;
            if let Err(e) = handle_dns_packet(&sock, &packet, src, &resolver, log_tx.as_deref())
                .await
            {
                warn!(error = %e, client = %src, "dns query handling failed");
            }
        });
    }
}

pub async fn run_tcp_dns_listener(
    bind_addr: SocketAddr,
    resolver: Arc<SentinelResolver>,
    log_tx: Option<LogSender>,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(bind_addr).await?;
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DNS_TASKS));
    let log_tx = log_tx.map(Arc::new);
    info!("TCP DNS listener bound on {}", bind_addr);

    loop {
        let (stream, src) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                error!(error = %e, "failed to accept TCP connection");
                continue;
            }
        };

        let permit = match semaphore.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                warn!("TCP DNS task limit reached, dropping connection from {}", src);
                continue;
            }
        };

        let resolver = Arc::clone(&resolver);
        let log_tx = log_tx.clone();

        tokio::spawn(async move {
            let _permit = permit;
            if let Err(e) =
                handle_tcp_dns_stream(stream, src, &resolver, log_tx.as_deref()).await
            {
                warn!(error = %e, client = %src, "TCP DNS handling failed");
            }
        });
    }
}

async fn handle_tcp_dns_stream(
    mut stream: tokio::net::TcpStream,
    src: SocketAddr,
    resolver: &SentinelResolver,
    log_tx: Option<&LogSender>,
) -> anyhow::Result<()> {
    loop {
        let mut len_buf = [0u8; 2];
        if stream.read_exact(&mut len_buf).await.is_err() {
            break;
        }
        let msg_len = u16::from_be_bytes(len_buf) as usize;
        if msg_len == 0 || msg_len > 65535 {
            break;
        }

        let mut packet = vec![0u8; msg_len];
        stream.read_exact(&mut packet).await?;

        let response = process_dns_query(&packet, src, resolver, log_tx, DnsProtocol::Tcp).await;
        if let Some(resp) = response {
            let resp_len = (resp.len() as u16).to_be_bytes();
            stream.write_all(&resp_len).await?;
            stream.write_all(&resp).await?;
        }
    }
    Ok(())
}

async fn process_dns_query(
    packet: &[u8],
    src: SocketAddr,
    resolver: &SentinelResolver,
    log_tx: Option<&LogSender>,
    protocol: DnsProtocol,
) -> Option<Vec<u8>> {
    if packet.len() < 12 {
        return None;
    }
    let id = u16::from_be_bytes([packet[0], packet[1]]);
    let qdcount = u16::from_be_bytes([packet[4], packet[5]]);
    if qdcount == 0 {
        return None;
    }
    let (domain, qtype, offset) = match parse_question(packet, 12) {
        Ok(v) => v,
        Err(_) => return None,
    };
    let result = match resolver
        .resolve_domain_for_client(&domain, Some(&src.ip().to_string()))
        .await
    {
        Ok(r) => r,
        Err(_) => return None,
    };

    if let Some(tx) = log_tx {
        let log_record =
            SentinelResolver::into_log_record(&src.ip().to_string(), &domain, protocol, &result);
        let _ = tx.try_send(log_record);
    }

    let response = match result.action {
        DnsAction::Blocked => match result.block_mode {
            BlockMode::NullIp => {
                if qtype == 1 {
                    build_a_response(id, packet, offset, &[[0, 0, 0, 0]])
                } else if qtype == 28 {
                    build_aaaa_response(id, packet, offset, &[[0; 16]])
                } else {
                    build_nxdomain_response(id, packet, offset)
                }
            }
            _ => build_nxdomain_response(id, packet, offset),
        },
        _ => {
            let rtype = qtype_to_record_type(qtype);
            match resolver.upstream.lookup(&domain, rtype).await {
                Ok(lookup) => {
                    if qtype == 1 {
                        let ips: Vec<[u8; 4]> = lookup
                            .iter()
                            .filter_map(|r| {
                                r.to_string()
                                    .parse::<std::net::Ipv4Addr>()
                                    .ok()
                                    .map(|ip| ip.octets())
                            })
                            .collect();
                        build_a_response(id, packet, offset, &ips)
                    } else if qtype == 28 {
                        let ips: Vec<[u8; 16]> = lookup
                            .iter()
                            .filter_map(|r| {
                                r.to_string()
                                    .parse::<std::net::Ipv6Addr>()
                                    .ok()
                                    .map(|ip| ip.octets())
                            })
                            .collect();
                        build_aaaa_response(id, packet, offset, &ips)
                    } else {
                        build_noerror_empty_response(id, packet, offset)
                    }
                }
                Err(_) => build_nxdomain_response(id, packet, offset),
            }
        }
    };
    Some(response)
}

async fn handle_dns_packet(
    socket: &UdpSocket,
    packet: &[u8],
    src: SocketAddr,
    resolver: &SentinelResolver,
    log_tx: Option<&LogSender>,
) -> anyhow::Result<()> {
    if let Some(response) = process_dns_query(packet, src, resolver, log_tx, DnsProtocol::Udp).await
    {
        socket.send_to(&response, src).await?;
    }
    Ok(())
}

fn qtype_to_record_type(qtype: u16) -> hickory_resolver::proto::rr::RecordType {
    use hickory_resolver::proto::rr::RecordType;
    match qtype {
        1 => RecordType::A,
        2 => RecordType::NS,
        5 => RecordType::CNAME,
        6 => RecordType::SOA,
        15 => RecordType::MX,
        16 => RecordType::TXT,
        28 => RecordType::AAAA,
        33 => RecordType::SRV,
        _ => RecordType::A,
    }
}

fn parse_question(packet: &[u8], mut pos: usize) -> anyhow::Result<(String, u16, usize)> {
    let mut labels = Vec::new();
    loop {
        if pos >= packet.len() {
            anyhow::bail!("truncated question");
        }
        let byte = packet[pos];

        if byte & 0xC0 == 0xC0 {
            if pos + 1 >= packet.len() {
                anyhow::bail!("truncated compression pointer");
            }
            let ptr = ((byte as usize & 0x3F) << 8) | packet[pos + 1] as usize;
            pos += 2;
            let (suffix, _, _) = parse_question(packet, ptr)?;
            if !suffix.is_empty() {
                labels.push(suffix);
            }
            break;
        }

        let len = byte as usize;
        pos += 1;
        if len == 0 {
            break;
        }
        if pos + len > packet.len() {
            anyhow::bail!("label exceeds packet");
        }
        labels.push(
            std::str::from_utf8(&packet[pos..pos + len])
                .unwrap_or("?")
                .to_string(),
        );
        pos += len;
    }

    if pos + 4 > packet.len() {
        anyhow::bail!("truncated qtype/qclass");
    }
    let qtype = u16::from_be_bytes([packet[pos], packet[pos + 1]]);
    pos += 4;

    Ok((labels.join("."), qtype, pos))
}

fn build_nxdomain_response(id: u16, query: &[u8], question_end: usize) -> Vec<u8> {
    let qend = question_end.min(query.len());
    let mut resp = Vec::with_capacity(qend);
    resp.extend_from_slice(&query[..qend]);
    if resp.len() < 12 {
        resp.resize(12, 0);
    }
    resp[0] = id.to_be_bytes()[0];
    resp[1] = id.to_be_bytes()[1];
    resp[2] = 0x81;
    resp[3] = 0x83;
    resp[6] = 0;
    resp[7] = 0;
    resp[8] = 0;
    resp[9] = 0;
    resp[10] = 0;
    resp[11] = 0;
    resp
}

fn build_noerror_empty_response(id: u16, query: &[u8], question_end: usize) -> Vec<u8> {
    let qend = question_end.min(query.len());
    let mut resp = Vec::with_capacity(qend);
    resp.extend_from_slice(&query[..qend]);
    if resp.len() < 12 {
        resp.resize(12, 0);
    }
    resp[0] = id.to_be_bytes()[0];
    resp[1] = id.to_be_bytes()[1];
    resp[2] = 0x81;
    resp[3] = 0x80; // NOERROR
    resp[6] = 0;
    resp[7] = 0;
    resp[8] = 0;
    resp[9] = 0;
    resp[10] = 0;
    resp[11] = 0;
    resp
}

fn build_a_response(id: u16, query: &[u8], question_end: usize, ips: &[[u8; 4]]) -> Vec<u8> {
    let ancount = ips.len() as u16;
    let qend = question_end.min(query.len());
    let mut resp = Vec::with_capacity(qend + ips.len() * 16);
    resp.extend_from_slice(&query[..qend]);
    if resp.len() < 12 {
        resp.resize(12, 0);
    }
    resp[0] = id.to_be_bytes()[0];
    resp[1] = id.to_be_bytes()[1];
    resp[2] = 0x81;
    resp[3] = 0x80;
    resp[6] = ancount.to_be_bytes()[0];
    resp[7] = ancount.to_be_bytes()[1];
    resp[8] = 0;
    resp[9] = 0;
    resp[10] = 0;
    resp[11] = 0;

    for ip in ips {
        resp.extend_from_slice(&[0xC0, 0x0C]);
        resp.extend_from_slice(&1u16.to_be_bytes());
        resp.extend_from_slice(&1u16.to_be_bytes());
        resp.extend_from_slice(&60u32.to_be_bytes());
        resp.extend_from_slice(&4u16.to_be_bytes());
        resp.extend_from_slice(ip);
    }

    resp
}

fn build_aaaa_response(id: u16, query: &[u8], question_end: usize, ips: &[[u8; 16]]) -> Vec<u8> {
    let ancount = ips.len() as u16;
    let qend = question_end.min(query.len());
    let mut resp = Vec::with_capacity(qend + ips.len() * 28);
    resp.extend_from_slice(&query[..qend]);
    if resp.len() < 12 {
        resp.resize(12, 0);
    }
    resp[0] = id.to_be_bytes()[0];
    resp[1] = id.to_be_bytes()[1];
    resp[2] = 0x81;
    resp[3] = 0x80;
    resp[6] = ancount.to_be_bytes()[0];
    resp[7] = ancount.to_be_bytes()[1];
    resp[8] = 0;
    resp[9] = 0;
    resp[10] = 0;
    resp[11] = 0;

    for ip in ips {
        resp.extend_from_slice(&[0xC0, 0x0C]);
        resp.extend_from_slice(&28u16.to_be_bytes());
        resp.extend_from_slice(&1u16.to_be_bytes());
        resp.extend_from_slice(&60u32.to_be_bytes());
        resp.extend_from_slice(&16u16.to_be_bytes());
        resp.extend_from_slice(ip);
    }

    resp
}

pub fn synthetic_chain_benchmark_avg(total_ms: u64, iterations: usize) -> u64 {
    if iterations == 0 {
        return 0;
    }
    total_ms / iterations as u64
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sentinel_policy::SentinelPolicyEngine;
    use sentinel_types::{BlockMode, PolicyEngine};

    use super::*;

    #[tokio::test]
    async fn resolver_returns_result() {
        let policy: Arc<dyn PolicyEngine> = Arc::new(SentinelPolicyEngine::from_blocklist_text(
            BlockMode::Nxdomain,
            "ads.example.com",
        ));
        let resolver = SentinelResolver::new(policy, BlockMode::Nxdomain);
        let result = resolver
            .resolve_domain("example.com")
            .await
            .expect("resolve should succeed");
        assert!(result.response_time_ms < 10000);
    }

    #[test]
    fn cache_eviction_removes_expired() {
        let policy: Arc<dyn PolicyEngine> = Arc::new(SentinelPolicyEngine::from_blocklist_text(
            BlockMode::Nxdomain,
            "",
        ));
        let resolver = SentinelResolver::new(policy, BlockMode::Nxdomain);

        // Fill past the threshold so evict_if_needed actually runs
        for i in 0..MAX_CACHE_ENTRIES + 10 {
            resolver.cache.insert(
                format!("d{i}.example.com"),
                CacheEntry {
                    chain: vec![],
                    inserted: Instant::now() - Duration::from_secs(600),
                },
            );
        }
        assert!(resolver.cache.len() > MAX_CACHE_ENTRIES);
        resolver.evict_if_needed();
        // All entries are expired, so TTL-based eviction removes them all
        assert_eq!(resolver.cache.len(), 0);
    }

    #[test]
    fn benchmark_avg_handles_zero() {
        assert_eq!(synthetic_chain_benchmark_avg(100, 0), 0);
        assert_eq!(synthetic_chain_benchmark_avg(100, 10), 10);
    }

    #[test]
    fn nxdomain_response_has_correct_header() {
        let query = vec![
            0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, b'e',
            b'x', b'a', b'm', b'p', b'l', b'e', 0x03, b'c', b'o', b'm', 0x00, 0x00, 0x01, 0x00,
            0x01,
        ];
        let resp = build_nxdomain_response(1, &query, query.len());
        assert_eq!(resp[2], 0x81);
        assert_eq!(resp[3], 0x83);
    }

    #[test]
    fn nullip_response_returns_zero_ip() {
        let query = vec![
            0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, b'e',
            b'x', b'a', b'm', b'p', b'l', b'e', 0x03, b'c', b'o', b'm', 0x00, 0x00, 0x01, 0x00,
            0x01,
        ];
        let resp = build_a_response(1, &query, query.len(), &[[0, 0, 0, 0]]);
        assert_eq!(resp[2], 0x81);
        assert_eq!(resp[3], 0x80);
        assert_eq!(resp[6], 0);
        assert_eq!(resp[7], 1);
        let rdata_start = query.len() + 12;
        assert_eq!(&resp[rdata_start..rdata_start + 4], &[0, 0, 0, 0]);
    }

    #[test]
    fn parse_question_handles_normal() {
        let mut packet = vec![
            0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        packet.extend_from_slice(&[0x07, b'e', b'x', b'a', b'm', b'p', b'l', b'e']);
        packet.extend_from_slice(&[0x03, b'c', b'o', b'm', 0x00]);
        packet.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]);
        let (domain, qtype, _) = parse_question(&packet, 12).unwrap();
        assert_eq!(domain, "example.com");
        assert_eq!(qtype, 1);
    }

    #[test]
    fn noerror_empty_response() {
        let query = vec![
            0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, b'e',
            b'x', b'a', b'm', b'p', b'l', b'e', 0x03, b'c', b'o', b'm', 0x00, 0x00, 0x10, 0x00,
            0x01,
        ];
        let resp = build_noerror_empty_response(1, &query, query.len());
        assert_eq!(resp[2], 0x81);
        assert_eq!(resp[3], 0x80);
        assert_eq!(resp[6], 0);
        assert_eq!(resp[7], 0);
    }
}
