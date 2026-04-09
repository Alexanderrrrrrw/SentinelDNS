<p align="center">
  <img src="https://img.shields.io/badge/built_with-Rust-dea584?style=flat-square&logo=rust" alt="Rust" />
  <img src="https://img.shields.io/badge/dashboard-Next.js_16-black?style=flat-square&logo=next.js" alt="Next.js" />
  <img src="https://img.shields.io/github/license/Alexanderrrrrrw/SentinelDNS?style=flat-square" alt="License" />
  <img src="https://img.shields.io/github/actions/workflow/status/Alexanderrrrrrw/SentinelDNS/ci.yml?style=flat-square&label=CI" alt="CI" />
  <img src="https://img.shields.io/badge/platform-Raspberry_Pi-c51a4a?style=flat-square&logo=raspberrypi" alt="Raspberry Pi" />
</p>

# Sentinel DNS

A DNS filter that actually respects your hardware. Built in Rust for Raspberry Pi.

Sentinel blocks ads, trackers, malware, and phishing at the network level — like Pi-hole, but it won't destroy your SD card, it catches threats Pi-hole can't see, and its dashboard was designed this decade.

## One-Line Install

```bash
curl -sSL https://raw.githubusercontent.com/Alexanderrrrrrw/SentinelDNS/main/deploy/install.sh | sudo bash
```

That's it. No compilation — pre-built Docker images are pulled from GHCR in ~60 seconds. The script installs Docker if needed, disables `systemd-resolved` with a self-healing fallback, generates an admin token, tunes the OS for SD card longevity, sets up iptables DNS interception, and starts everything. If anything goes wrong, a cron job automatically restores your system resolver within 60 seconds so you never brick a headless Pi.

> **Developer mode:** Pass `--build-from-source` to compile from the repo instead of pulling images.

`default.fst` is a **bootstrap accelerator**, not the long-term source of truth. Sentinel triggers gravity syncs after first boot so block data is refreshed from live upstream lists.

## Raspberry Pi Setup (First 5 Minutes)

### 1) Install on the Pi

```bash
curl -sSL https://raw.githubusercontent.com/Alexanderrrrrrw/SentinelDNS/main/deploy/install.sh | sudo bash
```

### 2) Verify services are up

```bash
cd /opt/sentinel-dns/deploy
docker compose ps
docker compose logs --tail=100
```

### 3) Quick DNS sanity checks

```bash
nslookup example.com 127.0.0.1
nslookup doubleclick.net 127.0.0.1
```

Expected: `example.com` resolves, `doubleclick.net` is blocked (NXDOMAIN/null response based on mode).

### 4) Open dashboard

Visit:

```text
http://<pi-ip>:3000
```

### 5) Roll out safely

Set **one test device** DNS to `<pi-ip>` first. Validate browsing + logs, then switch router DHCP DNS for whole network.

### Rollback (if you need internet back instantly)

```bash
cd /opt/sentinel-dns/deploy
docker compose down
sudo systemctl enable --now systemd-resolved
```

---

## Why not just use Pi-hole?

Pi-hole is great for what it was built to do in 2015. But it has real problems that nobody talks about:

- It writes to disk on **every single DNS query**, which kills SD cards in months
- It can't catch tracker domains it hasn't seen before
- Its CNAME uncloaking is partial at best
- Chrome and Android can bypass it entirely via hardcoded DoH
- The dashboard looks like it was styled with Bootstrap 2

Sentinel fixes all of that.

## Non-Goals (What Sentinel is not)

Before opening an issue, quick reality check:

- Sentinel is **not** a VPN service
- Sentinel is **not** a full SSL/TLS interception proxy
- Sentinel is **not** a parental control suite with deep content inspection
- Sentinel is **not** a router firmware replacement
- Sentinel is **not** an enterprise SIEM or SOC platform

It is a fast, local DNS control plane for blocking junk at the resolver layer.

## Key Features

**🛡 4-Layer Detection Pipeline**
Static blocklists → CNAME chain walking → regex rules → heuristic scoring. If a domain passes your blocklists, Sentinel still catches it by analyzing its structure for DGA patterns, tracking infrastructure, and suspicious entropy.

**💾 SD Card Protection**
RAM-first logging pipeline. DNS queries buffer in a 100k-entry circular buffer and flush to DuckDB every 15 minutes instead of hammering the disk on every query. That's a 900x reduction in write cycles. Your SD card will outlive the Pi.

**🔗 Full CNAME Uncloaking**
Walks the entire CNAME chain and checks the blocklist at every hop. If `innocent.example.com` CNAMEs to `tracker.adnetwork.com`, Sentinel blocks it immediately without wasting upstream lookups on the rest of the chain.

**⌨️ Command Palette (Ctrl+K)**
Type `block ads.com` or `allow youtube.com` directly from the dashboard. Navigate anywhere, toggle heuristics, trigger gravity updates — all without touching a menu.

