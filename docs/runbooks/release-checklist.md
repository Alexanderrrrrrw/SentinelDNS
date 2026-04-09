# Sentinel DNS v1 Release Checklist

## Build and Test
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `pnpm --dir apps/dashboard lint`
- `pnpm --dir apps/dashboard test`
- `pnpm --dir apps/dashboard build`

## Security
- `cargo audit`
- `cargo deny check`
- `pnpm --dir apps/dashboard audit --prod`
- Validate no unreviewed dependency additions.

## Operational
- Start with `docker compose -f deploy/docker-compose.yml up`.
- Verify API `/healthz` and `/metrics`.
- Verify dashboard can list devices and inspect CNAME chain.
- Validate SQLite backup and restore procedure.
