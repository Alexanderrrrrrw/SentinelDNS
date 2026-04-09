# Sentinel DNS v0.1.0-beta (Draft Release Notes)

Sentinel DNS is now publicly available as a beta release.

## Why Sentinel exists

Pi-hole proved DNS blocking works. Sentinel keeps that model, then fixes the parts that break in real life on Raspberry Pi:

- SD card wear from constant writes
- missed tracker domains that are not yet blocklisted
- weak CNAME handling
- stale dashboard UX

## Highlights

### Rust-first core

- Resolver, policy, storage, and control plane are all implemented in Rust
- memory-safe behavior for a 24/7 always-on network service
- async architecture designed for low-latency DNS responses

### Sentinel vs Pi-hole (practical wins)

- RAM-first logging pipeline with periodic checkpointing to disk
- CNAME chain walk with short-circuit blocking at each hop
- heuristic scoring for suspicious domains not found in static lists
- SSE live tail with real-time query events
- command palette (`Ctrl+K`) for power-user actions

### Self-Healing installer

- one-line setup script
- auto-disables resolver conflicts on port 53
- safety-net fallback restores `systemd-resolved` if Sentinel fails to start
- prints dashboard URL + admin token for first-use onboarding

## Notes on bootstrap index

`default.fst` is shipped as a startup accelerator.  
Gravity sync remains source-of-truth and refreshes block data after first boot.

## Known beta limitations

- Dashboard polish and visualization layers are still being tuned
- Hardware-specific install edge cases may exist on uncommon Pi images
- Not intended for high-scale enterprise deployments in this beta phase

## Upgrade / install

```bash
curl -sSL https://raw.githubusercontent.com/Alexanderrrrrrw/SentinelDNS/main/deploy/install.sh | sudo bash
```

## Feedback

If this saves your network from ad/tracker spam, drop a star and open issues with repro steps.
