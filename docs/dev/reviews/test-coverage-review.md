---
doc: docs/dev/reviews/test-coverage-review.md
date: 2026-05-22
status: resolved
resolved: 2026-05-23
---

# Test Coverage Review

## Summary

Test coverage is strong for BPSK/QPSK and the protocol stack. Channel simulation
tests cover BPSK only — QPSK, 8PSK, 64QAM, OFDM, and SC-FDMA have no channel-sim
integration tests. Turbo FEC is in the test matrix but only with BPSK250/QPSK500 and
only at AWGN 20 dB — no Watterson or burst channel. Negative tests are present and
numerous. SC-FDMA has the best plugin-level coverage with 5 dedicated test files.

---

## Findings

### TST-01 — Channel simulation covers BPSK only · Severity: Medium

**File:** `crates/openpulse-modem/tests/channel_loopback.rs`

The channel simulation harness (`ChannelSimHarness`) is only exercised with
`BpskPlugin`. Modes tested: `BPSK250`, `BPSK31`. No channel-sim tests exist for:

- QPSK (any rate)
- 8PSK (any rate)
- 64QAM (any rate)
- OFDM16 / OFDM52
- SC-FDMA modes

Channel models exercised: clean, AWGN 20 dB, Watterson Good F1, Watterson Extreme
(degradation test), Watterson Good F2 + FEC, Gilbert-Elliott light + FEC, G-E moderate
(degradation test).

**Recommendation:** Add a `channel_loopback_multimode.rs` test file that runs at least
QPSK500, OFDM52, and SCFDMA52 through AWGN 20 dB and Watterson Good F1. These are the
modes that carry actual traffic in `hpx500` and `hpx_wideband_hd` profiles.

---

### TST-02 — Turbo test matrix: no Watterson or burst channels · Severity: Low

**File:** `apps/openpulse-testmatrix/src/cases.rs:263–278`

Turbo FEC is exercised in the test matrix for `BPSK250` and `QPSK500` under clean
channel and AWGN 20 dB only. The turbo BER test in `crates/openpulse-core/tests/turbo_ber.rs`
uses a synthetic AWGN channel (not the `ChannelSimHarness`).

No test verifies that turbo + BPSK250 recovers correctly through a Watterson fading
channel at the same SNR where LDPC is known to pass.

**Recommendation:** Add Watterson Good F1 entries to the turbo section of the test
matrix, or a dedicated `turbo_watterson.rs` integration test.

---

### TST-03 — No channel-sim tests for LDPC + multicarrier · Severity: Low

**File:** `crates/openpulse-modem/tests/ldpc_engine_loopback.rs`

LDPC loopback tests exist but use only the loopback backend (clean channel). There is
no test of LDPC error-correction capability through AWGN or Watterson.

**Recommendation:** Add an AWGN 15 dB test for `LDPC + BPSK250` and `LDPC + QPSK500`
in `channel_loopback.rs` or a dedicated `ldpc_channel.rs` file.

---

### TST-04 — Negative test coverage is strong · Severity: Pass

`grep -rn "assert.*is_err\|expect_err\|assert_err\|should_panic" crates/*/tests/
plugins/*/tests/` finds 40+ negative assertion sites covering:
oversized payloads, malformed frames, unknown mode strings, expired SAR slots, CRC
mismatches, and dispatch errors. Error paths are well exercised.

---

### TST-05 — SC-FDMA has strong plugin-level test coverage · Severity: Pass

SC-FDMA has 5 dedicated test files:
- `tests/loopback.rs` — clean loopback for all modes
- `tests/pilot_channel_estimation.rs` — DFT-CE correctness
- `tests/pilot_density_review.rs` — adaptive pilot density state machine
- `tests/llr_weighting_adaptation.rs` — LLR combining
- `tests/scfdma_acquisition.rs` — preamble sync
- `tests/scfdma_fec_interleaver.rs` — FEC+interleaver round-trip

No other plugin has comparable coverage depth.

---

### TST-06 — FSK4 plugin: no integration tests · Severity: Low

**File:** `plugins/fsk4/`

FSK4 has only inline unit tests (loopback in `src/lib.rs`). There is no integration
test file under `plugins/fsk4/tests/`. Given FSK4 is the ACK control channel — the
transport for `RateAdapter` events — a failure mode here would silently break rate
adaptation.

**Recommendation:** Add `plugins/fsk4/tests/fsk4_integration.rs` with at minimum a
loopback round-trip and an AWGN degradation test.

---

### TST-07 — Test matrix missing SC-FDMA HOM modes · Severity: Low

**File:** `apps/openpulse-testmatrix/src/cases.rs`

`SCFDMA52-8PSK`, `SCFDMA52-16QAM`, `SCFDMA52-32QAM`, `SCFDMA52-64QAM`, and
`SCFDMA52-64QAM-P4` are defined in the plugin but do not appear in the test matrix
(only `SCFDMA16` and `SCFDMA52` baseline QPSK modes are listed in `MULTICARRIER_MODES`).

**Recommendation:** Add the HOM SC-FDMA modes to the test matrix with appropriate SNR
thresholds (already defined in `mode_min_snr_db()`).

---

## Test file inventory (67 files)

Integration tests span: ardop (1), b2f (1), b2f-driver (3), cli (6), core (13),
daemon (2), freedv-auth (1), kiss (1), mesh (1), modem (22), qsy (2), radio (2),
repeater (1), scfdma plugin (6).

---

## Action Items

| ID | Severity | Action | Resolution |
|---|---|---|---|
| TST-01 | Medium | Add channel-sim tests for QPSK, OFDM, SC-FDMA through AWGN + Watterson | ✅ `crates/openpulse-modem/tests/channel_loopback_multimode.rs` — QPSK500/AWGN, QPSK500/Watterson Good F1 (degrades), OFDM52/AWGN, OFDM52/Watterson, SCFDMA52-16QAM/AWGN, SCFDMA52-64QAM/AWGN |
| TST-02 | Low | Add Watterson test for turbo FEC | ✅ `channel_loopback.rs::watterson_good_f1_bpsk250_turbo` — Watterson Good F1 through `FecMode::Turbo` |
| TST-03 | Low | Add AWGN channel-sim test for LDPC | ✅ `channel_loopback.rs::awgn_15db_bpsk250_ldpc` — AWGN 15 dB through `FecMode::Ldpc` |
| TST-06 | Low | Add `plugins/fsk4/tests/fsk4_integration.rs` | ✅ `plugins/fsk4/tests/fsk4_integration.rs` added with loopback round-trip and AWGN degradation tests |
| TST-07 | Low | Add SC-FDMA HOM modes to testmatrix | ✅ `SCFDMA_HOM_MODES` const in `apps/openpulse-testmatrix/src/cases.rs` covers SCFDMA52-8PSK, -16QAM, -32QAM, -64QAM, -64QAM-P4 with SNR gates from `mode_min_snr_db()` |
