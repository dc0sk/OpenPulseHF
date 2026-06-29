---
project: openpulsehf
doc: docs/dev/research/freedv-auth-research.md
status: shipped
last_updated: 2026-06-17
---

# FF-11 — FreeDV Authenticated Voice Shim: Design Research

> **Status: SHIPPED.** This is the original design-research note (predates implementation).
> FF-11 shipped as the `openpulse-freedv-auth` crate (PR #162); the open questions below
> were resolved during implementation. Kept for design rationale.

## Problem statement

FreeDV transmits codec2-compressed voice digitally.  Anyone can replay a recorded FreeDV
stream, or inject synthetic voice, and the recipient has no way to verify that the
transmission originated from the claimed callsign.  FF-11 adds Ed25519-signed
*authentication beacons* to FreeDV sessions without modifying FreeDV source code and
without degrading voice quality.

---

## Codec2 determinism (confirmed)

codec2 is deterministic: identical PCM input consistently produces the same compressed
bitstream.  This property makes frame-level signing possible in principle — but it
requires intercepting the codec2 bitstream before FreeDV's modem, which is non-trivial
(see §Interface below).

---

## What is being authenticated

Two candidate models were evaluated.

| Model | What is signed | Bandwidth | Tamper evidence |
|---|---|---|---|
| **Content signing** | Each codec2 frame batch | High (per-batch) | Proves specific audio |
| **Station identity** | Callsign + timestamp + session nonce | Low (once per QSO) | Proves station was QRV |

**Decision: station identity (Model 4).**

For HF amateur radio the legal obligation is call sign identification, not content
integrity.  Proving "W1AW, keyholder of this Ed25519 key, was transmitting at this
time and frequency" satisfies FCC Part 97 identification requirements and is
cryptographically meaningful.  Signing individual audio frames adds no additional
regulatory value and requires a C-level FreeDV/codec2 library dependency.

Auth beacon payload:

```json
{
  "callsign": "W1AW",
  "timestamp_utc": 1746800000,
  "session_nonce": "<16 random bytes, hex>",
  "freq_hz": 14236000,
  "mode": "FreeDV-1600",
  "signing_mode": "ed25519"
}
```

Signature covers the canonical JSON of this struct (same pattern as HPX CONREQ/CONACK).
Ed25519 signature = 64 bytes.  Total beacon wire size ≈ 144 bytes.

---

## FreeDV data channel capacity

| FreeDV mode | Frame period | "txt" channel | Data callback API | 144 B beacon time |
|---|---|---|---|---|
| 1600 (FDMDV) | 40 ms | ~100 bps (~12 B/s) | No |  ~12 s |
| 700D (OFDM) | 20 ms | Very limited | Yes (callback) | Mode-dependent |
| 2020 (OFDM) | 120 ms | Limited | Yes (callback) | Mode-dependent |
| DATAC3 (data-only) | ~500 ms/frame | 126 B/frame | Dedicated | 1–2 frames (~1 s) |
| DATAC1 (data-only) | ~500 ms/frame | 510 B/frame | Dedicated | 1 frame |

Key conclusions:
- The **txt channel** in 1600 carries ~12.5 bytes/sec — enough for one 144-byte beacon
  in ~12 seconds.  Usable for once-per-QSO authentication.
- **DATAC3** (data-only mode) fits the entire beacon in 1–2 frames (~1 second).  Ideal
  for an auth burst at QSO start, but requires switching the modem out of voice mode
  briefly (≤1 s interruption).
- FreeDV's data callback API (`freedv_datatx`/`freedv_datarx`) is the most flexible
  injection point, but requires C FFI via `codec2-sys` or bindgen.

---

## Interface mechanism options

### Option A — C FFI (libfreedv / codec2-sys)

Bind against `freedv_api.h` using `codec2-sys` or a custom `bindgen` output.  Register
`freedv_set_callback_datatx()` and `freedv_set_callback_datarx()` to inject and receive
beacon bytes inside the FreeDV process.

| Criterion | Rating |
|---|---|
| Control | Full: frame-level injection, timing control |
| Complexity | High: C dependency, unsafe FFI |
| `--no-default-features` | Breaks unless feature-gated (`freedv` feature) |
| codec2-sys maintenance | Poor: crate is minimally maintained |

### Option B — Pipe / child-process wrapping

Run `freedv_tx` / `freedv_rx` as child processes; intercept stdin/stdout.  The
intercepted data is raw audio PCM, not codec2 frames — so signed content cannot be
codec2-frame-aligned without running codec2 independently.

| Criterion | Rating |
|---|---|
| Control | Limited: audio level only, no data channel access |
| Complexity | Medium |
| `--no-default-features` | Compatible |
| Timing | Cannot align with FreeDV frame boundaries |

**Verdict: not suitable for frame-level signing; only viable for audio envelope signing.**

### Option C — UDP data port injection (recommended)

Modern FreeDV builds (GUI, ≥1.6) expose a UDP data port (default `127.0.0.1:10001`)
for external data injection.  The shim sends beacon bytes to this port; FreeDV schedules
them into the data channel at the next available slot.  On receive, FreeDV writes
incoming data bytes to the same port; the shim reads and verifies them.

| Criterion | Rating |
|---|---|
| Control | Good: no timing control, but scheduling is FreeDV's problem |
| Complexity | Low: pure UDP, no C dependency |
| `--no-default-features` | Fully compatible |
| FreeDV version | Requires FreeDV GUI ≥ 1.6; command-line tools need a wrapper |

**Verdict: recommended for the initial implementation.**

### Option D — PTT-triggered DATAC burst (complementary, not exclusive)

At PTT-on: transmit a DATAC3 burst (1–2 frames, ~1 s) containing the signed auth beacon
before handing off to FreeDV.  At PTT-off: transmit a session-end beacon.  Uses the
existing `openpulse-radio` PTT infrastructure.  No FreeDV data port required.

This can be **combined with Option C**: use the UDP data port during voice transmission
for ongoing presence beacons; use DATAC PTT-burst for the opening/closing handshake.

---

## Signing infrastructure reuse

The following already exists in `openpulse-core` and is directly reusable:

| Item | Location | Status |
|---|---|---|
| `ed25519_sign(seed, msg)` | `pq_handshake.rs:183` | **Private** — needs pub promotion |
| `verify_ed25519(pubkey, msg, sig)` | `handshake.rs:324` | **Private** — needs pub promotion |
| `InMemoryTrustStore` | `trust.rs` | Public |
| `SignedEnvelope` + `SignatureBlock` | `signed_envelope.rs` | Public |
| `ConnectionTrustLevel` | `trust.rs` | Public |
| ML-DSA-44 hybrid signing | `pq_handshake.rs` | Public (via `create_pq_conreq`) |

**Required change to `openpulse-core`**: add a `pub fn sign_bytes(seed: &[u8; 32], msg: &[u8]) -> [u8; 64]` and `pub fn verify_bytes(pubkey: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> bool` in a new `crates/openpulse-core/src/signing.rs` module.  This avoids duplicating the ed25519-dalek API surface in every consumer crate and is useful beyond FF-11.

---

## Proposed crate structure

**`crates/openpulse-freedv-auth`** (library + `openpulse-freedv-auth` binary):

```
src/
  beacon.rs       — AuthBeacon struct; sign() → [u8; 144]; verify() → TrustVerdict
  scheduler.rs    — BeaconScheduler: timer-based repeat (default: every 60 s)
  data_port.rs    — FreeDvDataPort: UDP read/write to FreeDV data port
  verdict.rs      — TrustVerdict { Verified(callsign), Unverified, Invalid }; Unix-socket API
  ptt.rs          — PttTrigger: listen for PTT events from openpulse-radio; fire DATAC burst
  lib.rs          — pub re-exports
  main.rs         — CLI: --freedv-data-port, --key-file, --trust-store, --callsign
```

**Dependencies** (no C, no codec2):
- `openpulse-core` (signing, trust store)
- `openpulse-radio` (PTT events)
- `tokio` (async UDP, Unix socket)
- `ed25519-dalek` (already a workspace dependency)
- `rand` (session nonce)

**Feature flags**:
- `pq` — enable ML-DSA-44 hybrid signing (pulls in `ml-dsa`)
- Default: Ed25519-only

---

## Verdict API (local, operator UI)

The shim exposes a small Unix-socket (or TCP `127.0.0.1:5001`) API so a FreeDV companion
UI can poll authentication status:

```json
{"verdict": "Verified", "callsign": "W1AW", "key_id": "a3f9...", "last_beacon_utc": 1746800060}
{"verdict": "Unverified", "reason": "no beacon received"}
{"verdict": "Invalid", "reason": "signature verification failed"}
```

---

## Key open questions (must resolve before implementation)

| # | Question | Options | Impact |
|---|---|---|---|
| 1 | FreeDV version scope | 1.6 GUI only vs. CLI tools vs. both | Determines whether UDP data port is available |
| 2 | DATAC PTT burst: desirable? | Yes (requires PTT control) / No (beacon-only in voice data channel) | Adds `openpulse-radio` dep; requires station PTT wiring |
| 3 | Beacon repetition interval | 30 s / 60 s / once per QSO | FCC ID requirement is every 10 min; 60 s is conservative |
| 4 | ML-DSA-44 hybrid now or later | Ed25519-only first / Hybrid from start | Complexity; `pq` feature flag defers it cleanly |
| 5 | Trust store source | InMemoryTrustStore populated from TOML / live PKI query | Offline operation vs. PKI dependency |

---

## Feasibility summary

| Concern | Assessment |
|---|---|
| 144-byte beacon in FreeDV data channel | **Feasible**: 12 s via txt channel; 1 s via DATAC burst |
| No FreeDV source modification | **Confirmed**: UDP data port is sufficient |
| No C dependency in critical path | **Confirmed**: UDP-based approach is pure Rust |
| Ed25519 signing infrastructure | **Reusable** with minor pub-promotion in openpulse-core |
| Regulatory compliance | **Compatible**: identifies station; does not encrypt content |
| Codec2 determinism (if needed later) | **Confirmed**: same PCM → same bitstream |

**Bottom line**: FF-11 is implementable in 2–3 days of development work, with no
fundamental blockers.  The main decision is Q1 above (FreeDV version scope) — answering
that pins the interface mechanism.  Everything else has a clear path.

---

## Recommended implementation sequence (when approved)

1. `openpulse-core`: add `pub signing::sign_bytes` / `verify_bytes` (1 hour)
2. `crates/openpulse-freedv-auth/beacon.rs`: `AuthBeacon` struct, sign/verify (2 hours)
3. `crates/openpulse-freedv-auth/data_port.rs`: UDP send/receive (2 hours)
4. `crates/openpulse-freedv-auth/verdict.rs`: `TrustVerdict`, Unix-socket API (2 hours)
5. `crates/openpulse-freedv-auth/scheduler.rs`: 60 s repeat timer (1 hour)
6. Integration tests: mock UDP server, sign → transmit → verify round-trip (2 hours)
7. `main.rs` CLI binary (1 hour)
8. (Optional) `ptt.rs` + DATAC burst: deferred until Q2 is answered

Total estimated effort: **~11 hours**, plus Q1 answer and any freeDV environment setup.
