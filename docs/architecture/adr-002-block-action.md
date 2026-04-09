# ADR 002: Block Action Semantics

## Status
Accepted

## Context
Networks have different compatibility needs for blocked DNS responses.

## Decision
Policy engine supports three modes:

- `allow`: request passes.
- `nxdomain`: return not found.
- `null_ip`: return 0.0.0.0 or ::.

Default production mode is `nxdomain`.

## Consequences
- Flexible compatibility for constrained environments.
- Dashboard and API can switch behavior without resolver restarts.
