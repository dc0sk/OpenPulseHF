---
project: openpulsehf
doc: docs/hpx-session-state-machine.md
status: living
last_updated: 2026-04-24
---

# HPX Session State Machine Specification

**Version**: 1.0.0  
**Status**: Normative specification for HPX mode (v2.0+)

---

## Executive Summary

This document defines the complete session lifecycle state machine for **HPX** (High-Performance eXperimental mode), OpenPulse's adaptive modulation and coding mode for HF radio. 

HPX sessions are bidirectional, authenticated exchanges where:
1. Two peers discover each other's capabilities
2. Jointly select an optimal modulation profile based on channel conditions
3. Exchange data with automatic rate adaptation and error recovery
4. Cleanly tear down the link with final statistics

This specification is independent of underlying modulation plugins, audio backends, and UI layers; it defines only the **session-level state transitions**, **timing constraints**, **security gates**, and **observability requirements**.

---

## Scope

### In Scope
- Session lifecycle: idle → discovery → training → active transfer → teardown → idle
- State transitions triggered by well-defined events
- Timing bounds and retry limits for each state
- Mandatory security gates (signature verification, trust policy checks)
- Event emission and observability requirements
- Conformance test scenarios

### Out of Scope
- Modulation/coding profile definitions (see `docs/high-performance-mode.md`)
- Channel estimation algorithms (implementation-specific)
- ARQ/FEC codec details (plugin-specific)
- Audio backend specifics
- Relay routing algorithms (future extension)

---

## Definitions

| Term | Definition |
|---|---|
| **Session** | A logical bidirectional connection between two peer stations |
| **Session ID** | 128-bit identifier assigned at session start; used in all frames |
| **Peer** | Another OpenPulse instance capable of HPX mode |
| **Profile** | A modulation+coding+power+bandwidth configuration (e.g., "HPX1200-QPSK-FEC") |
| **Capability** | A set of profiles a peer supports (discovered in training phase) |
| **Training** | Phase where peers discover each other and negotiate optimal profile |
| **ARQ** | Automatic Repeat reQuest; retransmission of failed chunks |
| **Manifest** | Signed summary of all data transferred in a session |
| **Relay** | An intermediate peer that forwards frames between two non-adjacent peers |
| **Trust Policy** | Set of rules determining whether a peer or relay is acceptable |
| **State** | One of the 9 defined session states |
| **Event** | Trigger that causes a state transition (e.g., `discovery_ok`) |
| **Reason Code** | Numeric identifier explaining why a transition occurred or failed |

---

## States

### 1. **idle**

**Purpose**: No active session.

**Entry**: System startup or return from teardown.

**Exit triggers**:
- `start_session` → `discovery`

**Invariants**:
- No session_id allocated
- No peer context in memory
- All timers idle

---

### 2. **discovery**

**Purpose**: Identify and verify peer, request capability set.

**Timeout**: 6 seconds (default)

**Entry**: `start_session` event.

**Actions**:
- Assign unique session_id (128-bit UUID)
- Transmit discovery message with local identity + signature
- Wait for peer's discovery acknowledgment

**Security Gate**: Peer's signature must verify against their public key.

**Exit triggers**:
- `discovery_ok` → `training`
- `discovery_timeout` → `failed`
- `signature_verification_failed` → `failed`
- `local_cancel` → `teardown`

---

### 3. **training**

**Purpose**: Select optimal modulation/coding profile.

**Timeout**: 10 seconds (default)

**Entry**: `discovery_ok` event.

**Actions**:
- Collect peer's supported profiles
- Perform optional channel probing
- Select optimal profile from intersection
- Both peers must converge to same profile

**Security Gate**: All probe frames carry authentication (HMAC/signature).

**Exit triggers**:
- `training_ok` → `active_transfer`
- `training_timeout` → `failed`
- `signature_verification_failed` → `failed`
- `relay_route_found` → `relay_active`
- `local_cancel` → `teardown`

---

### 4. **active_transfer**

**Purpose**: Exchange data with negotiated profile.

**Duration**: Unbounded (until completion or error).

**Entry**: `training_ok` event.

**Actions**:
- Configure modem with selected profile
- Exchange data frames with per-frame authentication
- Monitor channel quality; initiate recovery if degradation detected
- Use ARQ for failed chunks (max 6 consecutive retries per chunk)

**Exit triggers**:
- `transfer_complete` → `teardown`
- `transfer_error` → `recovery`
- `quality_drop` → `recovery`
- `relay_route_found` → `relay_active`
- `local_cancel` → `teardown`

---

### 5. **recovery**

**Purpose**: Attempt to resume transfer after error/quality drop.

**Timeout**: 8 seconds per attempt; max 4 attempts.

**Entry**: `transfer_error`, `quality_drop`, or `signature_verification_failed`.

