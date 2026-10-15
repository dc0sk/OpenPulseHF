---
project: openpulsehf
doc: docs/dev/peer-caching-relay.md
status: living
last_updated: 2026-10-15
---

# Peer Caching, Query, and Multi-Hop Relay

## Purpose

This document defines OpenPulseHF requirements and design constraints for peer discovery cache, query semantics, and multi-hop transfer relay.

> **Not part of Release 1.** Peer cache, query propagation and relay forwarding exist in
> `openpulse-core` (`peer_cache.rs`, `query_propagation.rs`, `relay.rs`, `wire_query.rs`) and in the
> experimental crates `openpulse-mesh` and `openpulse-discovery`, but Release 1 (`v0.17.0`) ships them
> disabled or experimental (work plan decision 1). The items below are requirements, not a
> description of shipped behaviour. As built, a `PeerRecord` holds `peer_id`, `capability_mask`,
> `route_quality`, `trust_level`, `revision`, `updated_at_ms` and `callsign_hash`; several fields
> listed below (identity key fingerprint, link-metric summary, quarantine of conflicting records) are
> not in it.

## Scope

- Discover and cache peer identity and capability records.
- Query local and network-discovered peer metadata.
- Route transfers through multiple relay hops when direct links are unavailable or inefficient.
- Preserve signed-transfer and trust-policy guarantees over relay paths.

Wire-level envelope and message schema details are specified in docs/dev/peer-query-relay-wire.md.

## Peer cache model

### Cache entries

Each peer cache entry should include:

- peer_id
- callsign or operator label
- identity key fingerprint
- supported capabilities (as built: the `capability_mask` bits registered in `peer-query-relay-wire.md`)
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
- Post-quantum-capable signatures must be supported for route and transfer metadata under configured policy. (Not implemented: route and relay signatures are Ed25519 only.)

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
