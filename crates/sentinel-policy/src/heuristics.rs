//! Heuristic domain scoring engine.
//!
//! Unlike Pi-hole (which relies entirely on static blocklists), Sentinel can
//! detect suspicious domains it has never seen before by analysing structural
//! signals in the domain name itself.
//!
//! Every signal produces a weighted score. When the cumulative score exceeds
//! `BLOCK_THRESHOLD`, the domain is flagged as suspicious.
//!
//! Signals:
//!  1. Shannon entropy of the longest label — DGA domains are effectively
//!     random and cluster around 3.5–4.0 bits/char.
//!  2. Consonant-to-vowel ratio — natural language labels have a predictable
//!     ratio; random strings deviate.
//!  3. Numeric density — many ad/tracking subdomains pack hex or decimal IDs.
//!  4. Label count — deeply nested subdomains are often tracking infrastructure.
//!  5. Total domain length — unusually long FQDNs are suspicious.
//!  6. Suspicious TLD — `.xyz`, `.top`, `.buzz`, `.tk`, `.gq`, `.ml`, `.ga`,
//!     `.cf` are disproportionately used for malware/spam.
//!  7. Digit-only label — labels that are entirely numeric are often ephemeral.
//!  8. Hexadecimal label — labels that look like hex hashes (32+ chars matching
//!     `[0-9a-f]+`) are extremely suspicious.

use std::collections::HashSet;
use std::sync::LazyLock;

/// Score ≥ this is considered suspicious.
pub const BLOCK_THRESHOLD: f64 = 70.0;

/// Score ≥ this is "warn but pass" territory — for future dashboard display.
pub const WARN_THRESHOLD: f64 = 45.0;

static SUSPICIOUS_TLDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "xyz", "top", "buzz", "tk", "gq", "ml", "ga", "cf", "pw", "cc",
        "club", "work", "date", "bid", "stream", "download", "racing",
        "win", "review", "accountant", "cricket", "science", "party",
        "faith", "loan", "click", "link", "trade", "icu", "cam", "rest",
        "monster", "cfd", "sbs", "quest",
    ]
    .into_iter()
    .collect()
});

static SAFE_DOMAINS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "google.com",
        "googleapis.com",
        "gstatic.com",
        "youtube.com",
        "microsoft.com",
        "windows.com",
        "azure.com",
        "office.com",
        "apple.com",
        "icloud.com",
        "amazonaws.com",
        "cloudfront.net",
        "github.com",
        "githubusercontent.com",
        "gitlab.com",
        "cloudflare.com",
        "wikipedia.org",
        "mozilla.org",
        "mozilla.com",
        "firefox.com",
        "facebook.com",
        "fbcdn.net",
        "instagram.com",
        "twitter.com",
        "linkedin.com",
        "reddit.com",
        "redditstatic.com",
        "redditmedia.com",
        "whatsapp.com",
        "netflix.com",
        "nflxvideo.net",
        "spotify.com",
        "scdn.co",
        "twitch.tv",
        "twitchcdn.net",
        "akamaiedge.net",
        "akamai.net",
        "akamaitechnologies.com",
        "fastly.net",
        "stackexchange.com",
        "stackoverflow.com",
        "docker.com",
        "docker.io",
        "npmjs.org",
        "npmjs.com",
        "debian.org",
        "ubuntu.com",
        "centos.org",
        "archlinux.org",
        "slack.com",
        "discord.com",
        "discord.gg",
        "zoom.us",
        "aaplimg.com",
        "mzstatic.com",
    ]
    .into_iter()
    .collect()
});

#[derive(Debug, Clone)]
pub struct HeuristicResult {
    pub score: f64,
    pub signals: Vec<HeuristicSignal>,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Clean,
    Warn,
    Suspicious,
}

impl Verdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::Clean => "clean",
            Verdict::Warn => "warn",
            Verdict::Suspicious => "suspicious",
        }
    }
}

#[derive(Debug, Clone)]
pub struct HeuristicSignal {
    pub name: &'static str,
    pub weight: f64,
    pub detail: String,
}

