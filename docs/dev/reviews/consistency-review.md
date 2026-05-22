---
doc: docs/dev/reviews/consistency-review.md
date: 2026-05-22
status: initial
---

# Consistency Review

## Summary

LLR sign convention and bit ordering are consistent (see correctness review). Plugin
trait surface has three gaps: FSK4 lacks `demodulate_soft` and `modulate_iq`; OFDM
lacks `demodulate_soft`. Mode string naming is consistent. Serde derives are present on
all API-crossing types. No `anyhow` in library crates. Config wiring has one gap in
the daemon (`pki_store_path` is parsed but not used to load a trust store at startup).

---

## Findings

### CON-01 — FSK4 plugin missing `demodulate_soft` and `modulate_iq` · Severity: Low

**File:** `plugins/fsk4/src/lib.rs`

`Fsk4Plugin` does not override `demodulate_soft` or `modulate_iq`. The default
`modulate_iq` falls back to the Hilbert-transform path (correct but slower than a
native baseband implementation). The default `demodulate_soft` produces ±1.0 hard
symbols cast to f32, which degrades LDPC/turbo iteration gain when FSK4-ACK is used
with soft FEC.

In practice FSK4 is only used for the ACK control channel, which uses hard-decision
decoding, so this is low risk. However it violates the plugin interface contract that
soft outputs should be genuine LLRs.

**Recommendation:** Either implement `demodulate_soft` for FSK4 with Goertzel-based
LLR output, or document that FSK4 intentionally provides only hard decisions and is
not intended for soft-FEC pipelines.

---

### CON-02 — OFDM plugin missing `demodulate_soft` · Severity: Low

**File:** `plugins/ofdm/src/lib.rs`

`OfdmPlugin` does not override `demodulate_soft`. The default returns ±1.0 hard
symbols, which prevents LDPC/turbo soft-iteration gain on OFDM subcarriers. Given that
each subcarrier already carries a QPSK symbol whose LLR is readily computable from the
FFT bin amplitudes, this is a straightforward gap.

**Recommendation:** Implement `demodulate_soft` using per-subcarrier QPSK LLR
computation from the IFFT output.

---

### CON-03 — Daemon does not load trust store at startup · Severity: Low

**File:** `crates/openpulse-daemon/src/main.rs`

`OpenpulseConfig` has a `trust_store.path` field. The ARDOP and KISS bridges read this
and load the trust store at startup (wired in their `main.rs`). The unified daemon
(`openpulse-server`) ignores `trust_store.path` — no `InMemoryTrustStore` is
constructed or passed to any handshake verifier.

**Recommendation:** Wire `trust_store_file::load_trust_store_from_file` in the daemon
startup, mirroring the ARDOP/KISS bridge pattern.

---

### CON-04 — Mode string naming is consistent · Severity: Pass

Mode strings across all plugins follow the pattern:
`{FAMILY}{BAUD}[-{VARIANT}]` in uppercase, e.g. `BPSK250`, `QPSK500`,
`8PSK1000-RRC`, `64QAM2000-RRC`, `SCFDMA52-64QAM-P4`, `OFDM52`, `FSK4-ACK`.
No inconsistent separators (e.g. underscores or lowercase) found.

---

### CON-05 — `anyhow` not used in library crates · Severity: Pass

`grep -rn "anyhow" crates/openpulse-core/src crates/openpulse-modem/src
crates/openpulse-audio/src crates/openpulse-dsp/src` returned no results.
All library crates use `thiserror`-derived errors. `anyhow` is correctly confined to
CLI and test code.

---

### CON-06 — Serde derives present on all API-crossing types · Severity: Pass

`ControlEvent`, `ControlCommand`, `EngineEvent`, and `SessionDiagnostics` all carry
`#[derive(Serialize, Deserialize)]`. The `HpxState` and `RateEvent` types (emitted in
the NDJSON event stream) are also derived. No gap found.

---

### CON-07 — `demodulate_soft` missing from FSK4 and OFDM in plugin registry · Severity: Low

Plugins that do not override `demodulate_soft` return the hard-decision fallback silently.
There is no runtime warning or capability flag indicating that soft output is unavailable.
A caller that passes FSK4 or OFDM LLRs into a turbo decoder will receive degraded results
without any diagnostic.

**Recommendation:** Add a `supports_soft_demod() -> bool` method to `ModulationPlugin`
(default `false`) that implementations override to `true` when they provide genuine LLRs.
The modem engine can log a warning when `transmit_with_fec_mode` selects a soft-FEC mode
with a plugin that returns `false`.

---

## Action Items

| ID | Severity | Action |
|---|---|---|
| CON-01 | Low | Document FSK4 hard-decision-only contract or implement Goertzel LLR |
| CON-02 | Low | Implement `demodulate_soft` for OFDM with per-subcarrier QPSK LLRs |
| CON-03 | Low | Wire `load_trust_store_from_file` in daemon startup |
| CON-07 | Low | Add `supports_soft_demod()` capability flag to `ModulationPlugin` |