**📡 mDNS Client Discovery**
Sees `Living Room Apple TV` instead of `192.168.1.45`. Background mDNS listener identifies Chromecasts, printers, phones, and computers automatically.

**🔒 Network Hardening**
iptables rules redirect all port-53 traffic through Sentinel (catches devices with hardcoded DNS like 8.8.8.8) and block outbound DoH to Google, Cloudflare, and Quad9 so browsers can't sneak around your filter.

**🩹 Self-Healing Installer**
A cron-based safety net monitors port 53. If Sentinel fails to start, `systemd-resolved` is automatically restored within 60 seconds. You will never lose internet access on a headless Pi.

**📊 Live Tail (SSE)**
Real-time query stream via Server-Sent Events. Blocked domains shake on arrival. No polling, no WebSocket complexity, no wasted CPU cycles.

---

## Sentinel vs. Pi-hole

| | Pi-hole | Sentinel DNS |
|---|---|---|
| **Install time** | ~2 min | ~2 min (pre-built images, no compilation) |
| **Blocklists on install** | 1 (StevenBlack) | 9 curated lists |
| **CNAME uncloaking** | Partial | Full — short-circuits at first blocked hop |
| **Heuristic detection** | None | 9-signal scoring engine (DGA, entropy, hex hashes) |
| **Encrypted upstream DNS** | Requires separate proxy | Built-in DoT/DoH |
| **DNS-over-TCP** | No | Yes |
| **Blocklist reload** | Restarts FTL | Zero-downtime atomic swap |
| **Log pipeline** | Disk write per query | RAM-first, flushes every 15 min |
| **SD card lifespan** | Months | Years |
| **Device discovery** | Manual | Automatic mDNS |
| **Dashboard** | PHP + lighttpd | Next.js + glassmorphic bento grid |
| **Command palette** | None | Ctrl+K with inline domain actions |
| **Live query tail** | Polling | SSE stream |
| **DoH bypass protection** | None | iptables blocks known DoH providers |
| **Self-healing install** | None | Auto-restores system DNS if service fails |
| **Memory** | ~100MB+ | ~20-40MB |
| **Language** | C + PHP | Rust + TypeScript |

---

## Screenshots

<!-- Replace these with actual screenshots before launch -->

### Bento Dashboard
> Glassmorphic metric cards, top domains with proportional bars, CNAME chain inspector — all on one page.

`[screenshot: apps/dashboard — main page]`

### Live Tail
> Real-time DNS query stream. Blocked entries shake. Allowed entries flash green. Discovered client names shown inline.

`[screenshot: apps/dashboard — query log with live tail active]`

### Command Palette
> Ctrl+K to block a domain, whitelist a domain, navigate anywhere, or trigger a gravity update. No menus needed.

`[screenshot: apps/dashboard — command palette open]`

### Heuristic Scanner
> Score any domain against 9 structural signals. See exactly which heuristics fired and why.

`[screenshot: apps/dashboard — heuristics page with a scored domain]`

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│  Clients (phones, laptops, smart TVs)                   │
│      DNS query ──► port 53                              │
└───────────┬─────────────────────────────────────────────┘
            │
            ▼
┌─────────────────────────────────────────────────────────┐
│  sentinel-resolver                                      │
│  ┌──────────┐  ┌──────────────┐  ┌───────────────────┐  │
│  │ UDP/TCP  │─►│ DashMap      │─►│ walk_cname_chain  │  │
│  │ listener │  │ cache (50k)  │  │ (short-circuits)  │  │
│  └──────────┘  └──────────────┘  └───────┬───────────┘  │
│                                          │              │
│  sentinel-policy ◄───────────────────────┘              │
│  ┌─────────────────────────────────────────────────┐    │
│  │ Bloom+HashSet → Wildcards → Regex → Heuristics  │    │
│  └─────────────────────────────────────────────────┘    │
│                                                         │
│  sentinel-storage (RAM-first pipeline)                  │
│  ┌───────────────┐     ┌──────────────────────┐         │
│  │ RamLogRing    │────►│ Checkpoint (15 min)   │         │
│  │ (VecDeque)    │     │ → DuckDB (niced I/O)  │         │
│  └───────┬───────┘     └──────────────────────┘         │
│          │ broadcast                                    │
│          ▼                                              │
│  sentinel-control-plane (Axum)                          │
│  ┌────────────────────────────────────────────────┐     │
│  │ REST API · SSE live tail · Prometheus metrics  │     │
│  └────────────────────────────────────────────────┘     │
│                                                         │
│  mDNS listener (224.0.0.251:5353)                       │
│  ┌────────────────────────────────────────────────┐     │
│  │ DashMap<IP → hostname + device type>           │     │
│  └────────────────────────────────────────────────┘     │
└─────────────────────────────────────────────────────────┘
            │
            ▼
