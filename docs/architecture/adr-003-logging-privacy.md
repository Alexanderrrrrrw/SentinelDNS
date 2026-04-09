# ADR 003: Logging Privacy Defaults

## Status
Accepted

## Context
DNS logs can expose user activity and should have strict defaults.

## Decision
- Store only operational fields required for filtering and troubleshooting.
- Persist client identifier as a device-scoped id, not user identity.
- Keep CNAME chain for uncloaking evidence.
- Expose retention controls in control plane (default 30 days, configurable).

## Consequences
- Enough visibility for operations while limiting personal data exposure.
- Future support for anonymization can be added without schema breakage.
