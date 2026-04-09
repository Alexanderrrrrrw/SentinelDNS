# ADR 001: CNAME Uncloaking Semantics

## Status
Accepted

## Context
Trackers often hide behind first-party looking domains with multiple CNAME hops.

## Decision
Sentinel walks the CNAME chain recursively with:

- Max depth of 8 hops.
- Loop protection via visited set.
- Blocklist evaluation on every hop.

## Consequences
- Better tracker coverage than first-hop-only matching.
- Slight additional lookup cost, mitigated by cache and bounded depth.
