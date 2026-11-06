---
project: openpulsehf
doc: docs/dev/design/ofdm-carrier-offset.md
status: resolved
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
1. **The told centre:** the search runs around `center_frequency` − 1500 Hz, whatever the engine's
   BPSK settles left, usually within a few Hz.
2. **The coarse grid** (`acquire::acquire`): ±50 Hz around the told offset in 12.5 Hz steps,
   9 candidates, the told one first and then outward. The first candidate whose pilot coherence reaches
   0.9 ends the search (`EARLY_ACCEPT_COHERENCE`); otherwise the best is taken. The window's analytic
   signal is computed once (zero-padded to a power of two), and each candidate is an O(n) rotation of
   it. For each candidate:
   - mix a short window from the burst's start (the timing search's reach plus 9 symbols, about 11 000
     samples). Only the window is ever mixed during the search, never the burst;
   - find the timing **on the mixed window**: the real-valued Schmidl–Cox metric falls as
     cos(πΔ/31.25 Hz) and is zero at ±15.6 Hz, so timing must come after the mix;
   - score the pilots' coherence across symbols, |Σ a*b| / Σ |a||b|. A wrong offset puts data, not
     pilots, in the pilot bins.
3. **The fine part:** the best candidate's pilot drift (`channel::pilot_drift`), then one more pass at
   the sum.
4. **One exact mix of the whole burst**, unless the acquired offset is under 0.25 Hz (leakage under
   −45 dB), (`shift::shift_hz`: an analytic signal by FFT, times
   e^{j2πft}), then the existing receiver. When no candidate finds a preamble with pilots, the burst is
   not mixed at all: there is nothing to decode, and a 26 s burst of another mode would otherwise cost
   a full-length FFT pair.
5. **Logged:** `OFDM: carrier offset acquired` at debug, with the told and acquired offsets.

`demodulate`, `demodulate_soft` and `estimate_snr_db` all go through step 1–4. The SNR the ladder
reads is therefore measured after the correction; an uncorrected offset would cut it.

**The constants, what they were set from, and what would prove them wrong.**
- **`COARSE_STEP_HZ` 12.5:** from two limits of the code, then checked at its falsifier.
  - Its half (6.25 Hz) stays inside the pilot drift's ±13.9 Hz and away from the timing null at
    ±15.6 Hz.
  - **The timing gate** is on M = P²/R² ≥ 0.5, so it scales as cos²: 0.65 of clean at 6.25 Hz. (The
    first draft said 81 %, which is the cos, not the cos²; the review caught it.) By that formula a
    frame halfway between grid points fails below about 9 dB in-band SNR.
  - **Measured at the falsifier**, offsets halfway between grid points (+6.25, −18.75, +31.25 Hz),
    8 noise seeds each:
    - SL10 near its floor (noise 0.05) and SL14 (0.02): **8/8 with this step**, the same as with a
      6.25 Hz step;
    - at noise 0.1, SL10 fails on carrier too, so the frame dies before the grid matters.
  - A 6.25 Hz step would double the search for a loss not seen anywhere a frame decodes at all.
  - **Still falsified by** an on-air OFDM frame that fails halfway between grid points and decodes on
    the grid.
- **`COARSE_SPAN_HZ` 50:**
  - codec2 DATAC's burst search covers ±50 Hz;
  - untrimmed 2 m rigs were 54–64 Hz apart (`openpulse-channel` `cfo.rs`), and the engine's BPSK
    settle normally moves the centre first.
  - **Falsified by** an on-air OFDM frame that fails with the BPSK rungs decoding at the same time
    (the centre known) and the OFDM offset more than 50 Hz from it.
- **`SCORE_SYMS` 9:** the pilot drift reads at most 8 symbol pairs.

## The HARQ regression the pre-push run found, and its fix

The first version wiped out HARQ combining on SL9 over a Watterson fade
(`harq_combining_survives_the_production_capture_entry`):

