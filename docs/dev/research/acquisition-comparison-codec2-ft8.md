---
project: openpulsehf
doc: docs/dev/research/acquisition-comparison-codec2-ft8.md
status: review
last_updated: 2026-11-03
---

# Acquisition: what codec2's OFDM modem and WSJT-X's FT8 do that we do not

Asked by the maintainer on 2026-11-03: can we learn from other modems how to acquire and lock better,
and are we overlooking something? This note compares our receive chain, step by step, with two open
modems that are proven on HF.
- **codec2/FreeDV OFDM**, the DATAC data modes in burst mode, at `310777b` (2026-03-17). Mercury
  (`5bc3e4c`) runs these modes unchanged.
- **WSJT-X FT8**, with FT4 and Q65 for contrast, at `2b9d654` (2024-03-12).

The notes behind it cite file and line for every claim:
- the codec2 notes cover `ofdm.c`, `ofdm_mode.c`, `freedv_api.c` and the README files;
- the FT8 notes cover `sync8.f90`, `ft8b.f90`, `ft8_decode.f90`, `ft4_decode.f90` and `q65.f90`.

Nothing here was taken from closed software.

Each item ends in a proposal with a test that could prove it wrong. Every proposal is measured on our own
fixtures before anything changes; none is adopted because a reference does it.

## Summary

1. **New, measured: the OFDM rungs (SL7–SL14) have no carrier-frequency correction.**
   - Through the production entry, a frequency offset of 2.5 Hz already loses SL14, 5 Hz loses SL10,
     and 10 Hz loses SL7.
   - Rigs are 4 Hz apart on 2 m after trimming, 54 Hz untrimmed, and commonly 10–20 Hz on HF.
   - The `fast` profile is in Release 1 and has never been on air. **This is a Release 1 risk.**
2. **Our only frame detector is a full decode.**
   - Both references gate on a cheap correlation metric with a threshold before they decode anything.
   - Every onset and frequency we guess costs a demodulation plus RS. That is the mechanism behind the
     whole acquisition cost history, the 139-onset sweep included.
