---
project: openpulsehf
doc: docs/peer-caching-relay.md
status: living
last_updated: 2026-04-23
---

# Peer Caching, Query, and Multi-Hop Relay

## Purpose

This document defines OpenPulseHF requirements and design constraints for peer discovery cache, query semantics, and multi-hop transfer relay.

## Scope

- Discover and cache peer identity and capability records.
- Query local and network-discovered peer metadata.
- Route transfers through multiple relay hops when direct links are unavailable or inefficient.
- Preserve signed-transfer and trust-policy guarantees over relay paths.

Wire-level envelope and message schema details are specified in docs/peer-query-relay-wire.md.

## Peer cache model

### Cache entries

Each peer cache entry should include:

- peer_id
- callsign or operator label
- identity key fingerprint
- supported modes (for example HPX500, HPX2300)
- last_seen timestamp
- trust status
- observed link metrics summary

### Cache lifecycle

- Entries have configurable TTL and aging policy.
- Stale entries are marked but optionally retained for operator audit.
- Conflicting identity records are quarantined until trust policy resolves them.

### Local query model

Queries should support filters for:

- trust status
- mode capability
- recency (last_seen window)
- minimum link quality estimate

## Network query model

- Query propagation uses bounded scope controls (for example hop limit).
- Query responses include signed peer descriptors.
- Duplicate response suppression is required to limit control-plane chatter.

## Multi-hop relay model

### Route construction

- Route selection must support one or more intermediate relays.
- Route scoring should consider trust policy, estimated reliability, and expected latency.
- Maximum hop count must be configurable with a safe default.

### Relay behavior

- Relays forward signed payload units without mutating protected content.
- Relays may add hop metadata in an outer envelope.
- Replay protection and loop prevention are mandatory.

### Delivery semantics

- End-to-end integrity is verified at final destination.
- Hop-level acknowledgments support troubleshooting and route adaptation.
- Transfer should fail closed when trust policy for any hop is violated.

## Security and trust over relay paths

- Hop trust and end-to-end signer trust are evaluated independently.
- Route admission requires each selected relay to satisfy minimum trust policy.
- Multi-hop manifests should include relay-path commitments to prevent silent route rewriting.
- Post-quantum-capable signatures must be supported for route and transfer metadata under configured policy.

## Observability

Minimum relay telemetry fields:

- route_id
- hop_index and hop_count
- per-hop latency estimate
- per-hop retry count
- relay trust decision
- route change reason

## Initial conformance set

- peer cache insert/update/expire behavior
- query filtering correctness
- duplicate suppression under concurrent query responses
- route loop prevention under conflicting path advertisements
- relay transfer success across 2-hop and 3-hop paths
- route rejection when intermediate relay trust fails
