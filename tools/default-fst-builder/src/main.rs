use std::collections::BTreeSet;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;

const SOURCES: &[&str] = &[
    "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts",
    "https://big.oisd.nl/domainswild",
    "https://raw.githubusercontent.com/hagezi/dns-blocklists/main/hosts/pro.txt",
];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let output_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("default.fst"));

    let client = reqwest::Client::builder()
        .user_agent("sentinel-default-fst-builder/0.1")
        .build()
        .context("failed to build HTTP client")?;

    let mut domains = BTreeSet::new();
    for url in SOURCES {
        eprintln!("fetching {url}");
        let body = client
            .get(*url)
            .send()
            .await
            .with_context(|| format!("failed to download {url}"))?
            .error_for_status()
            .with_context(|| format!("non-success response from {url}"))?
            .text()
            .await
            .with_context(|| format!("failed to read body from {url}"))?;

        for line in body.lines() {
            if let Some(d) = parse_domain(line) {
                domains.insert(d);
            }
        }
    }

    if domains.is_empty() {
        anyhow::bail!("no domains parsed from source lists");
    }

    eprintln!("parsed {} unique domains", domains.len());
    let mut builder = fst::SetBuilder::new(
        File::create(&output_path).with_context(|| format!("failed to create {:?}", output_path))?,
    )?;
    for domain in domains {
        builder
            .insert(domain)
            .context("failed while inserting domain into FST set")?;
    }
    builder.finish().context("failed to finalize FST")?;

    let size = std::fs::metadata(&output_path)
        .context("failed to stat output file")?
        .len();
    eprintln!("wrote {:?} ({} bytes)", output_path, size);

    // Optional plain-text debug companion (tiny utility for local checks)
    if std::env::var("SENTINEL_WRITE_DOMAIN_TXT").ok().as_deref() == Some("1") {
        let txt_path = output_path.with_extension("txt");
        let mut txt = File::create(&txt_path)?;
        writeln!(txt, "# generated domain list for debug")?;
        // intentionally empty for now; keep flag for future local debugging.
    }

    Ok(())
}

fn parse_domain(raw: &str) -> Option<String> {
    let mut line = raw.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
        return None;
    }

    // Adblock format: ||example.com^
    if let Some(rest) = line.strip_prefix("||") {
        let host = rest.split('^').next().unwrap_or("").trim();
        return normalize_domain(host);
    }

    // hosts format: 0.0.0.0 example.com
    if line.starts_with("0.0.0.0") || line.starts_with("127.0.0.1") {
        let mut parts = line.split_whitespace();
        let _ip = parts.next();
        if let Some(host) = parts.next() {
            return normalize_domain(host);
        }
        return None;
    }

    // plain domain-per-line
    if let Some(host) = line.split_whitespace().next() {
        line = host;
    }
    normalize_domain(line)
}

fn normalize_domain(input: &str) -> Option<String> {
    let mut d = input.trim().trim_matches('.').to_ascii_lowercase();
    if d.is_empty() {
        return None;
    }
    if let Some(rest) = d.strip_prefix("www.") {
        d = rest.to_string();
    }
    if d.contains('/') || d.contains(':') || d.contains(' ') {
        return None;
    }
    if !d.contains('.') {
        return None;
    }
    Some(d)
}