/// Run all heuristics on a normalized domain and produce a combined score.
pub fn score_domain(domain: &str) -> HeuristicResult {
    let lower = domain.to_ascii_lowercase().trim_end_matches('.').to_string();
    let labels: Vec<&str> = lower.split('.').collect();

    if labels.len() < 2 {
        return clean_result();
    }

    let base = base_domain(&labels);
    if SAFE_DOMAINS.contains(base.as_str()) {
        return clean_result();
    }

    let mut signals = Vec::new();

    // 1. Entropy of the longest non-TLD label
    if let Some(longest) = labels.iter().take(labels.len().saturating_sub(1)).max_by_key(|l| l.len()) {
        if longest.len() >= 4 {
            let ent = shannon_entropy(longest);
            if ent > 3.8 {
                signals.push(HeuristicSignal {
                    name: "high_entropy",
                    weight: 25.0 * ((ent - 3.8) / 0.7).min(1.0),
                    detail: format!("entropy={ent:.2} on label \"{longest}\""),
                });
            } else if ent > 3.4 {
                signals.push(HeuristicSignal {
                    name: "elevated_entropy",
                    weight: 10.0 * ((ent - 3.4) / 0.4).min(1.0),
                    detail: format!("entropy={ent:.2} on label \"{longest}\""),
                });
            }
        }
    }

    // 2. Consonant-to-vowel ratio on longest non-TLD label
    if let Some(longest) = labels.iter().take(labels.len().saturating_sub(1)).max_by_key(|l| l.len()) {
        if longest.len() >= 5 {
            let (vowels, consonants) = count_vowels_consonants(longest);
            if vowels == 0 && consonants > 4 {
                signals.push(HeuristicSignal {
                    name: "no_vowels",
                    weight: 20.0,
                    detail: format!("label \"{longest}\" has zero vowels"),
                });
            } else if vowels > 0 {
                let ratio = consonants as f64 / vowels as f64;
                if ratio > 5.0 {
                    signals.push(HeuristicSignal {
                        name: "extreme_consonant_ratio",
                        weight: 15.0,
                        detail: format!("C:V ratio={ratio:.1} on \"{longest}\""),
                    });
                } else if ratio > 3.5 {
                    signals.push(HeuristicSignal {
                        name: "high_consonant_ratio",
                        weight: 8.0,
                        detail: format!("C:V ratio={ratio:.1} on \"{longest}\""),
                    });
                }
            }
        }
    }

    // 3. Numeric density
    let total_chars: usize = labels.iter().take(labels.len().saturating_sub(1)).map(|l| l.len()).sum();
    if total_chars > 0 {
        let digit_chars: usize = labels
            .iter()
            .take(labels.len().saturating_sub(1))
            .flat_map(|l| l.chars())
            .filter(|c| c.is_ascii_digit())
            .count();
        let density = digit_chars as f64 / total_chars as f64;
        if density > 0.5 {
            signals.push(HeuristicSignal {
                name: "high_numeric_density",
                weight: 15.0 * ((density - 0.5) / 0.3).min(1.0),
                detail: format!("digit density={density:.2}"),
            });
        }
    }

    // 4. Label count (depth)
    if labels.len() > 5 {
        signals.push(HeuristicSignal {
            name: "deep_subdomain",
            weight: 10.0 + (labels.len() as f64 - 5.0) * 3.0,
            detail: format!("{} labels deep", labels.len()),
        });
    }

    // 5. Total length
    if lower.len() > 60 {
        signals.push(HeuristicSignal {
            name: "very_long_domain",
            weight: 10.0 + (lower.len() as f64 - 60.0) * 0.3,
            detail: format!("{} chars total", lower.len()),
        });
    }

    // 6. Suspicious TLD
    if let Some(tld) = labels.last() {
        if SUSPICIOUS_TLDS.contains(tld) {
            signals.push(HeuristicSignal {
                name: "suspicious_tld",
                weight: 20.0,
                detail: format!("TLD .{tld}"),
            });
        }
    }

    // 7. Digit-only labels (excluding TLD)
    let digit_labels: Vec<&&str> = labels
        .iter()
        .take(labels.len().saturating_sub(1))
        .filter(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_digit()))
        .collect();
    if !digit_labels.is_empty() {
        signals.push(HeuristicSignal {
            name: "numeric_label",
            weight: 8.0 * digit_labels.len() as f64,
            detail: format!("{} all-numeric labels", digit_labels.len()),
        });
    }

    // 8. Hexadecimal-looking label (≥ 16 chars matching [0-9a-f])
    for label in labels.iter().take(labels.len().saturating_sub(1)) {
        if label.len() >= 16 && label.chars().all(|c| c.is_ascii_hexdigit()) {
            let has_alpha = label.chars().any(|c| c.is_ascii_alphabetic());
            let has_digit = label.chars().any(|c| c.is_ascii_digit());
            if has_alpha && has_digit {
                let weight = if label.len() >= 32 { 35.0 } else { 25.0 };
                signals.push(HeuristicSignal {
                    name: "hex_hash_label",
                    weight,
                    detail: format!("label \"{label}\" looks like a hex hash ({} chars)", label.len()),
                });
                break;
            }
        }
    }

    // 9. Hyphen-heavy labels (common in DGA)
    for label in labels.iter().take(labels.len().saturating_sub(1)) {
        if label.len() >= 8 {
            let hyphens = label.chars().filter(|&c| c == '-').count();
            if hyphens as f64 / label.len() as f64 > 0.3 {
                signals.push(HeuristicSignal {
                    name: "hyphen_heavy",
                    weight: 12.0,
                    detail: format!("label \"{label}\" is {:.0}% hyphens", hyphens as f64 / label.len() as f64 * 100.0),
                });
                break;
            }
        }
    }

    let score: f64 = signals.iter().map(|s| s.weight).sum();
    let verdict = if score >= BLOCK_THRESHOLD {
        Verdict::Suspicious
    } else if score >= WARN_THRESHOLD {
        Verdict::Warn
    } else {
        Verdict::Clean
    };

    HeuristicResult {
        score,
        signals,
        verdict,
    }
}

