---
project: openpulsehf
doc: docs/dev/design/ofdm-carrier-offset.md
status: review
last_updated: 2026-11-03
---

# The OFDM receiver acquires its own carrier offset

Work plan M3; the maintainer's decision on 2026-11-03 to fix it before M3. Found by the acquisition
comparison (`docs/dev/research/acquisition-comparison-codec2-ft8.md` §1).

## Problem — measured

The OFDM rungs (SL7–SL14, the `fast` profile, Release 1) had no carrier-frequency correction.
- The engine applies AFC by moving `center_frequency`. The OFDM receiver refused any centre but
  1500 Hz on its hard path, and ignored it on its soft path (SL7–SL10 decode soft).
- An uncorrected offset leaks power between the 31.25 Hz subcarriers. By estimate, (πε)²/3 of each
  subcarrier's power goes to its neighbours, which is −17 dB at 2.5 Hz.
- **Measured** on the daemon's receive path (`accumulate_capture` → `ota_decode_burst`, a 0.002-RMS
  floor), before the fix:

  | Rung | Decoded | Failed |
  |---|---|---|
  | SL7 | to ±7.5 Hz | ±10 Hz |
  | SL10 | −2.5 to +5 Hz | −5 Hz |
  | SL14 | ±1 Hz | ±2.5 Hz |

- **On air:** rigs are 4 Hz apart on 2 m after trimming, 54–64 Hz untrimmed, and commonly 10–20 Hz on
  HF.

**Making the receiver mix to the told centre was not enough.** With that change alone, the engine's
AFC settle, built for BPSK, read garbage on OFDM, e.g. −11.5 Hz for a true +2.5 Hz:
- each settle window converged to its own wrong value (−8.4, −15.5, +9.9, +2.2, −14.6 Hz);
- SL14 still failed at 2.5 and 5 Hz;
- nothing decoded past ±15 Hz, because the pilot-drift estimate is unambiguous only to ±13.9 Hz.

## Decision

The OFDM plugin acquires its own offset in its receive entries, and opts out of the engine's AFC
(`estimate_afc_hz` returns `None`), as MFSK16 does. This is codec2's DATAC approach: a coarse search,
then the pilots for the fine part.
1. **The told centre:** mix the burst from `center_frequency` back to 1500 Hz (`at_nominal_centre`).
   This is whatever the engine's BPSK settles left, usually within a few Hz.
2. **The coarse grid** (`acquire::acquire_offset_hz`): from −50 to +50 Hz in 12.5 Hz steps,
   9 candidates, 0 first. For each candidate:
   - mix a short window from the burst's start (the timing search's reach plus 9 symbols, about 11 000
     samples);
   - find the timing **on the mixed window**: the real-valued Schmidl–Cox metric falls as
     cos(πΔ/31.25 Hz) and is zero at ±15.6 Hz, so timing must come after the mix;
   - score the pilots' coherence across symbols, |Σ a*b| / Σ |a||b|. A wrong offset puts data, not
     pilots, in the pilot bins.
3. **The fine part:** the best candidate's pilot drift (`channel::pilot_drift`), then one more pass at
   the sum.
4. **One exact mix of the whole burst** (`shift::shift_hz`: an analytic signal by FFT, times
   e^{j2πft}), then the existing receiver.

`demodulate`, `demodulate_soft` and `estimate_snr_db` all go through step 1–4. The SNR the ladder
reads is therefore measured after the correction; an uncorrected offset would cut it.

**The constants, what they were set from, and what would prove them wrong.**
- **`COARSE_STEP_HZ` 12.5:** from two limits of the code, not fitted.
  - Its half (6.25 Hz) must stay inside the pilot drift's ±13.9 Hz and away from the timing null at
    ±15.6 Hz; 6.25 Hz keeps 81 % of the timing metric.
  - **Falsified by** a frame that fails at an offset halfway between grid points (6.25, 18.75 Hz …)
    and decodes on the grid.
- **`COARSE_SPAN_HZ` 50:**
  - codec2 DATAC's burst search covers ±50 Hz;
  - untrimmed 2 m rigs were 54–64 Hz apart (`openpulse-channel` `cfo.rs`), and the engine's BPSK
    settle normally moves the centre first.
  - **Falsified by** an on-air OFDM frame that fails with the BPSK rungs decoding at the same time
    (the centre known) and the OFDM offset more than 50 Hz from it.
- **`SCORE_SYMS` 9:** the pilot drift reads at most 8 symbol pairs.

## Measured after the fix (x86, release)