**Actions**:
- Record current transfer state
- Perform optional channel re-estimation
- Send recovery request to peer
- Resume from last-confirmed chunk

**Exit triggers**:
- `recovery_ok` → `active_transfer`
- `recovery_timeout` → `failed`
- `relay_route_found` → `relay_active`
- `local_cancel` → `teardown`

---

### 6. **relay_active**

**Purpose**: Transfer data via relay peer(s).

**Entry**: `relay_route_found` event.

**Actions**:
- Select relay peers from route discovery
- Verify trust policy for each relay
- Maintain end-to-end authentication (relay cannot see content)
- Each hop preserves session_id and end-to-end signature

**Security Gate**: Each relay must pass trust policy check before activation.

**Exit triggers**:
- `transfer_complete` → `teardown`
- `transfer_error` → `recovery`
- `relay_policy_failed` → `failed`
- `local_cancel` → `teardown`

---

### 7. **teardown**

**Purpose**: Cleanly close session, verify manifest, emit result.

**Duration**: 0–2 seconds (orderly close).

**Entry**: `transfer_complete`, `local_cancel`, or `remote_teardown`.

**Actions**:
1. Compute SHA-256 hash of all transferred data (manifest)
2. Sign manifest with session key
3. Send close frame with manifest hash + signature
4. Receive peer's close acknowledgment and manifest hash
5. Compare manifests (advisory check; mismatch is warning, not fatal)
6. Emit final result event

**Exit triggers**:
- `transfer_complete` → `idle` (success)
- `transfer_error` → `failed` (failure)

---

### 8. **failed**

**Purpose**: Terminal failure state; no recovery.

**Entry**: From discovery, training, active_transfer, recovery, relay_active, or teardown on timeout, security failure, policy failure, or exhausted retries.

**Why**: Timeout (0x01), signature failure (0x02), quality drop (0x03), retries exhausted (0x04), recovery timeout (0x05), relay policy (0x06), recovery attempts exhausted (0x07), manifest verification failed (0x08), unclassified (0xFF).

**Invariants**:
- Session is permanently closed
- Reason code and diagnostic message recorded
- No further transitions

---

## Event Taxonomy

| Category | Events |
|---|---|
| Session control | `start_session`, `local_cancel`, `remote_teardown` |
| Discovery | `discovery_ok`, `discovery_timeout` |
| Training | `training_ok`, `training_timeout` |
| Data transfer | `transfer_complete`, `transfer_error`, `quality_drop` |
| Recovery | `recovery_ok`, `recovery_timeout` |
| Relay | `relay_route_found`, `relay_route_failed`, `relay_policy_failed` |
| Security | `signature_verification_failed` |

---

## State Transition Matrix

| From | Event | To | Actions |
|---|---|---|---|
| idle | start_session | discovery | Assign session_id, transmit discovery |
| discovery | discovery_ok | training | Verify signature, store capabilities |
| discovery | discovery_timeout | failed | (emit timeout diagnostic) |
| discovery | signature_verification_failed | failed | (emit trust failure) |
| discovery | local_cancel | teardown | Send close frame |
| training | training_ok | active_transfer | Configure modem, init ARQ |
| training | relay_route_found | relay_active | Select relay, verify trust |
| training | training_timeout | failed | (emit timeout diagnostic) |
| training | signature_verification_failed | failed | (emit auth failure) |
| training | local_cancel | teardown | Send close frame |
| active_transfer | transfer_complete | teardown | Compute manifest, sign |
| active_transfer | transfer_error | recovery | Store state, increment retries |
| active_transfer | quality_drop | recovery | Log SNR, start recovery timer |
| active_transfer | relay_route_found | relay_active | Switch to relay |
| active_transfer | signature_verification_failed | recovery | (frame auth failure) |
| active_transfer | local_cancel | teardown | Send close frame |
| recovery | recovery_ok | active_transfer | Resume from last-confirmed offset |
| recovery | recovery_timeout | failed | (emit recovery exhausted) |
| recovery | relay_route_found | relay_active | Switch to relay |
| recovery | local_cancel | teardown | Send close frame |
| relay_active | transfer_complete | teardown | Verify end-to-end manifest |
| relay_active | transfer_error | recovery | Increment retries |
| relay_active | relay_policy_failed | failed | (emit trust rejection) |
| relay_active | local_cancel | teardown | Send close frame |
| teardown | transfer_complete | idle | (session success) |
| teardown | transfer_error | failed | (session failure) |
| failed | (none) | (terminal) | (no transitions) |

---

## Timing Constraints