fn clean_result() -> HeuristicResult {
    HeuristicResult {
        score: 0.0,
        signals: Vec::new(),
        verdict: Verdict::Clean,
    }
}

fn shannon_entropy(s: &str) -> f64 {
    let len = s.len() as f64;
    if len == 0.0 {
        return 0.0;
    }
    let mut freq = [0u32; 256];
    for b in s.bytes() {
        freq[b as usize] += 1;
    }
    let mut entropy = 0.0f64;
    for &count in &freq {
        if count > 0 {
            let p = count as f64 / len;
            entropy -= p * p.log2();
        }
    }
    entropy
}

fn count_vowels_consonants(s: &str) -> (usize, usize) {
    let mut vowels = 0;
    let mut consonants = 0;
    for c in s.chars() {
        if c.is_ascii_alphabetic() {
            if "aeiou".contains(c.to_ascii_lowercase()) {
                vowels += 1;
            } else {
                consonants += 1;
            }
        }
    }
    (vowels, consonants)
}

fn base_domain(labels: &[&str]) -> String {
    if labels.len() >= 2 {
        format!("{}.{}", labels[labels.len() - 2], labels[labels.len() - 1])
    } else {
        labels.join(".")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_safe_domain_is_clean() {
        let r = score_domain("www.google.com");
        assert_eq!(r.verdict, Verdict::Clean);
        assert_eq!(r.score, 0.0);
    }

    #[test]
    fn normal_domain_scores_low() {
        let r = score_domain("docs.example.com");
        assert!(r.score < WARN_THRESHOLD, "score={}", r.score);
    }

    #[test]
    fn dga_looking_domain_scores_high() {
        // DGA domain: consonant-heavy random labels on a suspicious TLD
        // Stacks: no_vowels + high_entropy + suspicious_tld + deep_subdomain
        let r = score_domain("xkqz9m2np5v8w1rjbg7t3cd.kz8wp3.nx7b.rm4d.qc2.tk");
        assert!(
            r.score >= BLOCK_THRESHOLD,
            "expected suspicious, score={}, signals={:?}",
            r.score,
            r.signals.iter().map(|s| (s.name, s.weight)).collect::<Vec<_>>()
        );
        assert_eq!(r.verdict, Verdict::Suspicious);
    }

    #[test]
    fn single_label_dga_is_at_least_warned() {
        let r = score_domain("qxz7k9m2n5p3v8w1.xyz");
        assert!(
            r.score >= WARN_THRESHOLD,
            "expected at least warn, score={}",
            r.score
        );
    }

    #[test]
    fn hex_hash_subdomain_flagged() {
        let r = score_domain("a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6.tracker.com");
        assert!(r.signals.iter().any(|s| s.name == "hex_hash_label"), "signals={:?}", r.signals);
    }

    #[test]
    fn deeply_nested_domain_flagged() {
        let r = score_domain("a.b.c.d.e.f.tracking.com");
        assert!(r.signals.iter().any(|s| s.name == "deep_subdomain"));
    }

    #[test]
    fn suspicious_tld_adds_weight() {
        let r = score_domain("something.tk");
        assert!(r.signals.iter().any(|s| s.name == "suspicious_tld"));
    }

    #[test]
    fn regular_com_domain_not_penalized_for_tld() {
        let r = score_domain("mysite.com");
        assert!(!r.signals.iter().any(|s| s.name == "suspicious_tld"));
    }

    #[test]
    fn entropy_signal_on_random_string() {
        let e = shannon_entropy("qxz7k9m2n5p3v8w1");
        assert!(e > 3.5, "entropy should be high for random: {e}");
    }

    #[test]
    fn entropy_signal_on_normal_word() {
        let e = shannon_entropy("google");
        assert!(e < 3.0, "entropy should be low for normal word: {e}");
    }
}