- **Production path** (the probe): SL7, SL8, SL10, SL12 and SL14 at 0, +1.5, −2.5, +5, −7.5, +10, −15,
  +20, −30, +40 and −50 Hz: **55 of 55 decoded, each in 1 attempt.**
- **Pinned test** (`ofdm_carrier_offset_ota`): SL7, SL10 and SL14 at +2.5, −7.5, +15 and −40 Hz:
  - 12 of 12 in 1 attempt each;
  - the SNR read is within 1 dB of on-carrier at every rung (SL7 6.0–7.2 against 6.6 dB, SL10
    10.5–11.4 against 10.2, SL14 9.0–9.5 against 9.1).
- **Cost:** the pinned test's 15 decodes take 0.66 s in all, the modulation included.

## Risks and how each is checked

- **A wrong coarse pick:**
  - a strong narrowband interferer in a pilot bin could raise a wrong candidate's coherence;
  - noise lowers every candidate's coherence equally, so the choice is relative;
  - UNCHECKED on air: the test is a clean channel plus a floor.
- **A frame near the end of a long burst:** acquisition uses the first ~11 000 samples, as the timing
  search already did (`SEARCH_CAP`). Nothing changes there.
- **Silence:**
  - on exact digital silence, the mix's round trip leaves a residue near 1e-6 that the timing
    metric, normalised by its own energy, can lock to;
  - seen in a test without a noise floor, so every test carries a floor;
  - a receiver always has one, but the behaviour is noted here.
- **The engine's corrected pass:** with no estimate from OFDM, the engine's settle no longer moves the
  centre for the OFDM candidate. The plugin's own acquisition covers that.

## Not changed, and found on the way

- **The SNR the OFDM receiver reads is low on a clean channel**, before and after this change: SL7
  reads 6.6 dB over a 0.002 floor. The `fast` profile's SNR floor for SL7 is 9 dB, so `climb_on_snr`
  may never take the fast ladder past SL7.
  - UNCHECKED, and a possible Release 1 risk of its own. The `fast` profile has never been on air.
  - Put to the maintainer, not fixed here.

## Tests

- **`ofdm_carrier_offset_ota` (`openpulse-modem`):** the production path.
  - Passes as above.
  - **Sabotage**, with acquisition switched off: SL7 fails at −7.5 Hz (15 attempts).
- **`carrier_offset_applied` (`ofdm-plugin`):** every mode, from −40 to +40 Hz, hard and soft.
  - The playbook's swept-applied correction (told).
  - The acquisition (not told).
  - The raw demodulator fails 64QAM at 5 Hz, which shows the channel really moved the signal.
  - **Sabotage:** with the mix removed, "told" fails at −40 Hz; with acquisition off, "not told" fails
    at −40 Hz.
- **`shift::tests`:** a tone moves by the shift and keeps its level (odd and even lengths, fractional
  Hz); a zero shift is the identity.
- **`acquisition_is_near_zero_on_carrier_and_the_engine_gets_no_estimate`:** replaces the two tests
  that asserted OFDM publishes an engine AFC estimate.

## Consumer

`ModemEngine` calls `demodulate` / `demodulate_soft` / `estimate_snr_db` on the OFDM plugin for every
OFDM decode, the daemon's OTA receive included (`nack_budget::decode_ota_burst` →
`ota_decode_burst`). Found with `grep -n "estimate_afc_hz\|demodulate_soft(" crates/openpulse-modem/src/engine.rs`.

## Prior art

- **codec2 DATAC** (`ofdm.c`, `310777b`): a joint ±50 Hz preamble search, then pilot tracking (the
  research note §1).
- **`openpulse-channel` `CfoChannel`:** the same analytic-signal method, used here as the test's
  impairment.
- **MFSK16**, which acquires in its plugin and opts out of the engine's AFC.
- **The playbook**, rules 1 (the swept-applied correction, done first) and 5 (a scan only for coarse
  acquisition).

`grep -rn "fn shift_hz\|analytic" crates plugins` found only `CfoChannel` and `openpulse-core::iq`'s
63-tap FIR Hilbert, whose image would show at 64QAM.

## Twins

- **`CfoChannel::analytic` and `shift::shift_hz`** carry the same algorithm, one in the channel model
  and one in the receiver. The receiver cannot depend on the simulation crate. Moving both into
  `openpulse-dsp` is a refactor for later.
- **SC-FDMA** (`plugins/scfdma`, off the ladder) has the same structure. Whether it refuses a moved
  centre is UNCHECKED.
- **The other OFDM callers** (the CLI, ARDOP, the testmatrix) go through the same plugin entries, so
  they get the acquisition. The full suites run them.
