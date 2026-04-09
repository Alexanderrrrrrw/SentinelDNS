use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DnsAction {
    Allowed,
    Blocked,
    Cached,
}

impl DnsAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Blocked => "blocked",
            Self::Cached => "cached",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DnsProtocol {
    Udp,
    Tcp,
    Doh,
    Dot,
}

impl DnsProtocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Udp => "udp",
            Self::Tcp => "tcp",
            Self::Doh => "doh",
            Self::Dot => "dot",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BlockMode {
    Nxdomain,
    NullIp,
    Allow,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RiskPolicyMode {
    Block,
    Bypass,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DnsLogRecord {
    pub timestamp: DateTime<Utc>,
    pub client_id: String,
    pub query_domain: String,
    pub cname_chain: Vec<String>,
    pub action: DnsAction,
    pub protocol: DnsProtocol,
    pub response_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevicePolicy {
    pub id: Uuid,
    pub mac_address: String,
    pub group_memberships: Vec<String>,
    pub wireguard_enabled: bool,
    pub risk_policy_mode: RiskPolicyMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyDecision {
    pub mode: BlockMode,
    pub matched_domain: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WireGuardProfile {
    pub device_id: Uuid,
    pub profile_name: String,
    pub config: String,
}

// --- Gravity / Adlist types ---

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdlistKind {
    Block,
    Allow,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Adlist {
    pub id: i64,
    pub url: String,
    pub name: String,
    pub kind: AdlistKind,
    pub enabled: bool,
    pub domain_count: i64,
    pub last_updated: Option<DateTime<Utc>>,
    pub last_status: Option<String>,
}

// --- Domain rule types (exact + regex, allow + deny) ---

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DomainRuleKind {
    ExactDeny,
    ExactAllow,
    RegexDeny,
    RegexAllow,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DomainRule {
    pub id: i64,
    pub kind: DomainRuleKind,
    pub value: String,
    pub enabled: bool,
    pub comment: Option<String>,
}

// --- Group / Client types ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Group {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Client {
    pub id: i64,
    pub ip: String,
    pub name: Option<String>,
    pub group_ids: Vec<i64>,
}

// --- Helpers ---

pub fn normalize_domain(value: &str) -> String {
    value.trim().trim_end_matches('.').to_ascii_lowercase()
}

pub fn is_valid_domain(domain: &str) -> bool {
    if domain.is_empty() || domain.len() > 253 {
        return false;
    }
    let d = domain.trim_end_matches('.');
    d.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    })
}

pub fn is_valid_mac(mac: &str) -> bool {
    if mac.len() != 17 {
        return false;
    }
    mac.bytes().enumerate().all(|(i, b)| {
        if (i + 1) % 3 == 0 {
            b == b':'
        } else {
            b.is_ascii_hexdigit()
        }
    })
}

// --- Traits ---

pub trait PolicyEngine: Send + Sync {
    fn decide(&self, query_domain: &str, cname_chain: &[String]) -> PolicyDecision;
    fn decide_for_client(
        &self,
        query_domain: &str,
        cname_chain: &[String],
        client_ip: Option<&str>,
    ) -> PolicyDecision {
        let _ = client_ip;
        self.decide(query_domain, cname_chain)
    }
    /// Quick single-domain check used during CNAME chain walking.
    /// Returns Some(decision) if the domain should be blocked/allowed,
    /// None if no policy matches (continue walking).
    fn check_domain_quick(&self, domain: &str) -> Option<PolicyDecision> {
        let _ = domain;
        None
    }
}

pub trait ConfigStore: Send + Sync {
    fn upsert_device_policy(&self, policy: DevicePolicy) -> anyhow::Result<()>;
    fn get_device_policy(&self, device_id: Uuid) -> anyhow::Result<Option<DevicePolicy>>;
    fn list_device_policies(&self) -> anyhow::Result<Vec<DevicePolicy>>;
}

pub trait LogStore: Send + Sync {
    fn append_dns_logs(&self, rows: &[DnsLogRecord]) -> anyhow::Result<()>;
}

pub trait AdlistStore: Send + Sync {
    fn create_adlist(&self, url: &str, name: &str, kind: AdlistKind) -> anyhow::Result<Adlist>;
    fn list_adlists(&self) -> anyhow::Result<Vec<Adlist>>;
    fn update_adlist_status(
        &self,
        id: i64,
        domain_count: i64,
        status: &str,
    ) -> anyhow::Result<()>;
    fn delete_adlist(&self, id: i64) -> anyhow::Result<()>;
    fn toggle_adlist(&self, id: i64, enabled: bool) -> anyhow::Result<()>;
}

pub trait DomainRuleStore: Send + Sync {
    fn create_domain_rule(
        &self,
        kind: DomainRuleKind,
        value: &str,
        comment: Option<&str>,
    ) -> anyhow::Result<DomainRule>;
    fn list_domain_rules(&self) -> anyhow::Result<Vec<DomainRule>>;
    fn delete_domain_rule(&self, id: i64) -> anyhow::Result<()>;
    fn toggle_domain_rule(&self, id: i64, enabled: bool) -> anyhow::Result<()>;
}

pub trait GroupStore: Send + Sync {
    fn create_group(&self, name: &str, description: Option<&str>) -> anyhow::Result<Group>;
    fn list_groups(&self) -> anyhow::Result<Vec<Group>>;
    fn delete_group(&self, id: i64) -> anyhow::Result<()>;
    fn assign_adlist_to_group(&self, adlist_id: i64, group_id: i64) -> anyhow::Result<()>;
    fn remove_adlist_from_group(&self, adlist_id: i64, group_id: i64) -> anyhow::Result<()>;
    fn assign_rule_to_group(&self, rule_id: i64, group_id: i64) -> anyhow::Result<()>;
    fn remove_rule_from_group(&self, rule_id: i64, group_id: i64) -> anyhow::Result<()>;
}

pub trait ClientStore: Send + Sync {
    fn upsert_client(&self, ip: &str, name: Option<&str>, group_ids: &[i64])
        -> anyhow::Result<Client>;
    fn list_clients(&self) -> anyhow::Result<Vec<Client>>;
    fn delete_client(&self, id: i64) -> anyhow::Result<()>;
    fn get_client_by_ip(&self, ip: &str) -> anyhow::Result<Option<Client>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_policy_enum_in_snake_case() {
        let raw = serde_json::to_string(&BlockMode::NullIp).expect("enum serialization must work");
        assert_eq!(raw, "\"null_ip\"");
    }

    #[test]
    fn roundtrips_dns_log_record() {
        let row = DnsLogRecord {
            timestamp: Utc::now(),
            client_id: "device-1".to_string(),
            query_domain: "d.example.com".to_string(),
            cname_chain: vec![
                "d.example.com".to_string(),
                "tracker.example.net".to_string(),
            ],
            action: DnsAction::Blocked,
            protocol: DnsProtocol::Udp,
            response_time_ms: 7,
        };

        let json = serde_json::to_string(&row).expect("log record serialization must work");
        let parsed: DnsLogRecord =
            serde_json::from_str(&json).expect("log record deserialization must work");
        assert_eq!(parsed.client_id, "device-1");
        assert_eq!(parsed.action, DnsAction::Blocked);
    }

    #[test]
    fn validates_domains() {
        assert!(is_valid_domain("example.com"));
        assert!(is_valid_domain("sub.domain.example.com"));
        assert!(!is_valid_domain(""));
        assert!(!is_valid_domain("-bad.com"));
        assert!(!is_valid_domain("bad-.com"));
    }

    #[test]
    fn validates_macs() {
        assert!(is_valid_mac("AA:BB:CC:DD:EE:FF"));
        assert!(is_valid_mac("00:11:22:33:44:55"));
        assert!(!is_valid_mac("not-a-mac"));
        assert!(!is_valid_mac("AA:BB:CC:DD:EE"));
    }

    #[test]
    fn action_as_str() {
        assert_eq!(DnsAction::Allowed.as_str(), "allowed");
        assert_eq!(DnsAction::Blocked.as_str(), "blocked");
        assert_eq!(DnsAction::Cached.as_str(), "cached");
    }

    #[test]
    fn adlist_kind_roundtrips() {
        let json = serde_json::to_string(&AdlistKind::Block).unwrap();
        assert_eq!(json, "\"block\"");
        let parsed: AdlistKind = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, AdlistKind::Block);
    }

    #[test]
    fn domain_rule_kind_roundtrips() {
        let json = serde_json::to_string(&DomainRuleKind::RegexAllow).unwrap();
        assert_eq!(json, "\"regex_allow\"");
        let parsed: DomainRuleKind = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, DomainRuleKind::RegexAllow);
    }
}
