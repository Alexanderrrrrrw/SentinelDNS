pub mod heuristics;
pub mod presets;

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::io::BufRead;
use std::path::Path;
use std::sync::Arc;

use arc_swap::ArcSwap;
use regex::RegexSet;
use sentinel_types::{
    normalize_domain, AdlistKind, BlockMode, DomainRule, DomainRuleKind, PolicyDecision,
    PolicyEngine,
};
use tracing::warn;

// ─── Bloom filter ───

#[derive(Debug, Clone)]
pub struct BloomFilter {
    bits: Vec<u8>,
    bit_count: usize,
    hash_count: usize,
}

impl BloomFilter {
    pub fn new(bit_count: usize, hash_count: usize) -> Self {
        Self {
            bits: vec![0; bit_count.div_ceil(8)],
            bit_count,
            hash_count,
        }
    }

    pub fn insert(&mut self, value: &str) {
        for index in self.indexes(value) {
            let byte_index = index / 8;
            let bit_index = index % 8;
            self.bits[byte_index] |= 1 << bit_index;
        }
    }

    pub fn maybe_contains(&self, value: &str) -> bool {
        self.indexes(value).into_iter().all(|index| {
            let byte_index = index / 8;
            let bit_index = index % 8;
            (self.bits[byte_index] & (1 << bit_index)) != 0
        })
    }

    fn indexes(&self, value: &str) -> Vec<usize> {
        let mut indexes = Vec::with_capacity(self.hash_count);
        for seed in 0..self.hash_count {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            seed.hash(&mut hasher);
            value.hash(&mut hasher);
            indexes.push((hasher.finish() as usize) % self.bit_count);
        }
        indexes
    }
}

// ─── Domain block index (exact + wildcard) ───

#[derive(Debug, Clone)]
pub struct DomainBlockIndex {
    bloom: BloomFilter,
    exact: HashSet<String>,
    wildcards: Vec<String>,
}

impl DomainBlockIndex {
    pub fn from_domains(domains: Vec<String>) -> Self {
        let count = domains.len().max(1024);
        let bit_count = count * 10;
        let mut bloom = BloomFilter::new(bit_count, 5);
        let mut exact = HashSet::with_capacity(domains.len());
        let mut wildcards = Vec::new();

        for domain in domains {
            let d = normalize_domain(&domain);
            if d.starts_with("*.") {
                wildcards.push(d[1..].to_string());
            } else {
                bloom.insert(&d);
                exact.insert(d);
            }
        }

        Self {
            bloom,
            exact,
            wildcards,
        }
    }

    pub fn domain_count(&self) -> usize {
        self.exact.len() + self.wildcards.len()
    }

    pub fn is_blocked(&self, domain: &str) -> bool {
        let d = normalize_domain(domain);
        if self.bloom.maybe_contains(&d) && self.exact.contains(&d) {
            return true;
        }
        for suffix in &self.wildcards {
            if d.ends_with(suffix.as_str()) || d == suffix[1..] {
                return true;
            }
        }
        false
    }
}

// ─── Regex index ───

#[derive(Debug, Clone)]
#[derive(Default)]
pub struct RegexIndex {
    deny_patterns: Vec<String>,
    deny_set: Option<RegexSet>,
    allow_patterns: Vec<String>,
    allow_set: Option<RegexSet>,
}


impl RegexIndex {
    pub fn from_rules(rules: &[DomainRule]) -> Self {
        let mut deny_pats = Vec::new();
        let mut allow_pats = Vec::new();

        for rule in rules {
            if !rule.enabled {
                continue;
            }
            match rule.kind {
                DomainRuleKind::RegexDeny => deny_pats.push(rule.value.clone()),
                DomainRuleKind::RegexAllow => allow_pats.push(rule.value.clone()),
                _ => {}
            }
        }

        let deny_set = if deny_pats.is_empty() {
            None
        } else {
            match RegexSet::new(&deny_pats) {
                Ok(s) => Some(s),
                Err(e) => {
                    warn!(error = %e, "failed to compile deny regex set");
                    None
                }
            }
        };

        let allow_set = if allow_pats.is_empty() {
            None
        } else {
            match RegexSet::new(&allow_pats) {
                Ok(s) => Some(s),
                Err(e) => {
                    warn!(error = %e, "failed to compile allow regex set");
                    None
                }
            }
        };

        Self {
            deny_patterns: deny_pats,
            deny_set,
            allow_patterns: allow_pats,
            allow_set,
        }
    }