┌─────────────────────────────────────────────────────────┐
│  Dashboard (Next.js 16 · standalone)                    │
│  Bento grid · command palette · live tail · 8 pages     │
└─────────────────────────────────────────────────────────┘
```

## How Detection Works

### Layer 1: Static blocklists
Fetches 9 community blocklists on first boot (StevenBlack, OISD, HaGeZi, abuse.ch, Frogeye, BlocklistProject). Loaded into a Bloom filter + HashSet. Supports hosts-file format, domain-per-line, and Adblock `||domain^` syntax. Hot-reloaded via atomic `ArcSwap` — no restarts.

### Layer 2: CNAME chain walking
Resolves the full CNAME chain and checks the blocklist at every hop. If any intermediate CNAME points to a known tracker, the query is blocked immediately — no wasted upstream lookups for the remaining chain.

### Layer 3: Regex rules
User-defined regex patterns compiled into a `RegexSet` for O(1) matching against all patterns simultaneously.

### Layer 4: Heuristic scoring
9 structural signals run on every uncategorized domain:

| Signal | What it catches |
|---|---|
| Shannon entropy | DGA domains (random characters) |
| Vowel/consonant ratio | Machine-generated strings |
| Numeric density | Tracking subdomains with hex/decimal IDs |
| Subdomain depth | Deeply nested tracking infrastructure |
| Domain length | Abnormally long FQDNs |
| Suspicious TLD | .xyz, .tk, .top, .buzz abuse |
| Hex hash labels | C2 beacons and tracking hashes |
| Numeric-only labels | Ephemeral infrastructure |
| Hyphen density | DGA and phishing patterns |

Domains scoring >= 70 are blocked. >= 45 are warned. A safe-domain list prevents false positives on CDNs and major services.

## SD Card Protection

Pi-hole writes to SQLite on every DNS query — about 86,400 writes per day. Sentinel uses a RAM-first pipeline:

```
DNS query → mpsc channel → RamLogRing (100k entries in RAM)
                                │
                                ├── Dashboard reads from here (zero disk I/O)
                                │
                                └── Checkpoint every 15 min → DuckDB
                                    └── niced thread (IOPRIO_CLASS_IDLE)

SIGTERM → emergency flush → drain all RAM to DuckDB (zero data loss)
```

**96 writes/day instead of 86,400.** The `deploy/sd-card-tuning.sh` script also applies `noatime`, tmpfs mounts, disabled swap, and RAM-only journald.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `SENTINEL_DNS_BIND` | `0.0.0.0:5353` | DNS listener bind address |
| `SENTINEL_API_BIND` | `0.0.0.0:8080` | HTTP API bind address |
| `SENTINEL_ADMIN_TOKEN` | *(generated)* | Admin token for protected endpoints |
| `SENTINEL_BLOCK_MODE` | `nxdomain` | `nxdomain` or `null_ip` |
| `SENTINEL_UPSTREAM` | *(system)* | Encrypted upstream: `tls://1.1.1.1:853#cloudflare-dns.com` |
| `SENTINEL_HEURISTICS` | `true` | Enable heuristic domain scoring |
| `SENTINEL_RAM_LOG_CAPACITY` | `100000` | Log entries buffered in RAM |
| `SENTINEL_CHECKPOINT_SECS` | `900` | Flush interval (seconds) |
| `SENTINEL_GRAVITY_INTERVAL_SECS` | `604800` | Blocklist auto-update (7 days) |

Full list in [`deploy/.env.example`](deploy/.env.example).

## Quick Start (Development)

```bash
# Backend
SENTINEL_BLOCKLIST_PATH=fixtures/blocklist.txt cargo run -p sentinel-control-plane

# Dashboard (separate terminal)
cd apps/dashboard && pnpm install && pnpm dev
```

## Repository Layout

```
crates/
  sentinel-types/          Shared types, traits, validation
  sentinel-policy/         Blocklists, Bloom filter, regex, heuristics, gravity
  sentinel-storage/        RAM-first log pipeline, DuckDB checkpoint, SQLite config
  sentinel-resolver/       DNS resolver, CNAME walker, cache, mDNS discovery
  sentinel-control-plane/  Axum API, SSE live tail, Prometheus metrics
apps/
  dashboard/               Next.js 16 dashboard (bento grid, command palette, 8 pages)
deploy/
  install.sh               One-command Pi installer with self-healing
  docker-compose.yml       Production orchestration
  sd-card-tuning.sh        OS-level SD card optimizations
  sentinel-dns.service     Systemd unit with niced I/O
```

## Security

- Admin API requires `x-admin-token` (constant-time comparison)
- Domain inputs validated before processing
- Global rate limiting on `/api/resolve` (600/min)
- `Cargo.lock` committed — reproducible builds
- `cargo-audit` and `cargo-deny` in CI
- Dependabot monitors Rust, npm, Docker, and GitHub Actions dependencies
- Emergency flush on SIGTERM — zero data loss on shutdown

## Support Sentinel

If Sentinel helped you reclaim your network, throw it a GitHub star:

- [⭐ Star Sentinel DNS](https://github.com/Alexanderrrrrrw/SentinelDNS)

## License

MIT
