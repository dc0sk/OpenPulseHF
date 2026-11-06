---
project: openpulsehf
doc: docs/dev/design/ofdm-preamble-detect.md
status: review
last_updated: 2026-11-06
---

# A level-independent carrier detect for the OFDM rungs: open on the preamble, hold on the cyclic prefix

Work plan M3, A2 on the `fast` profile. Found in on-air task 27, run f2g; reviewed by Fable on
2026-11-06 (task 28's "Why"). **Design only: nothing here is implemented.** It changes the plugin
trait, so it is a mandatory design review before implementing (CLAUDE.md, adversarial review). The
design review (APPROVE WITH CHANGES) is
[`review-ofdm-preamble-detect-2026-11-06.md`](../reviews/review-ofdm-preamble-detect-2026-11-06.md);
its required changes are applied below.

## Problem — measured

On 2 m, B→A, station A (IC-9700, AGC FAST, a Release 1 operating requirement:
`dcd-raised-floor.md`) never gathers B's OFDM52 (SL7) frames into a burst. Offline they decode, 6/6,
from `docs/dev/test-reports/on-air/rx-audio-2026-11-01/rx-a-f2g-8k.wav.gz`.
- **Under the AGC a frame is not louder than the gap.** Total power is 1.15–1.22× the gap hiss,
  0.82–0.96 of the squelch. In the probe below, three of the six frames' RMS (0.0058) sits *under*
  the hiss 10 s before them (0.0078–0.0080).
- **Per 62.5 Hz band** the frame is a median 1.2× its floor; the spectral test opens at 4.5 and holds
  at 2.5 (`openpulse-dsp/src/noise_floor.rs`, `S_OPEN`, `S_HOLD_MEAN`).
- The carrier detect is total power OR that spectral test (`ModemEngine::update_dcd_at_seam`,
  `accumulate_routed`, `crates/openpulse-modem/src/engine.rs`). Neither can see these frames, and no
  level statistic can: the reviewer measured the idle hiss's own 64 ms band power at p99 1.66–1.87× its
  mean against an OFDM step of 1.02–1.45×.
- **Correction to the frame windows quoted in the brief.** The preambles start 0.77–1.14 s after the
  quoted window starts. Measured preamble starts (this note's probe, ρ argmax): **355.073, 366.880,
  478.236, 490.003, 687.629, 699.440 s** from the recording's start (07:07:00.85 UTC). The frames are
  1.512 s long (12 096 samples: SL7, ≤ 200 B, soft-concatenated), not 3 s.

## Decision

A third carrier test, **P**, OR'd into the existing decision. It is normalised, so the receive level
does not enter it.
- **P opens** on a correlation with the OFDM52 preamble, confirmed by the cyclic prefix of the next
  4 data symbols.
- **P holds** on the cyclic-prefix correlation of the last 8 symbols, on the grid the preamble set,
  and never past the candidate rung's longest coded frame.
- **Total power and the spectral test are unchanged.**

### Where it runs

- **The seam.** `ModemEngine::update_dcd_at_seam`, called once per read from
  `route_audio_stage(PipelineStage::InputCapture)`.
  - It sees the samples after the DC block and the notch, and before the AGC, like total power and S.
  - It sets `seam_p` (held this block) and `seam_p_onset` (the preamble's offset in this block).
    `accumulate_routed` reads them as it reads `seam_s`.
  - Decode slices bypass the seam (`input_prerouted`), so P never runs on them.
- **When it is active (decision, Release 1): only while `ota.rx_candidates()` holds a mode whose
  plugin publishes a detector.** Not the wider arriving set (`rx_mode` ∪ `relay_mode` ∪ candidates)
  that `active_burst_cap_samples` and `active_shortest_preamble` use.
  - On `fast`, that is when A recommends or has confirmed SL7–SL14.
  - `robust` never climbs past SL6 (`ota_rate.rs`, `robust_profile_never_climbs_past_sl6`), so there
    P never runs.
  - **Why not the arriving set:** it would run P for a configured OFDM `rx_mode` on ARDOP, KISS and
    the CLI monitor with no session. There the own-echo rule and the ACK path are twins nobody has run
    under OFDM. Widening is post-release.
- **One detector serves SL7–SL14.** Every `fast` OFDM rung is an OFDM52 variant
  (`profile.rs`: SL7 `OFDM52` … SL14 `OFDM52-64QAM`). `build_preamble` uses only
  `first_sc`/`last_sc`, so the transmitted preamble is identical across them (probe: normalised dot
  1.000000 between `OFDM52` and `OFDM52-64QAM`, different payloads). The engine dedups by template.
- **SC-FDMA, `64qam`, `pilot`:** not on `hpx_hf`, so not needed for Release 1. Each has its own
  preamble and would need its own measured constants (post-release).

### The state machine (per read, in samples, never by clock)

1. **Search.** Correlate the read, plus the previous read's last `SYM_LEN − 1` samples, against the
   template, so ρ scores each offset once. Use `IqMatchedFilter::rho_profile_over_frequency`, energy
   floor 0.05, over a grid around the told offset (`center_frequency − 1500`, as `acquire` uses it).
   - The 5-symbol carry (`5·SYM_LEN`, for an onset whose confirm spans reads) is kept for γ only,
     which is cheap (0.26 ms/s). Carrying it into ρ re-correlates 5 already-scored symbols on every
     read: measured 3.2× the cost (Cost below).
2. **Open** at offset `o` when ρ(o) ≥ `RHO_OPEN` **and** γ₄(o) ≥ `GAMMA_CONFIRM`:
   - γ₄ is the CP correlation of data symbols 1–4 on the grid `o + k·SYM_LEN`;
   - it is computed on the analytic signal, |Σ z[n]·z*[n+256]| / Σ ½(|z[n]|² + |z[n+256]|²), because a
     real-valued lag-256 product falls as cos(2π·Δf·32 ms) and is zero at 7.8 Hz of offset;
   - the decision waits until symbol 4 is in: 1 440 samples after `o`.
   - **No persistence count.** A preamble is a single 36 ms event, so the confirm takes the place
     persistence would have.
3. **Hold** while γ₈ ≥ `GAMMA_RELEASE`:
   - γ₈ is evaluated on each new symbol, over the last 8 symbols on that grid;
   - the hold ends after `hold_bound` samples from the latest open, whatever γ₈ says.
   - **`hold_bound`** is the maximum, over the OFDM candidates, of `frame_plan(max_frame_samples, fec).0`
     (`engine.rs:284`), from the plugin's `frame_geometry` and the candidate's FEC. It is never the
     burst cap. SL7: 6 652 × 4 = 26 608 samples (3.3 s); the longest coded SL7 frame (255 B) is
     23 328. SL14: 2 534 × 2 = 5 068; its 255 B frame is 3 744.
   - `hold_bound` reads the `ota.rx_candidates()` `(mode, fec)` tuples itself.
     `active_burst_cap_samples` and `active_shortest_preamble` discard the FEC, so neither can be
     reused for it.
4. **Re-open.** A preamble found while held re-arms the bound and moves the grid. A keying with
   several frames back to back stays one burst, which `decode_burst_frames` (#1461) handles.
5. **`force_busy`.** While P holds, the seam calls `dcd.force_busy()` as it does for S. CSMA, the
   station-ID quiet clock and the `DcdChange` event then see the channel busy.

### What it interacts with

- **The pre-trigger ring (#1443/#1454).** A P-opened burst does not take the whole ring.
  - It starts at `max(0, o − SYM_LEN)`, taken from the ring when `o` lies in an earlier read, else
    from the current read.
  - The ring (`S_RING_WINDOWS` 16 × 512 = 8 192) covers the decision lag (1 440 + 288) plus a read of
    up to about 6 400 samples. For a longer read, `o` lies inside that read.
  - **Three burst-open sites in `accumulate_routed` must take P's head instead of their own:**
    - `rx_burst_first_block = samples.len()` is set unconditionally (`engine.rs` ~2933);
    - the whole current read is appended (~2943);
    - the `if self.seam_s` branch copies the whole ring (~2921).
  - A burst that total power or S opened keeps its lead; P only records `o`.
- **The onset bound.** For a P-opened burst, `rx_burst_lead = SYM_LEN` and the onset bound handed to
  the decoder is `2·SYM_LEN` (`record_flush_flags` → `burst_onset_scan_bounds`, reach
  max(4·288, 576 + 288) = 1 152).
  - The ρ argmax landed 0–8 samples after the true start on all 62 faded frames that decoded (`moderate_f1`, probe
    p6).
  - The plugin's own timing search (`find_first_data_body`, 8 192-sample reach) places the frame
    finely.
- **The burst cap.** P cannot cause a cap flush: its bound (≤ 26 608) is far under
  `BURST_MIN_CAP_SAMPLES` (240 000). The cap and `commit_band` are unchanged for total power and S.
- **The noise-floor commit.**
  - **While P holds, its blocks are carrier blocks** (`carrier_block` gains `|| p`), so the tracker
    holds rather than learns them.
  - **At a flush,** a burst P opened or held is always `discard`ed, never `commit`ted: it is a frame,
    not band.
  - **Discarding it is unbiased,** unlike the flicker case #1454 S6 measured. P does not select
    windows by level, so leaving them out removes no loud tail from the history.
  - **The cold-start discard (#1254) must not apply to a P-opened burst.** That rule exists for
    bursts the cold squelch judged, and P never consults the squelch. It does apply today:
    `accumulate_routed` discards any burst with `was_cold && mean_sq.is_some()` (`engine.rs`
    ~2991–3000). The burst carries a `p_opened` flag, read there to skip the discard. Gate 1's cold
    case and sabotage S6 pin it.
- **The ACK deadline.** The daemon takes the frame's end as the start of the read that flushed it
  (`server.rs:1135–1146`).
  - P releases 6–8 symbols (0.22–0.29 s) after the frame's real end, measured 6/6 on f2g. So the
    deadline is late by that much plus a read.
  - The tail is an artefact of the 8-symbol window, not of the channel: m symbols past the end, γ₈
    still averages 8 − m frame symbols, so it falls as about (8 − m)/8 of the in-frame value.
  - S already appends up to 0.5 s (`SPECTRAL_TAIL_MAX`), and `ACK_SLACK` is 0.3 s.
  - **Decision:** no sample-exact frame end (`last_flush_tail_samples`); the tail fits `ACK_SLACK`.
    Gate 11 asserts the ACK is keyed in time end to end.
- **HARQ.** A P-opened burst is a confirmed OFDM frame, so retaining its LLRs on failure is what HARQ
  is for. On a false open, the plugin's `acquire` finds no preamble with pilots, so `demodulate_soft`
  errors and nothing is retained. That reading of the code is UNCHECKED, and gate 6 asserts it.
- **The notch.** It runs before P. With an OFDM rung among the candidates it protects the OFDM band,
  about 500–2 500 Hz (`notch-protects-the-ladder.md`), so it cannot remove preamble lines.
- **The #1049 veto, the #1060 calibration and the #1062 stand-down.** P does not touch them. The
  detector is published through a new trait method, not `preamble_template`. Publishing the latter
  would switch on, for OFDM, both the decode-path veto (`build_preamble_veto`, 8 call sites) and
  `ranked_fallback_onsets` (`engine.rs` ~3352, called ~4017 and ~4184). `veto_membership_pin` would
  not notice today: it registers only bpsk, qpsk and psk8 (gate 8 fixes that).
- **D3** (S only while the adaptive squelch is in force). **Decision:** an operator squelch above the
  adaptive one does not switch P off. P is level-independent, and it is the only test that sees
  these frames.
- **Ladder evidence (#1456). Decision:** a failed burst that P confirmed counts as ladder evidence,
  and so can key a NACK, only when the plugin's `acquire` returns coherence ≥ 0.9 on the flushed
  burst. Without it, B→A waits out the 9 s ACK window on every miss. `FlushSpans` gains
  `p_confirmed`.

### Cost

Measured on this host (AMD Ryzen 9 3900X, one thread, `--release`, `rho_profile_over_frequency`):

| Stage | ms per second of audio, 4 096-sample chunks | reviewer's re-measure |
|---|---|---|
| ρ, 9-point grid (±50 Hz / 12.5 Hz) | **16.4** | 17.1 |
| ρ, 15-point grid (±50 Hz / 6.94 Hz) | 27.4 | — |
| ρ, told offset only | 3.1 | 3.2 |
| analytic signal + γ over every block | 0.26 | — |

- **The table is for long chunks, not daemon reads.** At 800-sample reads, correlating each read
  together with a 1 727-sample carry (`SYM_LEN − 1 + 5·SYM_LEN`) measured 4.78 ms per read: 48 ms/s,
  3.2× the table, because every read re-scores 5 symbols. The search therefore carries only
  `SYM_LEN − 1` into ρ (state machine step 1), which brings ρ back to the table's per-offset cost.
- **Pi 5, estimated:** about 5–7 ms per 100 ms tick (5–7 % of one core). **UNCHECKED on the Pi**: the
  next on-air task measures it, from a per-read duration counter the implementation adds.
- P runs only while an OFDM rung is among the OTA candidates, and only on receive.
- An overlap-save FFT correlation would cut the ρ cost if the Pi needs it.

## The preamble reference — by reference, not re-typed

- **What the transmitter sends** is `clip_iterative(build_preamble(p, …))`, then scaled
  (`plugins/ofdm/src/modulate.rs:111`). Its PAPR measures 12.00 dB against 12.69 dB unclipped, but
  ρ(clipped, unclipped) = 0.9987: the clip is not load-bearing. Taking the template by reference is
  still right, because it cannot drift from the transmitter.
- **The decoder's timing stage** uses the unclipped `modulate::preamble_template(p)`
  (`demodulate.rs`, `find_first_data_body`).
- **The probe** took the template as the first `SYM_LEN` samples of `OfdmPlugin::modulate` output,
  that is the clipped preamble. The measurements below are for that template.
- **The implementation** extracts one `pub(crate) fn clipped_preamble(p: &OfdmParams) -> Vec<f32>`,
  called both by `modulate_with_params` and by the detector, so the transmitter and the detector
  cannot drift apart.
- **Test 7** pins it in the default run: the detector's template has ρ = 1.000 against `modulate`'s
  first `SYM_LEN` samples, for two modes and payloads.
- **The grid** is `acquire`'s own `COARSE_SPAN_HZ` and `COARSE_STEP_HZ`, used by the detector in the
  same crate. They are not re-declared.

### Corrections to the brief, found in the code

- **The preamble loads only the 33 even subcarriers of 16–80** (`build_preamble`,
  `sc.is_multiple_of(2)`), not 52. A tone on a line can reach at most ρ ≈ 1/√33 = 0.17.
- **The body is two identical 128-sample halves** (probe: similarity 1.0000), so the 256-sample
  body is 128-periodic. The spectrum is 33 lines at 62.5 Hz spacing. The template's self-ρ at the
  ±128 alias is 0.601, not 1, because the CP breaks the periodicity within the 288-sample window. An
  onset taken from ρ alone is still not sample-exact, which is why P only hands the decoder a bound.

## The constants, the inventory each was fitted to, and what falsifies it

### The inventory (measured by a temporary probe, deleted)

**Positives:** ρ is the best value over the 9-point grid in the frame's preamble.

| Set | Frames | ρ min | ρ max / median | Note |
|---|---|---|---|---|
| f2g, A (IC-9700, AGC FAST), SL7, decoded 6/6 | 6 | **0.840** | 0.883 | told offset only: 0.602–0.686; best grid point −12.5 Hz in 6/6 |
| f1d, B (FT-991A, AGC FAST), SL7–SL9 decoded (B's log: 67) | 68 | **0.697** | 0.790 / 0.741 | 35 OFDM52-8PSK, 32 16QAM, 1 OFDM52 |
| f1d, B, SL10 present but not decoded (pilot coherence ≥ 0.94) | 30 | 0.679 | 0.774 | A sent 30 SL10 frames |
| `moderate_f1`, SL7 64 B, decoded at 6 / 9 / 12 dB (synthetic) | 8 / 20 / 29 of 40 | **0.593** / 0.638 / 0.605 | — | all frames at 6 dB, failed included: 31/40 ≥ 0.50 |
| `moderate_f1`, SL14 1 B (LDPC high-rate), 20 dB, decoded | 5 of 40 | 0.862 | — | |

**Negatives:** the largest ρ in each 64 ms window (512 offsets × 9 frequencies).

| Set | Windows | p99 | p99.9 | Max |
|---|---|---|---|---|
| `ft991a-idle` (3 s) | 47 | 0.387 | — | 0.387 |
| `ic9700-idle-hot` (3 s) | 47 | 0.372 | — | 0.372 |
| `ic9700-idle-wide-500hz-control` (45 s) | 703 | 0.381 | 0.412 | 0.419 |
| `ic9700-idle-500hz` (45 s) | 703 | 0.340 | 0.353 | 0.354 |
| `ic9700-idle-250hz` (45 s; passes the OFDM timing gate 438/438) | 703 | 0.311 | 0.333 | **0.348** |
| `ic9700-tone-1501hz` (2 s; pilot coherence 0.992) | 31 | 0.377 | — | **0.377** |
| `ic9700-frame-bpsk250-{none,rs}{,-whitened}` | 1 594 | ≤ 0.394 | ≤ 0.416 | 0.416 |
| `sdr-ic9700tx-bpsk250-rs-{1,2,3}` | 1 407 | ≤ 0.313 | ≤ 0.345 | 0.345 |
| `ft991a-ota-{frame-bpsk31-rs,fragment-5s,bpsk31-at-sl1}` | 1 937 | ≤ 0.364 | ≤ 0.388 | 0.404 |
| f2g, everything outside the six frames: 1 926 s of hiss and B's BPSK31…QPSK250-D frames | 30 096 | 0.383 | 0.416 | **0.474** (≥ 0.45: 2) |
| f1d, everything outside the OFDM frames: 1 586 s of hiss and A's BPSK/QPSK frames | 24 780 | 0.385 | 0.412 | **0.475** (≥ 0.45: 1) |

- **What the negatives show:**
  - Narrow filters *lower* P's noise ρ (wide 0.419, 500 Hz 0.354, 250 Hz 0.348). That is the opposite
    of BPSK250's #1060, because the template spans 2 kHz and a narrow filter removes most of it.
  - Tones and narrowband frames read no higher than the hiss under them.
- **The CP correlation:**

  | γ | Positives | Negatives (fixed grid; every phase is a trial) |
  |---|---|---|
  | γ₄, data symbols 1–4 | f2g 0.863–0.926; `moderate_f1` decoded min 0.716 / 0.667 / 0.661 (6 / 9 / 12 dB) | wide idle p99.9 0.441, max 0.475 (0/3 697 ≥ 0.50); 500 Hz max 0.733; 250 Hz max 0.812; f2g gaps max 0.940; tone and BPSK250 ≈ 1.0 |
  | γ₈, sliding in-frame minimum | f2g 0.842–0.877; `moderate_f1` decoded min 0.502 / 0.431 / 0.463 | wide idle p99 0.273, max 0.395; 250 Hz p99 0.568, max 0.704 |
  | γ₈ past the frame's end | f2g, 6/6: below 0.30 within 6–8 symbols | — |
  | γ₄ on all frames, ρ ≥ 0.55 (`moderate_f1`, SL7 64 B, 40 seeds each) | under 0.50: 8/38 at 6 dB (min 0.342), 1/38 at 9 dB (min 0.493), 0/40 at 12, 15 and 20 dB | — |

- **CP alone is useless as an opener.** Blind (max over the 288 grid phases), the tone and BPSK250
  read 0.999 and 1.000. This is the reviewer's tone-vulnerability finding, reproduced. CP is
  therefore used only on the grid a preamble set.
- **Not measured:** the brief's 12–15 s excerpts were not replayed through the seam. The probe scored
  whole recordings and ±1 s windows. The gates below do the excerpts.

### The constants

| Constant | Value | Fitted to | Falsified by |
|---|---|---|---|
| `RHO_OPEN` | **0.55** | noise max 0.475 (3 512 s on air + 459 s corpus), decoded min 0.593 (`moderate_f1` 6 dB) / 0.638 (9 dB, SL7's floor) / 0.697 (on air) | a decoded frame under 0.55 on a channel the rung exists for, or a negative ≥ 0.55 that also passes the confirm |
| `GAMMA_CONFIRM` | **0.50** (γ₄) | decoded min 0.661 (fade); noise 0 of 3 697 on wide hiss, max 0.475, so there is no room lower. **Miss on all frames with ρ ≥ 0.55:** 8/38 at 6 dB, 1/38 at 9 dB, 0/40 at 12–20 dB | a decoded frame under 0.50 (gate 3 counts confirm rejections among frames the stack decodes at 9 dB); it rejects only broadband noise, by design: narrowband reads up to 0.99 here and is ρ's job |
| `GAMMA_RELEASE` | **0.30** (γ₈) | decoded in-frame min 0.431 (fade), hiss falls under it within 8 symbols on air | a decoded frame whose γ₈ dips under 0.30 (the burst splits), or band audio that keeps it over 0.30 (the hold runs to `hold_bound`; bounded) |
| `hold_bound` | `frame_plan(max_frame_samples, fec).0`, max over the OFDM candidates | `tx_airtime_seconds` of a 255 B frame per rung | any OFDM rung whose longest coded frame exceeds it: test 5 asserts this from `tx_airtime_seconds`, by reference |
| grid | ±50 Hz, 12.5 Hz (`acquire::COARSE_*`) | f2g: told-only ρ 0.60–0.69, gridded 0.84–0.88; a 6.94 Hz step bought nothing (0.828–0.873) at 1.7× the cost | an on-air OFDM frame more than 50 Hz from the told centre with the BPSK rungs decoding (as `ofdm-carrier-offset.md`) |
| energy floor | 0.05 of the searched span's mean | the #1049 veto's value | P opening on, or missing, a frame at a silence→frame edge; UNCHECKED |
| confirm span | 4 data symbols | the shortest OFDM frame sent: SL14 1 B = 1 440 samples = preamble + 4 | a rung whose shortest frame has fewer than 4 data symbols (zero slack today) |

- **Margins at `RHO_OPEN` 0.55:**
  - 0.075 over the largest noise value measured;
  - 0.088 under the faded decode minimum at SL7's floor, 0.043 at 6 dB;
  - 0.15 under the on-air minimum.
- **Extreme-value caveat.** ρ is a maximum over 4 608 hypotheses per window, and its ceiling grows
  with listening time.
  - The reviewer's count, over 86 177 windows (5 515 s): 273 at ≥ 0.40, 5 at ≥ 0.45, 0 at ≥ 0.50.
  - One 0.05 step costs about ×55, so ≥ 0.55 extrapolates to about one window per 900 h on this
    link's audio mix.
  - That is a two-point tail extrapolation, for 2 m with this traffic only. It is not a measurement.
  - The confirm multiplies it by about 1/3 700 on hiss, but only by about 0.9 while narrowband
    traffic is on the channel (narrowband γ₄ reads up to 0.99).
- **`RHO_OPEN` decision: 0.55, not 0.50.** 0.50 gains 2/40 frames at 9 dB and 0 at 6 dB, and costs
  about ×55 in false-open rate.

### Falsifiers not in the corpus

- **UNCHECKED:**
  - CW on air (synthetic: ≤ 0.394; a stronger tone lowers ρ, 0.383 → 0.199 from +0 to +40 dB);
  - SSB voice;
  - another OFDM modem (VARA, FreeDV DATAC, Mercury). A comb at a 31.25 Hz multiple is the
    plausible false opener;
  - FT8 and RTTY;
  - impulsive QRN and key clicks on air. Synthetic only: 10 clicks/s at +10 dB over hiss measured
    ρ 0.419;
  - OFDM on air on HF.
- **Synthetic only:** OFDM at its floor on `moderate_f1` (the table above). Watterson noise is
  normalised over the frame-only buffer, so SNR and duration are not confounded here.

## Gates (tests) and sabotages

Every gate goes through `accumulate_capture` in daemon-sized reads (800, and also 400, 4 096 and
19 200, the 2.4 s read hold seen on air), then `ota_decode_burst`. The engine holds a `fast` session
locked to SL7 (`ota_lock_level`), so SL7 and SL6 are the candidates.

1. **`ofdm_frames_gather_under_agc`** (the case).
   - Input: the committed f2g excerpts (asset rule below), each with ≥ 9 s of gap hiss ahead.
   - Asserts: each frame flushes as one burst holding the whole frame, and decodes (6/6). Plus one
     f1d excerpt (B, FT-991A, SL8/SL9) for a second rig.
   - **Sabotage S1:** P not OR'd into `carrier_present`. Expected 0/6; task 28 measured 0 gathered on
     the production path.
   - **Cold case:** the same f2g excerpt fed to an engine whose noise floor is cold. The frame must
     still decode. **Sabotage S6:** the `p_opened` check removed from the `was_cold` discard. The
     burst is discarded, and it must fail.
2. **`ofdm_detect_ignores_the_corpus`.**
   - Input: every idle capture, the tone, the BPSK250 and BPSK31 captures, and one f2g QPSK250-D
     excerpt, with an OFDM candidate active.
   - Asserts:
     - `preamble_detect_opens()` stays 0 while `preamble_detect_blocks_processed()` moves;
     - the flush sequence is bit-identical to the same feed with no OFDM candidate (P off).
   - **Sabotage S2:** `RHO_OPEN` 0.40 with the confirm off. It must open on
     `ic9700-idle-wide-500hz-control` (3 windows ≥ 0.40 measured).
3. **`ofdm_frame_gathers_off_grid_and_faded`.**
   - Input: SL7 at 9 dB on `moderate_f1`, carrier offsets +31.25 and −43.75 Hz (halfway between grid
     points), embedded at a fixed sigma in recorded hiss, at a frame level 1.15× the hiss.
   - Asserts: P gathers at least what the decoder can decode.
   - Counts confirm rejections (ρ ≥ 0.55, γ₄ < 0.50) among the frames the stack decodes at 9 dB, and
     reports them. Measured on all frames: 1/38.
   - **Sabotage S3:** the told offset only, no grid. ρ at 31 Hz is about sinc(1.1) and must fail.
4. **`two_ofdm_frames_stay_two_bursts`.**
   - Input: two SL7 frames 0.4 s apart at AGC level.
   - Asserts: two bursts at 800-sample reads. At 19 200-sample reads they are one burst carrying two
     frames, and both decode. That case is stated as a limit, not hidden.
5. **`ofdm_hold_is_bounded_by_the_frame`.**
   - Input: a genuine preamble followed by a steady tone (γ₈ ≈ 1).
   - Asserts:
     - the burst flushes at `hold_bound` + one read, not at the 240 000-sample cap;
     - `hold_bound` ≥ `tx_airtime_seconds(255 B)` for every OFDM rung.
   - **Sabotage S4:** bound = burst cap. It must fail.
6. **`a_detected_burst_is_not_learned_as_band`.**
   - Asserts: the noise floor is unchanged across a P-opened burst, and the HARQ retained count is
     unchanged after the gate 2 corpus.
   - **Sabotage S5:** `commit` instead of `discard`. The floor moves, and it must fail.
7. **`the_detector_template_is_the_transmitted_preamble`** (default run).
   - Asserts: ρ = 1.000 against `OfdmPlugin::modulate`'s first `SYM_LEN` samples, for `OFDM52` and
     `OFDM52-64QAM` with different payloads.
8. **`veto_membership_pin`** (existing, extended): P must not switch on the OFDM decode veto.
   - Today it is vacuous for OFDM: `fn plugins()` registers only bpsk, qpsk and psk8, so an OFDM
     `preamble_template` would leave it green.
   - The change registers `ofdm-plugin` in the pin and keeps `MODES_WITH_VETO = ["BPSK250"]` (or
     asserts `!preamble_veto_active("OFDM52")` with OFDM registered).
9. **The tripwire on the production path.**
   - `preamble_detect_blocks_processed()` and `preamble_detect_opens()` (`instruments`).
   - A twin-daemon test (`server::run`) with an SL7 session asserts that both move.
10. **The slow suite** (`scripts/slow-tests.sh spectral`) re-run: the S decode counts must not move.
11. **The ACK path, end to end.** The ACK is the point of the feature, and gates 1–10 do not cross it.
    - A twin-daemon test (`server::run`) with an SL7 session, B's frame at AGC-like level (1.15× the
      hiss).
    - Asserts: A keys the ACK before the deadline.
12. **The own echo.** rx-a-f1d holds A's own OFDM frames at ρ 0.907–0.942, 98 times. P stays off
    them only because of the daemon's #1007/task-17 rule (`rx_stream = None; rx_pending.clear();
    abandon_burst()`, `server.rs` ~951–957 and ~1521–1525).
    - `abandon_burst_when_keying.rs` runs with an OFDM candidate, so a regression in that rule shows up
      as a P open.

**The asset rule** for committed captures:
- **Format:** 8 kHz mono 16-bit PCM, cut by sample index from the committed `.wav.gz` (no resampling).
- **Size:** ≤ 410 KB per file, ≤ 1.25 MB for this feature. That is three f2g episodes of two frames
  each (about 25 s each), plus one f1d excerpt of about 12 s.
- **Provenance:** a README row per file in `tests/captures/README.md` with the source file, the
  sample range and UTC.
- **Check:** `the_corpus_loads_and_is_at_the_modem_sample_rate` gains a size assertion.

## Risks

- **False opens hold the channel busy.**
  - Each one holds at most `hold_bound` (3.3 s at SL7), during which CSMA defers and the station-ID
    quiet clock waits.
  - **Measured rate:** 0 at 0.55 (or 0.50) over 3 512 s of on-air audio and 459 s of corpus;
    extrapolated about one ρ window per 900 h before the confirm (the extreme-value caveat above).
  - **The worse cost is a keyed NACK on noise** (the #1178 class), since a confirmed burst is ladder
    evidence. The `acquire` coherence ≥ 0.9 condition is what stands between the two.
  - The daemon logs a false-open counter, so the HF run measures the rate instead of extrapolating it.
- **CPU on the Pi.**
  - About 5–7 % of a core, estimated (Cost). Receive-only, and only with an OFDM candidate.
  - After a transmit or a decode, the first read can hold 2.4 s of audio, about 0.1 s of P work in
    one tick. UNCHECKED on the Pi.
- **`SPECTRAL_TAIL_MAX` and frames 0.4 s apart.**
  - P adds no tail beyond S's: it releases 0.22–0.29 s after the frame, against S's ≤ 0.5 s.
  - With P alone, frames 0.4 s apart stay two bursts only while a read is shorter than about
    0.11 s (880 samples; derived from the release, not measured). The daemon's 100 ms tick (800)
    qualifies with little margin; a 2.4 s read hold does not.
  - Where they merge, #1461's multi-frame decode carries both.
- **A deep fade splits a frame:** γ₈ under 0.30 mid-frame. Not seen on the 57 faded frames that
  decoded (minimum 0.431); seen in 4 of 40 at 6 dB, failed frames included.
- **One link, one band.** Every on-air positive is 2 m, one rig pair. HF is UNCHECKED, and Release 1
  confirms there.

## Decisions (design review, 2026-11-06)

The questions this note put to the reviewer, with the answers taken.

1. **Trait shape:** a trait object `CarrierDetector`, defined in `openpulse-core` and implemented in
   `ofdm-plugin` over `IqMatchedFilter`.
   - Core defines it because core must not depend on `openpulse-dsp` (`workspace_layering.rs`).
   - It is narrow: `observe(&mut self, read: &[f32]) -> Verdict` with `Verdict { Idle, Open { onset },
     Held, Released }`, plus `hold_bound_samples(fec)`.
   - The plugin method `carrier_detector(&self, &ModulationConfig) -> Option<Box<dyn
     CarrierDetector>>` defaults to `None`. Not `preamble_template`, for the veto reason above.
   - `PLUGIN_TRAIT_VERSION` 3.1.0 → 3.2.0. The same commit updates the history in
     `docs/dev/plugin-trait-versioning.md`, `contributing-plugins.md` and the `openpulse-book.md`
     version lines, and fixes the stale "framework minor is 0" comment in
     `higher_minor_than_framework_is_rejected`.
2. **Scope:** OTA candidates only (Where it runs).
3. **D3:** an operator squelch above the adaptive one does not switch P off. P is level-independent,
   and it is the only test that sees these frames.
4. **Evidence:** a failed P-confirmed burst counts as ladder evidence, gated by the plugin's
   `acquire` coherence ≥ 0.9 on the flushed burst. Otherwise B→A waits out the 9 s ACK window on every
   miss.
5. **`RHO_OPEN`:** 0.55. 0.50 gains 2/40 frames at 9 dB and 0 at 6 dB, and costs about ×55 in
   false-open rate.
6. **Release:** CP release at 0.30. A shorter release window (the tail is the 8-symbol window's
   artefact) is parked as a one-line follow-up in the work plan.
7. **ACK deadline:** no sample-exact frame end; the 0.22–0.29 s tail fits `ACK_SLACK`. Gate 11
   checks it.
8. **Inventory:** enough to build, not to call done. P ships behind the OTA scope with a false-open
   counter in the daemon log, and HF confirms.
   - What it has: one on-air link, 6 + 98 OFDM frames, and synthetic fades.
   - What it lacks: HF, other OFDM modems, CW and voice on air.
9. **Release 1:** it blocks. With AGC FAST required, B→A SL7 cannot be gathered at all, so A2 on
   `fast` cannot pass B→A.

## Consumer

- **The daemon:** `server::run`'s receive tick calls `engine.accumulate_capture(Some(&mode), samples)`
  (`crates/openpulse-daemon/src/server.rs:1044`), which calls `accumulate_routed` and then
  `route_audio_stage(InputCapture)` → `update_dcd_at_seam` (`engine.rs` 2873 → 9319 → 2488).
- **ARDOP and KISS:** they reach the same path through `CaptureTicker` (`capture_ticker.rs:177`).
- Found with `grep -rn 'accumulate_capture' crates/*/src | grep -v 'openpulse-modem/src/engine.rs'`.

## Prior art

- **#1049 correlation veto, and #1060 CFAR / runtime calibration.** These are the same primitive:
  normalised preamble ρ through `IqMatchedFilter::search_normalized_over_frequency`, with a
  per-mode threshold in `PreambleTemplate`. They run only on the decode path and only for BPSK250.
  P reuses the correlator and the per-mode-constants rule, and keeps clear of the veto.
- **#1062 stand-down (`rho_calibration`).** Not adopted. P's noise ceiling falls with a narrower
  filter (measured above), the reverse of the BPSK case the stand-down exists for. That is UNCHECKED
  on other bands.
- **BPSK250's PN-63 template** (ρ 0.315, ±20 Hz; `pn-preamble.md`). A periodic preamble cannot be
  searched over its own line spacing (sharp edges, #1049 item 2). OFDM52's lines are 62.5 Hz apart,
  and the ±50 Hz grid stays inside ±31.25 Hz of any line, so a tone gains nothing (measured 0.377 on
  the tone capture).
- **The acquisition chain's `EnergyGate` and settle** (`engine.rs`, the scanning `receive` family).
  It is level-based, so the "energy gate is the wrong primitive" lesson (#1020…#1045,
  `lessons-and-archetypes.md` #17) applies to it.
- **Reference modems** (`docs/dev/research/references.md:238` and `:280`):
  - modem73 / aicodix detect OFDM with a self-normalised Schmidl–Cox ("scale-invariant, so a capture
    AGC cannot break it"), confirmed by a frequency-domain MLS kernel with peak/second ≥ 4;
  - ROBUST uses CP autocorrelation plus a per-geometry pilot gate. That is the same open-then-confirm
    shape as here.
  - codec2 DATAC searches the known preamble jointly over ±50 Hz
    (`acquisition-comparison-codec2-ft8.md` §1).
- Found with `grep -n -i 'schmidl\|802.11\|minn\|cyclic prefix' docs/dev/research/references.md
  docs/dev/research/acquisition-comparison-codec2-ft8.md` and `grep -n 'build_preamble_veto'
  crates/openpulse-modem/src/engine.rs`.

## Twins

- **The ARDOP TNC and KISS** (`CaptureTicker` → `accumulate_capture`) share the seam, so they get P
  whenever their OTA candidates hold an OFDM mode. ARDOP runs no OTA session, so under the OTA-only
  scope (decision 2) it never does. UNCHECKED: no test runs them under AGC-level OFDM.
- **The CLI's `receive` family** (`stage_capture_input` → `route_audio_stage` → `EnergyGate`) runs the
  seam, but holds no OTA session, so P does not run there. It decides frame start by `EnergyGate`,
  not by a burst, so P would not help it. Whether CLI `receive --listen-ms` misses OFDM under AGC is UNCHECKED.
- **The twin-daemon harness** goes through `server::run`, so it gets P by construction. Gate 9 runs
  it.
- **The own-echo rule** (`server.rs` ~951–957 and ~1521–1525, #1007/task 17) is the only thing that
  keeps P off a station's own OFDM frames (ρ 0.907–0.942 on rx-a-f1d). Gate 12 runs it with an OFDM
  candidate.
- **`channel_still_busy`** (`server.rs:2671`) judges the NACK mute on total-power RMS alone. Under
  AGC it reads "not busy" during a frame P holds. That is a twin of the carrier detect that this
  design does not change (task 15).
- Found with `grep -rn 'route_audio_stage(PipelineStage::InputCapture' crates/openpulse-modem/src/engine.rs`
  and the Consumer grep.

## Review

Reviewed 2026-11-06, APPROVE WITH CHANGES:
[`review-ofdm-preamble-detect-2026-11-06.md`](../reviews/review-ofdm-preamble-detect-2026-11-06.md).