    pub fn is_regex_allowed(&self, domain: &str) -> bool {
        self.allow_set
            .as_ref()
            .is_some_and(|s| s.is_match(domain))
    }

    pub fn is_regex_denied(&self, domain: &str) -> bool {
        self.deny_set
            .as_ref()
            .is_some_and(|s| s.is_match(domain))
    }

    pub fn deny_count(&self) -> usize {
        self.deny_patterns.len()
    }

    pub fn allow_count(&self) -> usize {
        self.allow_patterns.len()
    }
}

// ─── Compiled policy snapshot (what ArcSwap holds) ───

#[derive(Debug, Clone)]
pub struct PolicySnapshot {
    pub block_mode: BlockMode,
    pub block_index: DomainBlockIndex,
    pub allow_index: DomainBlockIndex,
    pub exact_allow: HashSet<String>,
    pub exact_deny: HashSet<String>,
    pub regex_index: RegexIndex,
    pub heuristics_enabled: bool,
}

impl PolicySnapshot {
    pub fn empty(block_mode: BlockMode) -> Self {
        Self {
            block_mode,
            block_index: DomainBlockIndex::from_domains(Vec::new()),
            allow_index: DomainBlockIndex::from_domains(Vec::new()),
            exact_allow: HashSet::new(),
            exact_deny: HashSet::new(),
            regex_index: RegexIndex::default(),
            heuristics_enabled: true,
        }
    }

    fn check_domain(&self, domain: &str) -> Option<PolicyDecision> {
        let d = normalize_domain(domain);

        // Priority: exact allow > regex allow > exact deny > regex deny > gravity > heuristics
        if self.exact_allow.contains(&d) {
            return Some(PolicyDecision {
                mode: BlockMode::Allow,
                matched_domain: None,
                reason: "exact allow rule".to_string(),
            });
        }

        if self.regex_index.is_regex_allowed(&d) {
            return Some(PolicyDecision {
                mode: BlockMode::Allow,
                matched_domain: None,
                reason: "regex allow rule".to_string(),
            });
        }

        if self.exact_deny.contains(&d) {
            return Some(PolicyDecision {
                mode: self.block_mode,
                matched_domain: Some(d),
                reason: "exact deny rule".to_string(),
            });
        }

        if self.regex_index.is_regex_denied(&d) {
            return Some(PolicyDecision {
                mode: self.block_mode,
                matched_domain: Some(d),
                reason: "regex deny rule".to_string(),
            });
        }

        if self.allow_index.is_blocked(domain) {
            return Some(PolicyDecision {
                mode: BlockMode::Allow,
                matched_domain: None,
                reason: "gravity allowlist".to_string(),
            });
        }

        if self.block_index.is_blocked(domain) {
            return Some(PolicyDecision {
                mode: self.block_mode,
                matched_domain: Some(d.clone()),
                reason: "gravity blocklist".to_string(),
            });
        }

        // Heuristic scoring — catches domains no blocklist has indexed
        if self.heuristics_enabled {
            let result = heuristics::score_domain(&d);
            if result.verdict == heuristics::Verdict::Suspicious {
                let reason = format!(
                    "heuristic block (score={:.0}): {}",
                    result.score,
                    result
                        .signals
                        .iter()
                        .map(|s| s.name)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                return Some(PolicyDecision {
                    mode: self.block_mode,
                    matched_domain: Some(d),
                    reason,
                });
            }
        }

        None
    }
}

// ─── Hot-reloadable policy engine ───

pub struct SentinelPolicyEngine {
    snapshot: ArcSwap<PolicySnapshot>,
}

impl SentinelPolicyEngine {
    pub fn new(snapshot: PolicySnapshot) -> Self {
        Self {
            snapshot: ArcSwap::from_pointee(snapshot),
        }
    }