| Version | Rescued | Combining rate | Time |
|---|---|---|---|
| Acquisition off | 42 | 0.953 | 7.4 s |
| First version | 0 | 0.660 | 120 s |

**The mechanism**, found by deleting suspects one at a time:
- **Not wrong coarse picks.** On 60 faded bursts with a true offset of 0, acquisition read within
  ±1 Hz every time.
- **It was the mix itself.** Skipping mixes under 1.5 Hz brought back 29 rescues.
- **How the mix did it:**
  - the test feeds exact digital zeros before and after the burst;
  - a 0.5–1 Hz mix leaves the frame's Hilbert tails in them, 1–4 × 10⁻³ RMS (−35 to −45 dB);
  - HARQ demodulates the whole burst from offset 0, and the Minn timing metric, normalised by its own
    window's energy, locked to that residue;
  - standalone attempts start inside the frame, so they were untouched.

**Fixed at the timing search**, not at the mix (`demodulate.rs`, `TIMING_ENERGY_FLOOR` = 0.01): a lag
whose window energy is more than 20 dB below the strongest in the search cannot be the preamble. That
closes the class for any residue, quiet sound cards included. The function's own comment records
that a quiet card exposed the trailing-edge case before.

**Result:**

| Version | Rescued | Standalone | Combining | Time |
|---|---|---|---|---|
| Acquisition off | 42 | 0.673 | 0.953 | 7.4 s |
| After the fix | 41 | 0.680 | 0.953 | 27 s |

The extra time is the full grid, which the faded frames that fail pay on every attempt (below).

## The pilot-coherence inventory (what `EARLY_ACCEPT_COHERENCE` was set from)

The setup:
- 40 Watterson `moderate_f1` bursts per row, with carrier offsets 0, +7, −18 and +33 Hz in turn;
- each rung's own frame and FEC;
- `acquire` run on the raw burst;
- "wrong" means more than 3 Hz from the truth.

| Rung | SNR | Found | None | Wrong | Coherence p10 / p50 / p90 |
|---|---|---|---|---|---|
| SL7 | 5 dB | 8 | 32 | 2 | 0.18 / 0.88 / 0.90 |
| SL7 | 10 dB | 30 | 10 | 8 | 0.16 / 0.90 / 0.95 |
| SL7 | 20 dB | 40 | 0 | 0 | 0.90 / 0.94 / 0.98 |
| SL9 | 5 dB | 22 | 18 | 5 | 0.16 / 0.84 / 0.91 |
| SL9 | 10 dB | 38 | 2 | 5 | 0.23 / 0.92 / 0.95 |
| SL9 | 20 dB | 40 | 0 | 0 | 0.91 / 0.95 / 0.99 |
| SL14 | 5 dB | 21 | 19 | 4 | 0.24 / 0.86 / 0.94 |
| SL14 | 10 dB | 38 | 2 | 4 | 0.29 / 0.90 / 0.97 |
| SL14 | 20 dB | 40 | 0 | 0 | 0.91 / 0.94 / 0.99 |

- Noise alone, 20 bursts per rung: **nothing found**.
- Wrong picks come with low coherence, on deeply faded frames that no offset decodes. Without
  acquisition, those 7, −18 and 33 Hz off would not have decoded at all.
- At 20 dB the p10 sits at 0.90, so 0.9 early-accepts the typical clean frame after one timing search.
- **Falsified by** a frame that early-accepts a wrong candidate at coherence ≥ 0.9: none in this
  inventory, but it is one channel model.

## Measured after the fix (x86, release)

- **Production path** (the probe): SL7, SL8, SL10, SL12 and SL14 at 0, +1.5, −2.5, +5, −7.5, +10, −15,
  +20, −30, +40 and −50 Hz: **55 of 55 decoded, each in 1 attempt.**
