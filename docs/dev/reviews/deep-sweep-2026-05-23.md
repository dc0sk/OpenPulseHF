---
doc: docs/dev/reviews/deep-sweep-2026-05-23.md
date: 2026-05-23
status: initial
---

# Deep Sweep — Refactoring, Error Handling, DSP, API Gaps

Parallel survey across four dimensions: refactoring potential, error-handling
violations, DSP/protocol plausibility, and API/config gaps. Findings are
prioritized by likely operational impact.

---

## High Priority

### SWEEP-01 — OFDM soft-LLR scaling missing noise variance · Severity: High

**File:** `plugins/ofdm/src/demodulate.rs:90–92`

`qpsk_llr()` returns raw equalized symbol components `[c.re, c.im]` without
scaling by noise variance. Correct soft-LLR is `2 * symbol / sigma^2`. Effect:
turbo/LDPC decoders receive inconsistent confidence magnitudes; ARQ retransmit
combining mixes incomparable LLRs; coding gain degraded in faded channels.

Cross-check SCFDMA (which has explicit LLR weighting in `llr_weighting_adaptation`
tests) — OFDM lacks the equivalent path.

---

### SWEEP-02 — `.expect()` in OFDM/SCFDMA plugin public modulate/demodulate fns · Severity: High

**Files:**
- `plugins/ofdm/src/modulate.rs:12`, `plugins/ofdm/src/demodulate.rs:10`
- `plugins/scfdma/src/demodulate.rs:22, 30, 57`
- `plugins/qpsk/src/modulate.rs:146` (`qpsk_modulate_rrc_gpu`)

These are library-crate public functions reachable from `ModemEngine` dispatch.
A malformed mode parameter or unexpected pilot configuration crashes the entire
modem worker. Violates CLAUDE.md rule against `unwrap`/`expect` in library
production paths.

---

### SWEEP-03 — `.expect("cpal buffer poisoned")` × 6 in audio callbacks · Severity: High

**File:** `crates/openpulse-audio/src/cpal_backend.rs:147, 205, 241, 248, 267, 277`

Mutex-poison panics in audio I/O callbacks take down the entire process on a
thread crash. Should degrade gracefully: drop a buffer, log, and continue — a
stale audio frame is recoverable; killing the daemon is not.

---

## Medium Priority

### SWEEP-04 — Oversized `ModemEngine` (2718 lines) · Severity: Medium

**File:** `crates/openpulse-modem/src/engine.rs`

40+ match arms; `apply_ack_internal` (lines 292–345) couples rate-adapter,
session profile, and SNR-hint logic three levels deep. Hard to unit-test
individual transitions. Candidate: extract `RateAdaptationPolicy` and
`SnrFeedbackTracker` modules; keep `ModemEngine` as orchestration only.

---

### SWEEP-05 — Daemon TCP/WS ports and tick interval hardcoded · Severity: Medium

**File:** `crates/openpulse-daemon/src/main.rs:153–154, 274`

`127.0.0.1:9000`, `127.0.0.1:9001`, and `50ms` receive ticker are not in
`openpulse-config`. Operators running multiple daemons on one host cannot bind
to alternative ports without rebuilding. Add `[daemon] tcp_port`,
`websocket_port`, `receive_tick_ms` config keys.

---

### SWEEP-06 — Relay store-forward TTL inconsistent across TNCs · Severity: Medium

**Files:**
- `crates/openpulse-daemon/src/main.rs:225` (honors `cfg.relay.store_forward_ttl_s`)
- `crates/openpulse-kiss/src/main.rs`, `crates/openpulse-ardop/src/main.rs` (ignore it)

Same config field has different semantics depending on which TNC binary the
operator runs. Either propagate or document the gap.

---

### SWEEP-07 — Library `let _ = event_tx.send(...)` × 20 silently drops events · Severity: Medium

**File:** `crates/openpulse-modem/src/engine.rs` (lines 280, 336, 439, 505, 514, 529, 552, 569, 688, 739, 758, 807, 818, 852, 860, 901, 949, 958, 1132, 1170)

Dropped broadcast sends are observable via `Receiver::recv()` lag counters at
the consumer, but the producer side has no warning when subscribers fall
behind. At minimum log at `debug` on `Err`; consider a `dropped_events` counter
on `EngineEvent`.

---

### SWEEP-08 — B2F proposal/answer count mismatch silently accepted · Severity: Medium

**File:** `crates/openpulse-b2f/src/session.rs:188–196`

ISS receiving a `Fs` answer block uses `proposals.get_mut(i)` which silently
drops extra answers; fewer-than-expected answers leave proposals stuck in
non-`Accept` state. Add explicit count validation and reject mismatch with a
typed error.

---

### SWEEP-09 — `try_into().unwrap()` in FEC decoder · Severity: Medium

**File:** `crates/openpulse-core/src/fec.rs:218`

Bounds-checked but still uses `unwrap()` instead of `?`. Library production
path. One-line fix; eliminates a latent panic if buffer corruption survives
RS correction.

---

### SWEEP-10 — `openpulse-radio` rig_definition unchecked array conversion · Severity: Medium

**File:** `crates/openpulse-radio/src/rig_definition.rs:203`

`bytes[..4].try_into().unwrap()` after `bytes.len() >= 4` check. Same pattern
as SWEEP-09; should use `?`.

---

## Low Priority / Hygiene

### SWEEP-11 — Daemon-only operations unreachable from CLI · Severity: Low