    pub fn from_blocklist_text(block_mode: BlockMode, blocklist: &str) -> Self {
        let domains: Vec<String> = blocklist
            .lines()
            .filter_map(parse_blocklist_line)
            .collect();
        let snap = PolicySnapshot {
            block_mode,
            block_index: DomainBlockIndex::from_domains(domains),
            allow_index: DomainBlockIndex::from_domains(Vec::new()),
            exact_allow: HashSet::new(),
            exact_deny: HashSet::new(),
            regex_index: RegexIndex::default(),
            heuristics_enabled: true,
        };
        Self::new(snap)
    }

    pub fn from_blocklist_file<P: AsRef<Path>>(
        block_mode: BlockMode,
        path: P,
    ) -> anyhow::Result<Self> {
        let file = std::fs::File::open(path.as_ref())?;
        let reader = std::io::BufReader::new(file);
        let mut domains = Vec::new();
        for line in reader.lines() {
            if let Some(d) = parse_blocklist_line(&line?) {
                domains.push(d);
            }
        }
        let snap = PolicySnapshot {
            block_mode,
            block_index: DomainBlockIndex::from_domains(domains),
            allow_index: DomainBlockIndex::from_domains(Vec::new()),
            exact_allow: HashSet::new(),
            exact_deny: HashSet::new(),
            regex_index: RegexIndex::default(),
            heuristics_enabled: true,
        };
        Ok(Self::new(snap))
    }

    pub fn with_allowlist(self, allowlist: Vec<String>) -> Self {
        let mut snap = (*self.snapshot.load_full()).clone();
        snap.exact_allow = allowlist
            .into_iter()
            .map(|d| normalize_domain(&d))
            .collect();
        self.snapshot.store(Arc::new(snap));
        self
    }

    pub fn swap_snapshot(&self, new_snapshot: PolicySnapshot) {
        self.snapshot.store(Arc::new(new_snapshot));
    }

    pub fn load_snapshot(&self) -> Arc<PolicySnapshot> {
        self.snapshot.load_full()
    }

    pub fn domain_count(&self) -> usize {
        let snap = self.snapshot.load();
        snap.block_index.domain_count() + snap.allow_index.domain_count()
    }
}

impl PolicyEngine for SentinelPolicyEngine {
    fn decide(&self, query_domain: &str, cname_chain: &[String]) -> PolicyDecision {
        let snap = self.snapshot.load();

        if let Some(decision) = snap.check_domain(query_domain) {
            if decision.mode != BlockMode::Allow {
                return decision;
            }
            if decision.reason.contains("allow") {
                return decision;
            }
        }

        for hop in cname_chain {
            if let Some(decision) = snap.check_domain(hop) {
                if decision.mode != BlockMode::Allow {
                    return decision;
                }
            }
        }

        PolicyDecision {
            mode: BlockMode::Allow,
            matched_domain: None,
            reason: "no block match".to_string(),
        }
    }

    fn check_domain_quick(&self, domain: &str) -> Option<PolicyDecision> {
        let snap = self.snapshot.load();
        snap.check_domain(domain)
    }
}

// ─── Gravity fetch + build ───

pub struct GravityResult {
    pub block_domains: Vec<String>,
    pub allow_domains: Vec<String>,
    pub per_list_counts: Vec<(i64, i64, String)>, // (list_id, count, status)
}

pub async fn gravity_pull(
    lists: &[sentinel_types::Adlist],
) -> GravityResult {
    let mut block_domains = Vec::new();
    let mut allow_domains = Vec::new();
    let mut per_list_counts = Vec::new();

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "failed to create HTTP client for gravity pull");
            return GravityResult {
                block_domains,
                allow_domains,
                per_list_counts,
            };
        }
    };

    for list in lists {
        if !list.enabled {
            continue;
        }

        let result = client.get(&list.url).send().await;
        match result {
            Ok(resp) => {
                if !resp.status().is_success() {
                    let status = format!("HTTP {}", resp.status());
                    per_list_counts.push((list.id, 0, status));
                    continue;
                }
                let body = match resp.text().await {
                    Ok(b) => b,
                    Err(e) => {
                        per_list_counts.push((list.id, 0, format!("read error: {e}")));
                        continue;
                    }
                };

                let mut count: i64 = 0;
                for line in body.lines() {
                    if let Some(domain) = parse_blocklist_line(line) {
                        count += 1;
                        match list.kind {
                            AdlistKind::Block => block_domains.push(domain),
                            AdlistKind::Allow => allow_domains.push(domain),
                        }
                    }
                }
                per_list_counts.push((list.id, count, "ok".to_string()));
            }
            Err(e) => {
                per_list_counts.push((list.id, 0, format!("fetch error: {e}")));
            }
        }
    }

    GravityResult {
        block_domains,
        allow_domains,
        per_list_counts,
    }
}