- **Pinned test** (`ofdm_carrier_offset_ota`): SL7, SL10 and SL14 at +2.5, −7.5, +15 and −40 Hz:
  - 12 of 12 in 1 attempt each;
  - the SNR read is within 1 dB of on-carrier at every rung (SL7 6.0–7.2 against 6.6 dB, SL10
    10.5–11.4 against 10.2, SL14 9.0–9.5 against 9.1).
- **Cost** (x86, release, a 2.3 s burst):
  - the raw soft demodulation alone is 0.56 ms;
  - the full search was 4.7 ms per call before the cuts: 9 timing searches, 9 rotations at 0.19 ms,
    an analytic signal at 0.5 ms, and a 1.5 ms full-burst mix;
  - with early accept, a clean frame near the told offset costs one timing search;
  - a failing faded frame still pays the grid, which is why the HARQ test takes 27 s against 7.4 s.

  The review counts up to 40–50 attempts per burst in the worst case (2 candidates × attempt 0, the
  ranked onsets and the walk). At about 4× on a Pi 4 that is about 1.5 s, inside the 6 s ACK
  deadline. UNCHECKED on the Pi; the next on-air task measures it there.

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
- **The engine's corrected pass:** with no estimate from OFDM, the engine's settle leaves its
  correction at 0, so no corrected pass runs and phase 2 yields nothing for OFDM (review, from
  `engine.rs` `afc_mini_settle` and `worth_retrying`). The plugin's own acquisition covers ±50 Hz.
- **Telemetry:** the engine's `last_afc_offset_hz` is `None` after an OFDM decode, so the daemon's
  AFC telemetry reads 0 there. The acquired offset is in the plugin's debug log instead. Carrying it
  to the engine would change the plugin trait (a mandatory review); it is parked.
- **The peer's BPSK bursts:** a BPSK250 preamble line sits on OFDM52's pilot bin 50 and repeats every
  128 samples. On those bursts every candidate "finds" a coherent pilot. That costs an attempt and a
  wrong mix on a burst that cannot decode; FEC and CRC stop a false decode (review).

## Not changed, and found on the way

- **The SNR the OFDM receiver read was low on a clean channel**, before and after the offset fix.
  **Fixed 2026-11-03** (the maintainer's go):
  - **The mechanism, measured:** `equalized_data_symbols` equalised every symbol in the slice it was
    given, and the OTA slice runs past the frame. The same 16QAM frame read 41.6 dB alone and 6.1 dB
    with one second of noise after it; SL7 read 31.5 and 2.6 dB.
  - **The fix:** count the frame's symbols from its length prefix (`frame_symbol_count`), and keep
    every symbol when the prefix does not decode. This bounds both the SNR estimate and the display
    scatter.
  - **What it changes on the ladder:**
    - SL7 read 6.6 dB on the production path against a 12 dB climb ceiling, so `climb_on_snr` never
      fired on an OFDM rung;
    - SL14 is admitted only after an SNR upgrade candidate (`ack_up_requires_snr_candidate_at`), so
      the `fast` ladder could not have reached its top rung at all.
  - **After the fix**, on the production path over a 0.002 floor, every rung clears its climb
    ceiling: SL7 16.5 dB against 12, SL8–SL13 37–42 dB against 14–22.
  - **Tests:** `ofdm_snr_clears_the_climb_ceiling` (`openpulse-modem`) and `snr_ignores_the_tail`
    (`ofdm-plugin`). Sabotage, with the bound removed: SL7 reads 7.1 dB and fails; the plugin test
    reads 2.7 against 31.5 dB and fails.

## Tests

- `harq_combining_survives_the_production_capture_entry` (existing): 41 rescued, the guard that found
  the regression.

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

## Review

Fable, 2026-11-03 (`docs/dev/reviews/review-ofdm-carrier-offset-2026-11-03.md`): no blocker, does
not block Release 1. Its findings are taken above:
- the cos² of the timing gate (measured at the falsifier);
- the full-burst mix before any preamble (removed: only the window is mixed until a preamble is
  found);
- the lost observability (a debug log; the telemetry gap parked);
- the SNR mechanism.

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