CLI cannot drive a running daemon: `ConnectPeer`, `DisconnectPeer`,
`ListMessages`, `GetMessage`, `DeleteMessage`, `EnableRepeater`,
`DisableRepeater`, `SubscribeSpectrum`, `GetConfig`, `SetConfig` are control
protocol commands with no CLI counterpart. Add an `openpulse daemon <cmd>`
subcommand or a thin TCP-client crate.

---

### SWEEP-12 — `ArdopError` vs `KissTncError` error-tree asymmetry · Severity: Low

**Files:** `crates/openpulse-ardop/src/error.rs:8`, `crates/openpulse-kiss/src/error.rs:3–11`

ARDOP wraps `ModemError` via `#[from]`; KISS does not. `FrameTooLarge` is a
struct variant in ARDOP, tuple variant in KISS. Same conceptual error, divergent
ergonomics.

---

### SWEEP-13 — `apply_ack_internal` complexity · Severity: Low

**File:** `crates/openpulse-modem/src/engine.rs:292–345`

Three-level nesting with SNR-candidate selection inside rate-adapter dispatch.
Extract a pure `decide_rate_change(ack, snr_hint, profile) -> RateDecision`
helper that can be unit-tested without spinning up a `ModemEngine`.

---

### SWEEP-14 — Wire-codec scaffolding duplicated · Severity: Low

**File:** `crates/openpulse-core/src/wire_query.rs:109, 133, 361, 381`

Hand-rolled big-endian `read_u64`/`read_u32`/`read_arr32` repeated across each
message-type decoder. A `WireCodec` trait or `bytes::Buf`-based helper would
shrink the file and make new message types lower-effort.

---

### SWEEP-15 — `tx_levels`, `audio.iq_output`, `audio.iq_device` config fields unused · Severity: Low

**File:** `crates/openpulse-config/src/lib.rs:40, 96–100`

Schema fields with no readers. Either wire them up or delete from the schema
and template so operators don't set values that have no effect.

---

### SWEEP-16 — Inconsistent default features: `openpulse-audio` vs `openpulse-daemon` · Severity: Low

`openpulse-audio` defaults `["cpal-backend"]`; `openpulse-daemon` defaults `[]`.
A default daemon build has no audio; the failure mode is "PTT works, modem
silent" which is hard to debug.

---

### SWEEP-17 — Relay forwarder lacks observability · Severity: Low

**File:** `crates/openpulse-daemon/src/lib.rs:893–923` (`maybe_relay_forward`)

Successful forwards, TTL expiries, dedup hits, and hop-limit drops should emit
`tracing::info` so operators can audit relay traffic. Currently only failed
re-encodes log at warn.

---

## Summary table

Status legend: ✅ resolved · ⏳ deferred (out of scope for sweep cleanup; tracked separately)

| ID | Severity | Area | Action | Status |
|---|---|---|---|---|
| SWEEP-01 | High | DSP | OFDM soft-LLR scaling: multiply by `2/sigma^2` | ✅ Partial — per-subcarrier `|H|²` weighting added; absolute `sigma²` normalization deferred (would require restructuring channel estimation) |
| SWEEP-02 | High | Plugin panic | Replace `.expect()` in OFDM/SCFDMA/QPSK-GPU public fns with typed errors | ✅ `.expect()` removed; helper fns return empty outputs on unknown mode, typed errors propagate at the `ModulationPlugin` trait boundary |
| SWEEP-03 | High | Audio | Audio callback mutex-poison should degrade, not panic | ✅ |
| SWEEP-04 | Medium | Refactor | Split `ModemEngine` (extract rate policy + SNR tracker) | ✅ Extracted `RateAdaptationPolicy` (owns rate adapter, session profile, last-RX SNR) into `crates/openpulse-modem/src/rate_policy.rs`; SNR tracker folded into the same module since `select_rx_ack_type`/`apply_snr_hint` bridge SNR data to rate decisions |
| SWEEP-05 | Medium | Config | Hoist daemon TCP/WS ports + tick interval to config | ✅ |
| SWEEP-06 | Medium | Config | Honor `relay.store_forward_ttl_s` in KISS/ARDOP TNCs | ✅ |
| SWEEP-07 | Medium | Observability | Log on `event_tx.send()` failure in `ModemEngine` | ✅ (false positive — `broadcast::Sender::send` only errs on no subscribers; lag is consumer-side via `RecvError::Lagged`) |
| SWEEP-08 | Medium | Protocol | Validate B2F proposal/answer count match | ✅ |
| SWEEP-09 | Medium | Error | Replace `try_into().unwrap()` in `fec.rs:218` with `?` | ✅ |
| SWEEP-10 | Medium | Error | Same fix in `rig_definition.rs:203` | ✅ |
| SWEEP-11 | Low | API parity | Add CLI surface for daemon control commands | ⏳ Deferred — new feature work; 10 commands warrant a dedicated PR |
| SWEEP-12 | Low | API | Normalize `ArdopError`/`KissTncError` ergonomics | ✅ |
| SWEEP-13 | Low | Refactor | Extract `decide_rate_change` from `apply_ack_internal` | ✅ |
| SWEEP-14 | Low | Refactor | Generic wire-codec helper for `wire_query.rs` | ✅ (added `read_u16` helper; existing `read_u64`/`read_u32`/`read_arr32` already factored) |
| SWEEP-15 | Low | Config | Delete or wire up unused config fields | ✅ (removed `tx_levels`, `audio.iq_output`, `audio.iq_device`) |
| SWEEP-16 | Low | Build | Align default features between `openpulse-audio` and `openpulse-daemon` | ✅ (`openpulse-audio` default features cleared) |
| SWEEP-17 | Low | Observability | Add `tracing::info` to relay forwarding decisions | ✅ |