pub fn build_snapshot(
    block_mode: BlockMode,
    gravity_block: Vec<String>,
    gravity_allow: Vec<String>,
    domain_rules: &[DomainRule],
) -> PolicySnapshot {
    build_snapshot_with_heuristics(block_mode, gravity_block, gravity_allow, domain_rules, true)
}

pub fn build_snapshot_with_heuristics(
    block_mode: BlockMode,
    gravity_block: Vec<String>,
    gravity_allow: Vec<String>,
    domain_rules: &[DomainRule],
    heuristics_enabled: bool,
) -> PolicySnapshot {
    let mut exact_allow = HashSet::new();
    let mut exact_deny = HashSet::new();

    for rule in domain_rules {
        if !rule.enabled {
            continue;
        }
        match rule.kind {
            DomainRuleKind::ExactAllow => {
                exact_allow.insert(normalize_domain(&rule.value));
            }
            DomainRuleKind::ExactDeny => {
                exact_deny.insert(normalize_domain(&rule.value));
            }
            _ => {}
        }
    }

    PolicySnapshot {
        block_mode,
        block_index: DomainBlockIndex::from_domains(gravity_block),
        allow_index: DomainBlockIndex::from_domains(gravity_allow),
        exact_allow,
        exact_deny,
        regex_index: RegexIndex::from_rules(domain_rules),
        heuristics_enabled,
    }
}

// ─── Blocklist parser ───

