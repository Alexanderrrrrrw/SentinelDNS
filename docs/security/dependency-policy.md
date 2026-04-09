# Dependency Verification Policy

Sentinel DNS uses a strict allowlist approach for dependencies:

- Only add packages from official registries (`crates.io`, `npm`) with active maintenance history.
- Pin explicit versions; no `*` ranges.
- Require purpose justification in PR description for each new dependency.
- Prefer standard library and in-house code for simple utilities.

## Rust Security Checks

- `cargo audit`
- `cargo deny check`

## Frontend Security Checks

- `pnpm audit --prod`
- Lockfile review in CI.
