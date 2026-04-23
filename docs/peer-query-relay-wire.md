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
- All unsigned integer fields use network byte order (big-endian).

## Message type registry (initial)

- 0x01: peer_query_request
- 0x02: peer_query_response
- 0x03: route_discovery_request
- 0x04: route_discovery_response
- 0x05: relay_data_chunk
- 0x06: relay_hop_ack
- 0x07: relay_route_update
- 0x08: relay_route_reject

## Enum and code registries

### trust_filter

- 0x00: trusted_only
- 0x01: trusted_or_unknown
- 0x02: any

### trust_state and trust_decision

- 0x00: trusted
- 0x01: unknown
- 0x02: untrusted
- 0x03: revoked

### ack_status

- 0x00: ok
- 0x01: retry
- 0x02: reject

### sig_mode

- 0x00: classical
- 0x01: pq
- 0x02: hybrid

### sig_alg

- 0x0001: ed25519
- 0x0101: ml-dsa-65
- 0x0201: ed25519+ml-dsa-65-hybrid

### route_change_reason

- 0x0001: link_quality_degraded
- 0x0002: hop_unreachable
- 0x0003: trust_policy_change
- 0x0004: operator_override
- 0x0005: route_optimization

### reason_code (common)

- 0x0000: unspecified
- 0x0001: unsupported_version
- 0x0002: malformed_payload
- 0x0003: signature_invalid
- 0x0004: replay_detected
- 0x0005: hop_limit_exceeded
- 0x0006: loop_detected
- 0x0007: trust_policy_reject
- 0x0008: route_not_found
- 0x0009: congestion_backoff
- 0x000A: rate_limited

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

## TLV extension block registry (initial)

TLV blocks are encoded as type(u16), length(u16), value(bytes).

- 0x1001: peer_geo_hint
- 0x1002: peer_hardware_class
- 0x1003: estimated_energy_budget
- 0x1004: relay_cost_score
- 0x1005: operator_policy_tag
- 0x1006: pq_algorithm_preference

Rules:

- Unknown TLV types are ignored and preserved for forwarding when safe.
- Duplicate TLV type entries are rejected unless explicitly marked repeatable.
- TLV total size must not exceed payload_len.

## Field validation constraints

- payload_len must equal the decoded payload byte count.
- hop_index must be strictly less than hop_limit for forwarded messages.
- hop_count in route_discovery_response and relay_route_update must be in range 1..8.
- max_results in peer_query_request should be capped at 256 by receivers.
- timestamp_ms older than receiver replay window lower bound should be rejected.

## Compact binary examples (annotated hex)

These examples use fixed-size values and repeated byte patterns to keep the layout easy to verify in parser tests.

### Example A: peer_query_request (msg_type 0x01)

```text
Envelope:
4f 50 48 46                                  # magic "OPHF"
01                                           # version
01                                           # msg_type: peer_query_request
00 01                                        # flags
00 00 00 00 00 00 10 01                      # session_id
aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa
aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa aa # src_peer_id (32B)
bb bb bb bb bb bb bb bb bb bb bb bb bb bb bb bb
bb bb bb bb bb bb bb bb bb bb bb bb bb bb bb bb # dst_peer_id (32B)
11 11 11 11 11 11 11 11 11 11 11 11          # nonce (12B)
00 00 01 96 8f 3a 20 00                      # timestamp_ms
03                                           # hop_limit
00                                           # hop_index
00 11                                        # payload_len = 17

Payload (peer_query_request):
00 00 00 00 00 00 00 22                      # query_id
00 00 00 05                                  # capability_mask
01 2c                                        # min_link_quality (300)
01                                           # trust_filter: trusted_or_unknown
00 20                                        # max_results (32)

cc cc cc cc cc cc cc cc cc cc cc cc cc cc cc cc # auth_tag (16B)
```

### Example B: relay_hop_ack (msg_type 0x06)

```text
Envelope (same layout as above, abbreviated values):
4f 50 48 46 01 06 00 00
00 00 00 00 00 00 20 02                      # session_id
aa..aa (32B src) | bb..bb (32B dst)
22..22 (12B nonce)
00 00 01 96 8f 3b 10 10                      # timestamp_ms
04 02                                        # hop_limit, hop_index
00 31                                        # payload_len = 49

Payload (relay_hop_ack):
00 00 00 00 00 00 09 99                      # transfer_id
00 00 00 07                                  # chunk_seq
dd dd dd dd dd dd dd dd dd dd dd dd dd dd dd dd
dd dd dd dd dd dd dd dd dd dd dd dd dd dd dd dd # hop_peer_id (32B)
00                                           # ack_status: ok
00 00                                        # retry_after_ms
00 00                                        # reason_code: unspecified

ee ee ee ee ee ee ee ee ee ee ee ee ee ee ee ee # auth_tag (16B)
```

### Example C: route_discovery_request with TLV extension

This example appends TLV type 0x1006 (pq_algorithm_preference) to the payload.

```text
Envelope:
4f 50 48 46 01 03 00 00
00 00 00 00 00 00 30 03                      # session_id
aa..aa (32B src) | cc..cc (32B dst)
33..33 (12B nonce)
00 00 01 96 8f 3c 00 20                      # timestamp_ms
06 00                                        # hop_limit, hop_index
00 35                                        # payload_len = 53

Payload (route_discovery_request + TLV):
00 00 00 00 00 00 04 44                      # route_query_id
99 99 99 99 99 99 99 99 99 99 99 99 99 99 99 99
99 99 99 99 99 99 99 99 99 99 99 99 99 99 99 99 # destination_peer_id (32B)
05                                           # max_hops
00 00 00 09                                  # required_capability_mask
00 03                                        # policy_flags
10 06 00 02 01 01                            # TLV: type=0x1006, len=2, value=0x0101

ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff # auth_tag (16B)
```

Validation notes:

- Example A payload_len is 0x0011 = 17 bytes.
- Example B payload_len is 0x0031 = 49 bytes.
- Example C payload_len is 0x0035 = 53 bytes, including the TLV bytes.
- The examples intentionally use patterned bytes (aa, bb, cc, etc.) for visual alignment checks.

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