pub fn parse_blocklist_line(line: &str) -> Option<String> {
    let raw = line.trim();
    if raw.is_empty() || raw.starts_with('#') || raw.starts_with('!') {
        return None;
    }

    // Adblock-style basic domain rules: ||domain.com^
    if let Some(stripped) = raw.strip_prefix("||") {
        let domain = stripped.trim_end_matches('^').trim();
        if !domain.is_empty() && !domain.contains('/') {
            return Some(normalize_domain(domain));
        }
        return None;
    }

    let domain_part = if let Some(rest) = raw.strip_prefix("0.0.0.0 ") {
        rest
    } else if let Some(rest) = raw.strip_prefix("127.0.0.1 ") {
        rest
    } else {
        raw
    };

    let cleaned = domain_part.split('#').next().unwrap_or(domain_part).trim();
    if cleaned.is_empty() || cleaned.contains(' ') || cleaned.starts_with("::") {
        return None;
    }

    Some(normalize_domain(cleaned))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hosts_style_entry() {
        let domain = parse_blocklist_line("0.0.0.0 ads.example.com").expect("must parse");
        assert_eq!(domain, "ads.example.com");
    }

    #[test]
    fn parses_hosts_with_comment() {
        let domain =
            parse_blocklist_line("0.0.0.0 ads.example.com # ad server").expect("must parse");
        assert_eq!(domain, "ads.example.com");
    }

    #[test]
    fn parses_adblock_style() {
        let domain = parse_blocklist_line("||tracker.example.com^").expect("must parse");
        assert_eq!(domain, "tracker.example.com");
    }

    #[test]
    fn blocks_when_any_cname_hop_matches() {
        let engine = SentinelPolicyEngine::from_blocklist_text(
            BlockMode::Nxdomain,
            "tracker-01.ads-giant.com",
        );
        let decision = engine.decide(
            "d.example.com",
            &[
                "alias.tracking-v2.net".to_string(),
                "tracker-01.ads-giant.com".to_string(),
            ],
        );
        assert_eq!(decision.mode, BlockMode::Nxdomain);
        assert_eq!(
            decision.matched_domain,
            Some("tracker-01.ads-giant.com".to_string())
        );
    }

    #[test]
    fn allows_when_no_hop_matches() {
        let engine =
            SentinelPolicyEngine::from_blocklist_text(BlockMode::Nxdomain, "ads.example.com");
        let decision = engine.decide(
            "legit.example.com",
            &["edge.partner.net".to_string(), "cdn.partner.net".to_string()],
        );
        assert_eq!(decision.mode, BlockMode::Allow);
        assert!(decision.matched_domain.is_none());
    }

    #[test]
    fn wildcard_blocks_subdomains() {
        let engine =
            SentinelPolicyEngine::from_blocklist_text(BlockMode::Nxdomain, "*.evil.com");
        let decision = engine.decide("sub.evil.com", &[]);
        assert_eq!(decision.mode, BlockMode::Nxdomain);

        let decision2 = engine.decide("deep.sub.evil.com", &[]);
        assert_eq!(decision2.mode, BlockMode::Nxdomain);

        let decision3 = engine.decide("evil.com", &[]);
        assert_eq!(decision3.mode, BlockMode::Nxdomain);

        let decision4 = engine.decide("notevil.com", &[]);
        assert_eq!(decision4.mode, BlockMode::Allow);
    }

    #[test]
    fn allowlist_takes_precedence() {
        let engine = SentinelPolicyEngine::from_blocklist_text(
            BlockMode::Nxdomain,
            "ads.example.com\ntracker.example.com",
        )
        .with_allowlist(vec!["ads.example.com".to_string()]);

        let allowed = engine.decide("ads.example.com", &[]);
        assert_eq!(allowed.mode, BlockMode::Allow);

        let blocked = engine.decide("tracker.example.com", &[]);
        assert_eq!(blocked.mode, BlockMode::Nxdomain);
    }

    #[test]
    fn regex_deny_blocks() {
        let rules = vec![DomainRule {
            id: 1,
            kind: DomainRuleKind::RegexDeny,
            value: r"^ads\d+\.example\.com$".to_string(),
            enabled: true,
            comment: None,
        }];
        let snap = build_snapshot(BlockMode::Nxdomain, Vec::new(), Vec::new(), &rules);
        let engine = SentinelPolicyEngine::new(snap);
        let d1 = engine.decide("ads123.example.com", &[]);
        assert_eq!(d1.mode, BlockMode::Nxdomain);
        assert!(d1.reason.contains("regex deny"));

        let d2 = engine.decide("safe.example.com", &[]);
        assert_eq!(d2.mode, BlockMode::Allow);
    }

    #[test]
    fn regex_allow_overrides_gravity() {
        let rules = vec![DomainRule {
            id: 1,
            kind: DomainRuleKind::RegexAllow,
            value: r"^cdn\..*\.com$".to_string(),
            enabled: true,
            comment: None,
        }];
        let snap = build_snapshot(
            BlockMode::Nxdomain,
            vec!["cdn.tracker.com".to_string()],
            Vec::new(),
            &rules,
        );
        let engine = SentinelPolicyEngine::new(snap);
        let d = engine.decide("cdn.tracker.com", &[]);
        assert_eq!(d.mode, BlockMode::Allow);
    }

    #[test]
    fn exact_deny_overrides_gravity_allow() {
        let rules = vec![DomainRule {
            id: 1,
            kind: DomainRuleKind::ExactDeny,
            value: "bad.example.com".to_string(),
            enabled: true,
            comment: None,
        }];
        let snap = build_snapshot(
            BlockMode::Nxdomain,
            Vec::new(),
            vec!["bad.example.com".to_string()],
            &rules,
        );
        let engine = SentinelPolicyEngine::new(snap);
        let d = engine.decide("bad.example.com", &[]);
        assert_eq!(d.mode, BlockMode::Nxdomain);
    }

    #[test]
    fn hot_swap_works() {
        let engine = SentinelPolicyEngine::from_blocklist_text(
            BlockMode::Nxdomain,
            "ads.example.com",
        );
        let d1 = engine.decide("ads.example.com", &[]);
        assert_eq!(d1.mode, BlockMode::Nxdomain);

        engine.swap_snapshot(PolicySnapshot::empty(BlockMode::Nxdomain));
        let d2 = engine.decide("ads.example.com", &[]);
        assert_eq!(d2.mode, BlockMode::Allow);
    }

    #[test]
    fn bloom_sized_proportional_to_input() {
        let small = DomainBlockIndex::from_domains(vec!["a.com".to_string()]);
        let big = DomainBlockIndex::from_domains(
            (0..5000).map(|i| format!("d{i}.example.com")).collect(),
        );
        assert!(big.domain_count() > small.domain_count());
    }
}