3. **We estimate the frequency once, from the squared signal at the trigger point.** codec2 searches time
   and frequency together on the known preamble, and FT8 searches an exhaustive but cheap grid. Our
   wrong settles (#1021; the trigger settle −12.8 Hz against a +3.2 Hz carrier) come from that one
   estimate.
4. **Already aligned:**
   - a time budget per decode (our ACK deadline; FT8's wall clock);
   - never searching the same samples twice (our `abandon_burst`; codec2's buffer reset);
   - ranking candidates (our ranked onsets; FT8's sorted candidates).
5. **Do not copy:**
   - codec2's fixed-SNR LLRs, its amplitude-estimate bug and its never-reset `mean_amp`;
   - FT8's 600-candidate exhaustive decode, which fits a 15 s cycle, not a 9 s ACK window.

## 1. The OFDM rungs have no frequency correction (measured)

**What the code does.**
- The OFDM plugin publishes a frequency estimate (`estimate_afc_hz` → `channel::estimate_cfo_hz`), so
  the engine's settle measures the offset.
- The engine applies a correction by moving the demodulator's centre frequency.
- But the OFDM demodulator refuses any centre more than 1 Hz from 1500 Hz (`plugins/ofdm/src/lib.rs`,
  `demodulate` and `demodulate_soft`: `center_frequency … must be 1500.0 Hz`).

So the estimate can never be applied: every corrected attempt is a configuration error, and only the
uncorrected one can decode.

**Measured** (2026-11-03, x86 release):
- **Setup:**
  - one `fast`-profile ladder frame at each rung, shifted exactly by an FFT bin shift, with 0.002 RMS
    noise and quiet around it;
  - fed in 800-sample reads through `accumulate_capture` → `ota_decode_burst`, the daemon's path.
- **Probe:** scratch code, not committed. Proposal 1 turns it into a test.

| Rung | Decodes at | Fails at | Attempts when it fails |
|---|---|---|---|
| SL7 `OFDM52` | −7.5 … +7.5 Hz | ±10 Hz | 15–45 |
| SL10 `OFDM52-32QAM` | −2.5 … +5 Hz | −5, ±7.5 Hz | 15–30 |
| SL12 | −1 … +2.5 Hz | (not probed further) | |
| SL14 `OFDM52-64QAM` | 0 … ±1 Hz | ±2.5 Hz | 30 |

**Why it fails: inter-carrier interference.** This is an estimate; the bench has not tested it.
- The subcarriers are 31.25 Hz apart. An uncorrected offset ε, as a fraction of the spacing, leaks about
  (πε)²/3 of each carrier's power into its neighbours.
- At SL14's 2.5 Hz that is −17 dB, below what 64QAM needs. At SL7's 10 Hz it is −5 dB.
- The pilots correct the phase of each symbol, but not the leakage, which happens inside the FFT.
- Only correcting the frequency **before** the FFT removes it.

**What codec2 does.**
- It searches the preamble jointly over ±50 Hz in 5 Hz steps, then refines to 1 Hz.
- It mixes the samples by that offset before the FFT.
- It tracks the frequency with the phase difference between pilot rows (loop gain 0.1 per frame, pull
  range ±6.25 Hz).

**What the offsets are on air.**
- On 2 m, after the per-station dial trim: 3.6–4.2 Hz (task 4's logs: the carrier line and the AFC correction). Untrimmed,
  the FT-991A was 54 Hz off.
- At 144 MHz, 0.5 ppm of reference error is 72 Hz, and a rig drifts several Hz as it warms.
- On HF, 10–20 Hz between two rigs is common.

**Consequences:**
- with today's code the `fast` profile reaches SL7 on a trimmed 2 m link, and probably not SL10–SL14;
- on HF, SL7 too is at risk;
- the ladder demotes, so nothing is lost, but the top half of Release 1's ladder would not be usable;
- the fast profile has never been on air, so this has not shown up yet.

**Proposal 1 (Release 1 risk: the maintainer decides whether it gates M3 or M4).**
- **The fix:** correct the frequency before the OFDM FFT. Mix the real samples down by the engine's
  correction (Hilbert shift) and demodulate at 1500 Hz. Two places it could go:
  - at the engine, for any plugin that cannot move its centre. That is one place, so a second such
    plugin would not repeat the gap.
  - in the OFDM plugin, accepting `center_frequency ≠ 1500` by mixing.
- **Then** check whether the existing estimate (`estimate_cfo_hz`) is good to about 1 Hz, which SL14
  needs. codec2 refines to 1 Hz on the preamble and then tracks on the pilots; a tracker may be needed
  for the 64QAM rungs.
- **What would prove it wrong:**
  - the probe, made a test, with every rung decoding at ±20 Hz (each rung's own floor, with noise);
  - with the mixing removed, the same test fails at ±2.5 Hz on SL14.
- **Also UNCHECKED, from reading the code:** the OFDM timing search is a real-valued Schmidl–Cox, which
  by derivation has nulls near ±15.6 Hz of offset. The test above would show it.
- **Review:** it changes a DSP path on every OFDM receive, so a design note, then the review (the
  constant question: what inventory sets the 1 Hz).

## 2. Our only frame detector is a full decode

**What both references do.** Each gates on a cheap metric with a threshold before any decoding:

| | Detection metric | Threshold | Gate before decoding |
|---|---|---|---|
| codec2 | normalised correlation \|c\|²/(E_template·E_rx), 0–1, of the received window against the known preamble and postamble | per mode, 0.08–0.5, set by simulation | the unique word, then CRC16 |
| FT8 | Costas-tone power over the off-pattern tones in the same slots: a self-normalising ratio, divided by the band's 40th percentile | 1.3–2.0 | more than 6 of the 21 sync symbols must match the Costas pattern; only then LDPC |

**What we do.**
- A burst is opened by energy (squelch, or the #1454 spectral test).
- After that, a frame is "found" only when a demodulation plus RS plus magic and CRC succeeds.
- So every guess at an onset or a frequency costs a full decode.
- The cost history follows from this:
  - the 139-onset sweep (8–10 s per burst, task 12);
  - #1118 (98 minutes);
  - #1138 (116 s);
  - the 79 s fragment;
  - 6822 fallback attempts;
  - a2d's 68 attempts.

  Each fix since has bounded or reordered the guesses. None has made a guess cheap.

**What we have.**
- **BPSK250** publishes a preamble template, with a correlation threshold of ρ 0.315 (decision 23,
  #1062). It is used for the settle veto and to rank the fallback's onsets.
- **BPSK63 and BPSK100** carry the same PN-63 preamble but publish no template: the BPSK250 constants
  were derived for its baud and do not transfer. This is parked in the work plan.
- **QPSK250-D** has no template, after the QPSK ρ threshold was withdrawn (#1053).
- **MFSK16** already works like FT8: a Costas share of the energy, with a lock threshold of 0.57.

**Proposal 2 (post-release unless task 20 shows decode cost again).**
- **The change:** a template and a correlation gate for every PN-63 rung (BPSK63, BPSK100), derived per
  baud as #1062 did for BPSK250. The ladder scan then:
  - tries onsets in correlation order;
  - **does not decode** where the correlation is below the rung's threshold.
- **What would prove it wrong:**
  - a frame at its rung's SNR floor whose preamble falls below the gate, which would lose a frame the
    walk finds today;
  - #1062's F1/F2 columns (non-inferiority at the floor; the interferer ceiling) are the existing
    measurement for this.
- **Why it waits:** the energy-rise ranking (`87ce9b6`) took a2d from 68 attempts to 3 on a strong
  signal. The gate is for the weak-signal regime, where the energy rise cannot see the frame, and for HF.

## 3. Frequency from one estimate, at the trigger point

**What the references do.**
- **codec2** finds time and frequency **together** on the preamble: coarse 4 samples × 5 Hz over
  ±50 Hz, then fine 1 sample × 1 Hz. It then tracks on the pilots.
- **FT8** searches an exhaustive coarse grid of 3.125 Hz bins × 40 ms steps with a power-only metric,
  which is cheap. It then refines each candidate in sequence: time ±50 ms, then frequency ±2.5 Hz in
  0.5 Hz steps, then time again.

**What we do.**
- The BPSK rungs estimate the frequency once, at the trigger point: a Goertzel scan of the squared
  signal over ±400 Hz in 12.5 Hz steps, then a squaring fine estimator.
- Phase 2 settles every 4 symbols and takes the median.
- The recorded failures of this are:
  - settles on noise (#1021, a 78/77 livelock);
  - the trigger settle −12.8 Hz against a +3.2 Hz carrier (`ota-scan-onset-major.md`), which failed the
    corrected pass on 129 onsets.

**A recorded elimination is in the way.** "codec2's frequency-first search order" is recorded as ruled
out, but for the alternating `--++` preamble. The PN-63 preamble (flag day 2026-10-10) changes that
premise:
- a PN sequence correlates sharply in time;
- a correlation over a frequency grid on it is the codec2 method.

Re-opening an elimination needs the adversarial review first (decision 8).

**Proposal 3 (post-release).** Once the onset is ranked (proposal 2's correlation peak), estimate the
frequency **from the preamble correlation over a frequency grid at that onset**, not from the squared
signal at the trigger. codec2's ±50 Hz, 5 Hz, then 1 Hz is the starting grid.
- **What would prove it wrong:** the wrong-settle fixtures (#1021, #1045, #1049, F6's
  `f6_capture_class_fixtures`) and the trigger-settle case; the change must settle within 2 Hz on
  each, and must not pass a steady tone.

## 4. Where we already match, and the reference ideas we already have

- **A time budget per decode:**
  - FT8 stops at fixed wall-clock times in its 15 s cycle (13.4 s and 14.3 s);
  - we stop each decode at the ACK deadline (`4fd5a13`);
  - FT4's "the nominal time window first, stop at the first decode" is our onset ranking plus the
    first-decode exit.
- **Never search the same samples twice:** codec2 zeroes its buffer after a postamble pass; we drop the
  burst at our own keying (`abandon_burst`).
- **Ranking:** FT8 sorts its candidates and drops neighbours that are within 4 Hz and 0.04 s of each
  other; we rank onsets (`87ce9b6`, #1454).
- **The decimation and caching codec2 lacks:**
  - codec2's preamble search runs as a plain loop at 8 kHz, costing about 75 M complex MACs per second
    while idle; Mercury measured 100 % of a Pi 4 core and moved to an FFT correlator.
  - FT8 computes one large FFT per pass and cuts each candidate's band out of it.

  If proposal 2's correlation is computed per onset, it should be done with an FFT, as FT8 does.

## 5. Do not copy

- **codec2:**
  - LLRs with a fixed Es/N0 of 3.0 (`freedv_700.c`, marked "further work");
  - an amplitude estimate whose first branch has an operator-precedence slip and is overwritten two
    lines later;
  - an amplitude average that starts at 0 and is never reset between bursts (by derivation, the first
    packet's LLRs come out about 8× overconfident);
  - its unique word is a weak gate for some modes (random bits pass with probability 0.055–0.105).
- **FT8:** the full refinement and decode of up to 600 candidates, three passes with subtraction. That
  fits a 15 s cycle with a known start time, not an ARQ burst with a 9 s ACK window.

## 6. Not yet exercised on air, so not yet known

- **OFDM SL7–SL14:** never on air. Every on-air run used `robust` (SL1–SL6). Section 1 is the first
  evidence about them, and it is from the bench.
- **The OFDM rungs against sample-clock offset:** stations A and B differ by +74 ppm (task 8).
  `soft_concatenated`, the FEC of SL7–SL10, fails in process from about ±150–200 ppm, and has failed
  in all 14 on-air reports since June on the off-ladder `BPSK250|soft_concatenated` case.
- **MFSK16 (SL1):** no data frame decoded on air, and no record of its K=3 return channel on air.
- **Fading:** SL2–SL6 have run only on a non-fading 2 m path with rigs trimmed close together. HF is
  the first fading test.

## 7. The records disagree about the 139-onset episode

The work plan's M3 line and task 12's review blame `decode_burst_inner`'s phase 1 (139 onsets about
20 Hz off). The reviewed design note `ota-scan-onset-major.md` says:
- the OTA trigger settle read −12.8 Hz against a +3.2 Hz carrier;
- the corrected pass failed on 129 onsets.

The OTA path does not call `decode_burst_inner`, so the design note matches the code. Which offset was
right is UNCHECKED. The work plan line should be corrected once it is checked.

## 8. VARA

The maintainer may study VARA in a separate, non-public session that returns only a description. This
project's rule for what comes back is the same as for this note:
- a description of a method, not code or constants;
- treated as a hypothesis and measured on our own fixtures.

The cleanest source is VARA's **signal**: record a VARA HF burst with the RSPdx and measure its
preamble, pilot layout and symbol timing. Studying how a program behaves is always allowed to a lawful
user (§69d(3) UrhG); decompiling is allowed only for interoperability (§69e).

## Proposals, ranked

| # | Proposal | Release 1 | What would prove it wrong |
|---|---|---|---|
| 1 | Correct the frequency before the OFDM FFT (mix at the engine or the plugin), then check the estimate is good to ~1 Hz; a pilot tracker if 64QAM needs it | **Risk:** `fast` SL7–SL14 is in Release 1 and loses SL14 at 2.5 Hz today. Maintainer decides | Every OFDM rung decodes at ±20 Hz at its floor; with the mixing removed, SL14 fails at 2.5 Hz |
| 2 | A preamble template and correlation gate for BPSK63 and BPSK100; rank by it and skip decoding below it | Post-release, unless task 20 shows decode cost again | #1062's F1/F2 columns at each rung's floor; a frame the walk finds must not be gated out |
| 3 | Frequency from the preamble correlation over a frequency grid at the ranked onset | Post-release; re-opens an elimination, so it needs a review first | The wrong-settle fixtures and the trigger-settle case settle within 2 Hz; a steady tone still fails |
| 4 | Correct the work plan's account of the 139-onset episode | Docs | The OTA code path, and the recorded settle value |

## Consumer

The maintainer's decision on proposal 1 (work plan M3/M4) and the parking entries for proposals 2 and
3. Found with `grep -n "fast" docs/dev/project/release-1.0-criteria.md` (line 11: the ladder with
its `fast` and `robust` profiles is Release 1).

## Prior art

- `docs/dev/research/references.md`, which catalogues codec2/Mercury, GNU Radio and others;
- `docs/dev/research/reference-mining-plan.md` (T2.1, a dedicated frequency-acquisition stage, still a
  proposal);
- `docs/dev/research/ofdm-research.md`;
- `docs/dev/dsp-acquisition-playbook.md`;
- `docs/dev/research/wsjtx-analysis.md`.

`grep -rn "center_frequency.*1500" plugins/ofdm` shows the refusal. `grep -rln "OFDM.*offset\|cfo"
docs/dev/research docs/dev/design` found no prior note of the OFDM frequency gap.

## Twins

- **SC-FDMA** (`plugins/scfdma`, off the ladder) has its own `estimate_cfo_hz`. Whether it refuses a
  moved centre too is UNCHECKED.
- **The OFDM paths outside the daemon's OTA receive** (the CLI, ARDOP, the monitor) were not probed.
  They share the plugin's refusal, but how much offset they tolerate is UNCHECKED.
