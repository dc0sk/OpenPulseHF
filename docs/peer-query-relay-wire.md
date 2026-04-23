---
project: openpulsehf
doc: docs/peer-query-relay-wire.md
status: living
last_updated: 2026-04-23
---

# Peer Query and Relay Wire Schema

## Purpose

This document defines an initial wire-level message schema for peer discovery queries, route discovery, and multi-hop relay transfer envelopes.

## Design goals

- Keep control-plane messages compact and versioned.
- Keep cryptographic fields explicit for signed and post-quantum-capable operation.
- Preserve compatibility via strict message type and schema version handling.

## Envelope format

All control-plane and relay-plane messages use a shared outer envelope:

```text
magic("OPHF") | version(u8) | msg_type(u8) | flags(u16) | session_id(u64) |
src_peer_id(32B) | dst_peer_id(32B) | nonce(96b) | timestamp_ms(u64) |
hop_limit(u8) | hop_index(u8) | payload_len(u16) | payload | auth_tag(16B)
```

Notes:

- magic: OpenPulseHF discriminator.
- version: wire schema version, initial value 1.
- nonce: unique per src_peer_id and session_id for replay protection.
- auth_tag: integrity tag for envelope fields and payload.

## Message type registry (initial)

- 0x01: peer_query_request
- 0x02: peer_query_response
- 0x03: route_discovery_request
- 0x04: route_discovery_response
- 0x05: relay_data_chunk
- 0x06: relay_hop_ack
- 0x07: relay_route_update
- 0x08: relay_route_reject

## peer_query_request payload

Required fields:

- query_id (u64)
- capability_mask (u32)
- min_link_quality (u16)
- trust_filter (enum: trusted_only, trusted_or_unknown, any)
- max_results (u16)

## peer_query_response payload

Required fields:

- query_id (u64)
- result_count (u16)
- results[] where each entry includes:
  - peer_id (32B)
  - callsign_hash (32B)
  - capability_mask (u32)
  - last_seen_ms (u64)
  - trust_state (enum)
  - descriptor_signature (variable)

## route_discovery_request payload

Required fields:

- route_query_id (u64)
- destination_peer_id (32B)
- max_hops (u8)
- required_capability_mask (u32)
- policy_flags (u16)

## route_discovery_response payload

Required fields:

- route_query_id (u64)
- route_id (u64)
- hop_count (u8)
- hops[] where each hop includes:
  - hop_peer_id (32B)
  - hop_trust_state (enum)
  - estimated_latency_ms (u16)
  - estimated_reliability_permille (u16)
- route_signature (variable)

## relay_data_chunk payload

Required fields:

- transfer_id (u64)
- chunk_seq (u32)
- total_chunks (u32)
- chunk_len (u16)
- chunk_hash (32B)
- e2e_manifest_hash (32B)
- chunk_signature (variable)
- chunk_data (variable)

Rules:

- Relays must not mutate e2e_manifest_hash, chunk_hash, or chunk_signature.
- Relay-specific metadata is carried in outer envelope fields.

## relay_hop_ack payload

Required fields:

- transfer_id (u64)
- chunk_seq (u32)
- hop_peer_id (32B)
- ack_status (enum: ok, retry, reject)
- retry_after_ms (u16)
- reason_code (u16)

## relay_route_update payload

Required fields:

- route_id (u64)
- previous_hop_count (u8)
- new_hop_count (u8)
- route_change_reason (u16)
- replacement_hops[] (same hop structure as route_discovery_response)
- route_update_signature (variable)

## relay_route_reject payload

Required fields:

- route_id (u64)
- reject_hop_peer_id (32B)
- reason_code (u16)
- trust_decision (enum)
- policy_reference (u16)

## Signature and algorithm metadata

Each signed payload must include signature metadata:

- sig_alg (enum)
- sig_mode (enum: classical, pq, hybrid)
- signer_key_id (32B)
- signature_bytes (variable)

Supported algorithm families for initial draft:

- classical: Ed25519
- pq-signature: ML-DSA
- optional hybrid: Ed25519+ML-DSA

## Anti-replay and loop prevention

- Receivers maintain a bounded replay window keyed by src_peer_id, session_id, and nonce.
- Messages with duplicate nonce in active window are rejected.
- hop_limit decrements at each relay and message is dropped at 0.
- route_id plus hop_index continuity checks reject looped or reordered relay flows.

## Compatibility and extension rules

- Unknown msg_type values must be ignored with diagnostic logging.
- Higher version messages are rejected with explicit unsupported_version reason.
- New payload fields are appended and identified by TLV extension blocks.

## Telemetry mapping

Every processed wire message should map to structured telemetry fields:

- msg_type
- session_id
- route_id (if present)
- hop_index and hop_limit
- trust_decision
- signature_verification_result
- replay_check_result
- latency_ms

## Conformance checkpoints

- Envelope parse/serialize round-trip for each msg_type.
- Replay rejection on duplicate nonce.
- hop_limit enforcement across 3-hop route.
- Route update acceptance and rejection behavior.
- Classical, PQ, and hybrid signature metadata validation.