| Phase | Timeout | Action on Expiry |
|---|---|---|
| discovery | 6 s | → failed (reason: timeout) |
| training | 10 s | → failed (reason: timeout) |
| recovery window | 8 s per attempt | → retry or fail |
| recovery attempts | max 4 | → failed after 4 timeouts |
| ARQ retries | max 6 per chunk | → transfer_error |
| teardown close ack | 2 s (optional; don't wait) | (close frame is best-effort) |

---

## Retry Bounds

### ARQ Retries (Per Chunk)
- **Max consecutive**: 6
- **Backoff**: Exponential (1 ms, 2 ms, 4 ms, 8 ms, 16 ms, 32 ms) or linear
- **On exhaustion**: `transfer_error` → `recovery`

### Recovery Attempts (Per Session)
- **Max**: 4
- **Per-attempt timeout**: 8 s
- **Total recovery time**: ~32 s maximum
- **On exhaustion**: `recovery_timeout` → `failed`

---

## Security Gates

### Mandatory Authentication

| Frame Type | Authentication | On Failure |
|---|---|---|
| Discovery msg | Peer identity signature | → failed |
| Capability msg | Signature (discovery) | → failed |
| Channel probe | HMAC or signature | → recovery |
| Data frame | Per-frame HMAC | → recovery / failed |
| Relay frame | End-to-end signature | → recovery |
| Close frame | Peer identity signature | (log error, allow teardown) |

### Trust Policy Checks

- **Peer**: Public key trust level must be ≥ `marginal` (from PKI policy)
  - `trusted` / `verified`: allow
  - `marginal` / `unknown`: policy-controlled
  - `untrusted` / `revoked`: reject immediately

- **Relay**: Each relay peer must pass trust check before path activation
  - If policy rejects: `relay_policy_failed` → `failed`

---

## Observability

### Event Structure

Every state transition emits:

```json
{
  "timestamp_ms": 1234567890123,
  "session_id": "abc-def-ghi",
  "from_state": "discovery",
  "to_state": "training",
  "triggering_event": "discovery_ok",
  "reason_code": 0,
  "reason_string": "Peer discovered and verified",
  "peer_name": "W1ABC",
  "profile_id": "hpx1200-qpsk",
  "trust_decision": "verified"
}
```

### Reason Codes

| Code | Meaning |
|---|---|
| 0x00 | Success |
| 0x01 | Timeout |
| 0x02 | Signature / authentication failure |
| 0x03 | Quality drop / SNR degradation |
| 0x04 | Retries exhausted |
| 0x05 | Recovery timeout |
| 0x06 | Relay policy failed |
| 0x07 | Recovery attempts exhausted |
| 0x08 | Manifest verification failed |
| 0xFF | Unclassified error |

### Heartbeat (active_transfer only)

Emit progress every 2–5 seconds:

```json
{
  "timestamp_ms": 1234567890200,
  "session_id": "abc-def-ghi",
  "event_type": "progress",
  "state": "active_transfer",
  "progress_percent": 45,
  "bytes_transferred": 4500,
  "ary_retries": 12,
  "channel_snr_db": 11.8
}
```

---

## Conformance Tests

Minimum test coverage:

1. **Happy path**: idle → discovery → training → active_transfer → teardown → idle
2. **Discovery timeout**: idle → discovery → 6 s timeout → failed
3. **Training timeout**: discovery → training → 10 s timeout → failed
4. **Signature rejection (discovery)**: idle → discovery → invalid peer sig → failed
5. **Signature rejection (active_transfer)**: active_transfer → invalid frame HMAC → recovery
6. **Quality drop and recovery**: active_transfer → quality_drop → recovery → recovery_ok → active_transfer
7. **Recovery exhaustion**: active_transfer → quality_drop → 4 failed recoveries → failed
8. **Local cancel**: any non-terminal state → local_cancel → teardown → idle
9. **Remote teardown**: active_transfer → remote_teardown → teardown
10. **Relay activation**: training → relay_route_found → relay_active → active_transfer

---

## Non-Normative Notes

### Potential Extensions
- **Multi-hop relay**: Support N relays (currently 0 or 1)
- **Adaptive profile switching**: Change profile in active_transfer (not yet specified)
- **Implicit training**: Skip training if peers have cached capability info
- **Partial transfer resume**: Pause and resume session at later time

### Implementation Flexibility
Implementations may optimize:
- Timer durations (shorter/longer per hardware/network)
- ARQ backoff (linear vs. exponential)
- Recovery re-probing (light vs. aggressive)
- Trust policy (strict vs. permissive)

But **must** adhere to:
- State transitions (no shortcuts)
- Security gates (verify all signatures)
- Event emission (required for observability)
- Timeouts and bounds (no infinite loops)

---

## References

- [OpenPulse High-Performance Mode](docs/high-performance-mode.md)
- [PKI Trust Policy](docs/pki-tooling-trust-policy.md)
- [Modem Engine Architecture](docs/architecture.md)

