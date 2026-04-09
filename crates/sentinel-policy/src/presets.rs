//! Curated default blocklists that ship with Sentinel DNS.
//!
//! These are auto-added on first boot if no adlists exist in the database,
//! giving users immediate protection without configuration.
//!
//! We intentionally pick a broader, more aggressive default set than Pi-hole
//! (which only ships StevenBlack). Users can disable any list from the dashboard.

pub struct PresetList {
    pub url: &'static str,
    pub name: &'static str,
    pub kind: PresetKind,
    pub category: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PresetKind {
    Block,
    Allow,
}

pub const PRESET_LISTS: &[PresetList] = &[
    // ── General ads + trackers ──
    PresetList {
        url: "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts",
        name: "StevenBlack Unified",
        kind: PresetKind::Block,
        category: "ads+tracking",
    },
    PresetList {
        url: "https://big.oisd.nl/domainswild",
        name: "OISD Big",
        kind: PresetKind::Block,
        category: "ads+tracking+malware",
    },
    // ── Malware + phishing ──
    PresetList {
        url: "https://urlhaus.abuse.ch/downloads/hostfile/",
        name: "URLhaus Malware",
        kind: PresetKind::Block,
        category: "malware",
    },
    PresetList {
        url: "https://threatfox.abuse.ch/downloads/hostfile/",
        name: "ThreatFox IOC",
        kind: PresetKind::Block,
        category: "malware",
    },
    // ── First-party tracking (CNAME cloakers) ──
    PresetList {
        url: "https://hostfiles.frogeye.fr/firstparty-trackers-hosts.txt",
        name: "Frogeye First-Party Trackers",
        kind: PresetKind::Block,
        category: "cname-tracking",
    },
    // ── HaGeZi Pro (aggressive but tuned) ──
    PresetList {
        url: "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/hosts/pro.txt",
        name: "HaGeZi Pro",
        kind: PresetKind::Block,
        category: "ads+tracking+malware",
    },
    // ── Popup / redirect / scam ads (common on streaming sites) ──
    PresetList {
        url: "https://raw.githubusercontent.com/blocklistproject/Lists/master/ads.txt",
        name: "BlocklistProject Ads",
        kind: PresetKind::Block,
        category: "ads",
    },
    PresetList {
        url: "https://raw.githubusercontent.com/blocklistproject/Lists/master/scam.txt",
        name: "BlocklistProject Scam",
        kind: PresetKind::Block,
        category: "scam+phishing",
    },
    PresetList {
        url: "https://raw.githubusercontent.com/blocklistproject/Lists/master/redirect.txt",
        name: "BlocklistProject Redirects",
        kind: PresetKind::Block,
        category: "redirects+popups",
    },
];

/// Sentinel's own built-in regex deny rules that catch patterns no static
/// list can cover. These target domain-generation algorithms (DGA), tracking
/// pixel subdomains, and other structural red flags.
pub struct BuiltinRegexRule {
    pub pattern: &'static str,
    pub comment: &'static str,
}

pub const BUILTIN_REGEX_RULES: &[BuiltinRegexRule] = &[
    // Tracking pixel / beacon patterns
    BuiltinRegexRule {
        pattern: r"^pixel[-.].*\.(com|net|org)$",
        comment: "tracking pixel subdomains",
    },
    BuiltinRegexRule {
        pattern: r"^(click|track|trk|beacon|log|telemetry|analytics|stats|metric|collect)\d*\.",
        comment: "common tracking/telemetry subdomain prefixes",
    },
    // Ad-serving infrastructure patterns
    BuiltinRegexRule {
        pattern: r"^(ad|ads|adserv|adserver|adtrack|adclick|adimg|adview)\d*\.",
        comment: "ad-serving subdomain patterns",
    },
    // Suspicious long random-looking subdomains (potential DGA / C2)
    BuiltinRegexRule {
        pattern: r"^[a-z0-9]{20,}\.",
        comment: "suspiciously long random subdomain (possible DGA/C2)",
    },
    // Fingerprinting / canvas / WebRTC leak endpoints
    BuiltinRegexRule {
        pattern: r"(fingerprint|canvas-fingerprint|webrtc-leak|device-fingerprint)\.",
        comment: "browser fingerprinting endpoints",
    },
    // Popup / redirect / overlay ad infrastructure
    BuiltinRegexRule {
        pattern: r"^(pop|popup|popunder|popundr|overlay|interstitial)\d*\.",
        comment: "popup/overlay ad subdomains",
    },
    // Coin miners injected via ad networks
    BuiltinRegexRule {
        pattern: r"(coinhive|coinpot|cryptoloot|minero|webmine|crypto-?loot)\.",
        comment: "in-browser cryptocurrency miners",
    },
];
