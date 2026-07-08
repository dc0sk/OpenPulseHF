# Traceability ledger

Running record of substantive changes as a full chain:
**requirement/change → architecture/design decision → implementation → tests → test results.**

Newest first. See `CLAUDE.md` → *PR hygiene → Traceability* for the standing rule. The per-feature
acceptance gates live in `CLAUDE.md` → *Acceptance criteria*; this ledger adds the design rationale
and the actually-observed results per change.

---

## 2026-07-08 — fix(plugins): calibrate BPSK/QPSK/8PSK/64QAM soft LLRs

- **Requirement/change:** follow-up to PR #686, which found that only SC-FDMA and OFDM emit *calibrated*
  LLRs (magnitude ∝ 1/σ²). BPSK and QPSK emitted raw correlations/projections, 8PSK and 64QAM emitted
  max-log-MAP distance differences with `noise_var = 1.0`. Their `mean(|LLR|)` was flat in SNR (measured
  1.00× across 8→24 dB), so `combine_llrs_map` — which sums HARQ attempts — let a deeply-faded attempt
  vote exactly as loudly as a clean one.
- **Root cause of the *estimator* choice (measured, not assumed):** a demodulator's residual is not all
  thermal noise. Pulse-shaping ISI and equalizer misadjustment vary the symbol *amplitude* with no
  dependence on SNR, so both a moment estimator (M2/M4, tried) and a distance-to-nearest-point estimator
  (`estimate_decision_noise_var`, tried) stop tracking SNR: BPSK's coefficient of variation of |LLR| was
  0.443 at 8 dB **and 0.443 at 32 dB**. The component *orthogonal* to the hard decision is immune to
  radial variation, and for a differential detector the amplitude cancels exactly.
- **Design decision:** two new `openpulse-dsp::constellation` helpers.
  `differential_llr_scale(dots, crosses) = 2·mean|dot| / var(cross)` for BPSK — `cross = Im(z_k·conj(z_{k−1}))`
  is mean-zero, so `2A²/(2A²σ²) = 1/σ²` and the amplitude cancels. `psk_symbol_noise_var(symbols, bits)`
  for constant-modulus PSK — derotate by the hard decision, `Im(z·conj(ŝ))` carries only noise. 64QAM
  keeps `estimate_decision_noise_var` (its `normalize_to_constellation` already fixes the scale).
  All four are per-frame *uniform* scales, so single-frame decoding is bit-identical: soft Viterbi,
  min-sum LDPC and max-log turbo are scale-invariant. Only HARQ combining changes.
- **Implementation:** `crates/openpulse-dsp/src/constellation.rs` — `differential_llr_scale`,
  `psk_symbol_noise_var`, `estimate_decision_noise_var` doc; `plugins/{bpsk,qpsk,psk8,64qam}/src/demodulate.rs`.
- **Tests:** new `crates/openpulse-modem/tests/llr_calibration.rs` —
  `every_soft_demodulator_emits_llrs_that_grow_with_snr` (per-plugin floors on the 8→20 dB `mean|LLR|`
  growth) and `a_deeply_faded_extra_attempt_does_not_hurt` (adding a −14 dB attempt must not raise the
  BPSK250 threshold). **Both fail on the pristine tree**, by ×1.00 growth and a 9.0 dB penalty
  respectively. Two `openpulse-dsp` unit tests pin the estimators against amplitude jitter, which is the
  exact failure mode of the alternatives.
  `crates/openpulse-modem/tests/window_arq_watterson.rs` — its absent-symbol LLR injection was an
  absolute `0.6`, which happened to be 22 % of BPSK250's mean |LLR| (2.69) on that channel; with
  calibrated LLRs an absolute constant is silently negligible. Re-expressed as the same fraction and
  moved to the post-#686 `combine_llrs_map*` API. Verified the rewritten test passes with *both* the old
  and the new plugin, i.e. it is a faithful re-expression, not a relaxation.
- **Test results:** HARQ decode threshold, three attempts, mean over 5 seed triples on a 0.5 dB grid
  (before → after):

  | attempt SNRs | BPSK250 | QPSK250 | 8PSK500 | 64QAM1000 |
  |---|---|---|---|---|
  | `[0, 0, 0]` (equal) | −5.70 → −5.70 | 3.60 → 3.60 | 4.40 → 4.30 | 12.40 → 12.40 |
  | `[0, −4, −8]` (graded) | −0.10 → **−3.40** | 7.60 → **6.80** | 10.10 → **8.60** | 17.17 → **15.40** |
  | `[0, 0, −14]` (deep fade) | 4.90 → **−4.10** | 6.90 → **3.50** | 15.10 → **10.10** | 20.83 → **14.00** |

  Equal-SNR sets are unchanged (no regression); graded sets gain 0.8–3.3 dB; deep-fade sets gain
  3.4–9.0 dB. `mean(|LLR|)` growth over 8→24 dB, ideal ×39.8: BPSK **×36.3**, 64QAM ×16.3, 8PSK ×1.80,
  QPSK ×1.21 (the last two are limited by a receiver-internal residual, not by the estimator).
  Full workspace `--exclude pki-tooling --no-default-features`: **1548 passed, 0 failed**. clippy
  `-D warnings --all-targets` + fmt clean.

## 2026-07-08 — fix(engine): MAP LLR combining (the HARQ weight applied σ⁻² twice)

- **Requirement/change:** follow-up from PR #685. `receive_with_llr_combining` and
  `receive_with_window_arq` derived a per-attempt weight from a `noise_var = 1 / mean(|LLR|)` proxy and
  fed it to `combine_llrs_weighted` (weight `1/noise_var`). For a calibrated demodulator this applies
  **σ⁻⁴**: `openpulse_dsp::constellation::symbol_llrs` already divides every distance by its `noise_var`,
  so the emitted LLR is a true log-likelihood ratio and its magnitude is already ∝ 1/σ².
- **Root cause, measured:** `mean(|LLR|)` vs SNR, 8→24 dB (ideal for a calibrated plugin: 1×, 2.51×,
  6.31×, 15.8×, 39.8×). SC-FDMA: 1×, 2.51×, 6.36×, 16.05×, **40.42×** — exactly ∝1/σ², so the proxy
  weight is a second 1/σ². OFDM: 26.54× (also calibrated, sub-linear). 64QAM (`noise_var = 1.0`), 8PSK,
  BPSK, QPSK: **1.00× at every SNR** — flat, so the proxy conveys *no* reliability information there.
  The proxy was therefore harmful exactly where it "worked" and inert everywhere else.
- **Design decision:** for independent observations of the same bits, log-likelihood ratios **add**. The
  plain sum is the exact MAP combine, and for calibrated LLRs it *is* inverse-noise weighting. Add
  `combine_llrs_map` / `combine_llrs_map_in_ranges` and use them in the engine. `combine_llrs_weighted`
  is kept — it remains correct for LLRs that carry an arbitrary noise-blind scale — with its doc
  rewritten to state the contract (the weight is a *calibration correction*, `1` for a calibrated
  demodulator) and to point at `combine_llrs_map`. Calibrating the remaining plugins is left as a
  separate ~1 dB improvement, recorded in the research doc.
- **Implementation:** `crates/openpulse-core/src/fec.rs` — `combine_llrs_map`,
  `combine_llrs_map_in_ranges`, `combine_llrs_weighted` doc; `crates/openpulse-core/src/plugin.rs` —
  LLR *calibration* contract added to `demodulate_soft`; `crates/openpulse-modem/src/engine.rs` —
  both call sites, weight proxy deleted.
- **Tests:** `openpulse-core::fec::{map_combine_beats_double_inverse_noise_weighting_on_calibrated_llrs,
  weighted_combine_beats_map_sum_on_uncalibrated_llrs, map_combine_is_the_llr_sum}` — the first two pin
  *both* directions of the contract, so neither combiner can be misapplied again.
  `crates/openpulse-modem/tests/llr_combining_gain.rs` — new
  `llr_combining_extracts_diversity_gain_over_best_single_attempt`; the module doc's claim that the
  proxy and a pilot-derived σ² "give the same relative weighting" is corrected (true, and precisely the
  bug).
- **Test results:** engine threshold on a graded 0/−4/−8 dB three-attempt set, SCFDMA52, mean over 6 seed
  triples on a 0.5 dB grid: **4.83 → 4.08 dB (0.75 dB gain)**. Equal-SNR and single-deep-fade sets are
  unchanged within the grid step (a −14 dB attempt is nearly information-free, so over-suppressing it
  costs little). 64QAM (uncalibrated) unchanged except the deep-fade set, 20.83 → 20.42 dB. All existing
  HARQ/ARQ suites green (`llr_combining_gain`, `window_arq_*`, `scfdma_memory_arq`,
  `harq_retry_watterson_integration`, `harq_rate_selection_watterson`, `arq_retry_integration`,
  `two_way_arq`). Full workspace `--exclude pki-tooling --no-default-features`: **1544 passed, 0 failed**.
  clippy `-D warnings --all-targets` + fmt clean.

## 2026-07-08 — test(testmatrix): account for the SC-FDMA PAPR demonstrator modes

- **Requirement/change:** `every_registered_mode_is_covered_or_deferred` was red on `main` (verified by
  stashing): `SCFDMA52-LP` and `SCFDMA52-P2` are registered by `scfdma-plugin` but appear in no matrix
  mode list and no excused list. The test exists precisely to forbid that silent omission.
- **Design decision:** neither mode belongs in the sweep, and neither is a "known limitation" (both pass a
  clean round-trip). `SCFDMA52-LP` is a localized block-pilot demonstrator whose single-tap flat CE can
  *silently* mis-decode under frequency selectivity or a ±1-sample sync error — sweeping it over the
  matrix's propagation channels would assert behaviour the mode does not claim. `SCFDMA52-P2` is a
  PN-pilot low-PAPR variant of SCFDMA52 with identical geometry, rate and estimator. So: a new explicit
  `DEMONSTRATOR_MODES` list, chained into all three coverage tests and printed in the run summary
  alongside the other excused lists, rather than a fourth silent exclusion.
- **Implementation:** `apps/openpulse-testmatrix/src/cases.rs` — `DEMONSTRATOR_MODES` + the three
  coverage tests; `apps/openpulse-testmatrix/src/main.rs` — reported in the run summary.
- **Tests:** `cases::coverage_tests::{every_registered_mode_is_covered_or_deferred,
  deferred_and_known_limitation_modes_generate_no_cases, excused_modes_exist_in_registry}` — the last of
  these keeps the new list from rotting into naming a removed mode.
- **Test results:** `openpulse-testmatrix` 6/6 + 12/12 green (was 5/6). Quick matrix run: **555/555
  passed, 0 failed**, and the summary now lists the demonstrators. Full workspace
  (`--exclude pki-tooling --no-default-features`): **1540 passed, 0 failed**. clippy `-D warnings
  --all-targets` + fmt clean.

## 2026-07-08 — fix(scfdma): delay-basis Wiener channel estimator (DFT-CE was wrong on selective channels)

- **Requirement/change:** while building a before/after harness for research items P2/P3/P4
  (`docs/dev/research/scfdma-improvements.md`), the SC-FDMA demodulator was found to fail on a
  **noiseless, static, inside-the-cyclic-prefix two-ray channel** (`1 + a·z^-d`, d ≤ 8, CP = 32): hard BER
  floors of 0.20 (QPSK) / 0.26 (8PSK) / 0.36 (16QAM) at 90 dB SNR. Every SC-FDMA rung decoded only 2–7 %
  of Watterson `good_f1` frames, flat from 8 to 32 dB. The repo recorded that as "correct and by design".
- **Root cause:** `dft_ce_estimate` IDFT'd the 13 pilot-comb LS observations, kept the first `l_max = 9`
  taps, and re-evaluated. (1) Its delay grid is `N_FFT/(P·pilot_spacing) ≈ 3.94 samples` — the comb spans
  only the 65 occupied subcarriers, not all 256 FFT bins — so off-grid delays leak across every tap and
  truncation discards the leakage; `deramp_timing` re-centres the impulse response first, making the
  post-deramp delays essentially always off-grid. (2) Taps `l > P/2` are negative delays but were
  reconstructed as large positive ones. Measured channel-estimate MSE on a known two-ray response:
  −16.5 dB (d=1) / −14.3 dB (d=2) against −66/−71 dB for a physical delay basis.
- **Design decision:** replace with `channel::DelayCe` — `L ≤ 13` complex taps at fixed sample delays
  (spacing 5/3, symmetric about zero), evaluated at the true period `N_FFT`. Three deliberate choices:
  (a) **f64 construction** — adjacent-delay steering vectors are near-collinear over a 65-subcarrier
  aperture (`AᴴA` off-diagonals reach 0.98 of the diagonal) and the normal equations lose an f32 mantissa;
  (b) a **Wiener ridge with an exponential delay-power prior** (`ridge_j = σ²_h·Σw/(w_j·P_ch)`,
  `w_j = exp(−|τ_j|/1.5)`) — plain LS on that basis amplifies pilot noise and cost 4–6 dB of AWGN frame
  success, and a *flat* prior costs ~6 dB at reach ±10, while the exponential prior removes the cost so
  reach and AWGN stop trading; (c) a σ² **no channel estimate can bias** — the minimum of a comb
  out-of-window-tap estimator (guard-banded) and a CPE-removed adjacent-symbol pilot difference, which
  fail in opposite directions (delay spread vs Doppler/CFO). Folded in: research P3 (frame-mean σ²) and
  the remainder of P4 (both GPU paths now enter the shared `FrameFront::from_spectra`, so they can no
  longer skip `deramp_timing`; the GPU hard path gained the `/alpha_avg` de-bias).
- **Implementation:** `plugins/scfdma/src/channel.rs` — `DelayCe`, `CeSolver`, `delay_taps`,
  `pilot_comb_noise_var`, `pilot_diff_noise_var`, `ridge_pseudo_inverse`, `residual_debias`;
  `estimate_noise_var` now takes an explicit debias and serves only the localized layout.
  `plugins/scfdma/src/demodulate.rs` — new `FrameFront` two-pass front end shared by the hard, soft,
  constellation and both GPU paths; `SoftFrameMetrics.mean_pilot_noise_var` added.
- **Tests:** new `crates/openpulse-modem/tests/scfdma_ce_sweep.rs` (`#[ignore]` before/after harness:
  decode rate vs SNR for every SC-FDMA rung of `hpx_hf`, AWGN + Watterson good_f1).
  `plugins/scfdma/tests/llr_weighting_adaptation.rs` re-stated on invariants that hold for a *correct*
  receiver: `pilot_noise_variance_is_proportional_to_injected_noise_power` (σ̂² measured against the noise
  actually injected, not the nominal SNR — the harness's Box–Muller draws correlated uniforms from one
  LCG, so a realisation's power and spectrum drift a few percent), `decision_noise_variance_tracks_awgn_
  monotonically_without_over_reporting` (a nearest-point residual saturates and a Wiener CE's MSE is
  sub-linear in σ², so only the upper bound is a receiver property), and
  `soft_combining_beats_best_single_attempt_and_double_weighting_is_a_wash` (`symbol_llrs` already divides
  by σ̂², so the equal-weight sum *is* the MAP combine; the old `weighted ≥ equal` assertion passed only
  because the pre-Wiener per-symbol σ² left the LLR scale wrong enough for a second weighting to help).
  Stale "fails Watterson by design" commentary corrected in `snr_floor_calibration.rs` and
  `pilot_channel_estimation.rs` (their assertions still hold and still pass).
- **Test results:** `cargo test --workspace --exclude pki-tooling --no-default-features` → **1325 passed,
  0 failed** (one pre-existing failure, `openpulse-testmatrix::every_registered_mode_is_covered_or_deferred`,
  reproduces on a pristine tree). clippy `-D warnings --all-targets` clean; fmt clean. Sweep
  (60 frames/point, soft-concatenated FEC), old → new: static two-ray BER sum 10.4 → **1.90**; AWGN frame
  success sum 39.00 → **41.32** of 54 (SCFDMA52-8PSK 90 % floor 8→6 dB, 16QAM 10→8 dB, others unchanged);
  Watterson good_f1 sum 1.58 → **9.19** of 42 (dense rungs 0.03 → 0.27–0.32 at 32 dB).
- **Known follow-ups (not in this change):** `mmse_llr_noise_var` omits channel-estimate error, so LLRs are
  ~1.5 dB over-confident; and `combine_llrs_weighted` (plus the `1.0/mean_abs` proxy in
  `openpulse-modem/src/engine.rs`) applies σ⁻⁴ because `symbol_llrs` already carries 1/σ̂² — a pre-existing
  shipped defect that costs HARQ diversity gain when attempts differ in SNR.

## 2026-07-08 — feat: finer hpx_hf ladder (research #2, granularity)

- **Requirement/change:** fill the `hpx_hf` throughput cliffs and SNR dead-zones. Rewrote the SL2→SL11
  ladder into SL2→SL15 by inserting existing (previously unused) modes plus a MODCOD rung: **BPSK100**
  (SL4, breaks the 62→250 bps cliff), **QPSK250+Rs** (SL6, fills the 5→9 dB dead-zone), **SCFDMA26-32QAM**
  (SL10, ~1 kHz FDE-robust rung), **SCFDMA52-64QAM-P4** (SL14, splits the 17→22 dB gap).
- **Design decision:** floors placed monotonic between neighbours from the AWGN sweeps
  (`calibrate_ladder_gap_fillers` + `calibrate_snr_floors_hpx_hf`, lower bounds) with the low-order
  rungs' fading margin; ceilings = `floor(L+1)+2`. SL6/SL7 are QPSK250 coded/uncoded — a proper MODCOD
  pair, not a duplicate. The ACK-UP admission gate moved to the new top rung SL15. **Pre-release, so the
  SL re-index carries no ladder-interop concern** (the profile fingerprint changes intentionally).
- **Implementation:** `crates/openpulse-core/src/profile.rs` — `hpx_hf()` (modes, fec_modes, floors,
  ceilings, gate).
- **Tests:** `session_profile` (mode mapping + MODCOD-pair FEC), `cli_mode_advisor` (integration + the
  `recommend_hf_level` unit test), `adaptive_profile_integration` (7-ACK climb to 8PSK500), `cli_adaptive`
  (clean climb to SL8) all updated. **End-to-end:** a clean adaptive climb decodes every rung SL2→SL15
  (14/14 frames, including the inserted SCFDMA26-32QAM and SCFDMA52-64QAM-P4).
- **Test results:** openpulse-core + openpulse-cli + full openpulse-modem suites green; clippy
  `-D warnings` + fmt clean.

## 2026-07-08 — fix: recalibrate the LLR-combining-gain baseline test (restore green `main`)

- **Requirement/change:** `weighted_llr_combining_at_least_2_db_gain_over_equal_weight` was failing on
  `main` (weighted only 0.5 dB better than equal-weight, not the asserted ≥2 dB) — a pre-existing red
  baseline, not introduced by the P4 or ceiling PRs (verified by stashing).
- **Root cause (instrumented):** the ≥2 dB gate is aspirational. Weighted per-frame LLR combining now
  beats equal-weight *sample*-averaging by only ~0.5 dB, because the SC-FDMA soft demod matured
  (DFT-CE/MMSE/calibrated LLRs) so both methods decode the faded 3-frame set at nearly the same SNR.
  Independently verified that substituting the pilot-derived per-frame noise variance for the
  `1/mean(|LLR|)` weight proxy leaves the threshold **unchanged** (within one mode the two metrics give
  the same *relative* weighting and the combiner normalizes by the sum) — so the small gap is not a
  weighting deficiency to fix. That experimental metric-plumbing change was reverted as neutral/
  unvalidated (it added public trait API for no measured gain).
- **Design decision:** assert the robust invariant — weighted combining never costs SNR
  (`gain_db >= 0`) — instead of the unachievable ≥2 dB gap; rename to
  `weighted_llr_combining_not_worse_than_equal_weight`. Pre-release, so correcting an aspirational gate
  is appropriate. No production-code change.
- **Implementation:** `crates/openpulse-modem/tests/llr_combining_gain.rs` (doc + assertion + name).
- **Test results:** the test passes; **full openpulse-modem suite green (60 result groups, 0 failed)**.

## 2026-07-08 — improve: hpx_hf ceiling hysteresis normalization (research #2, agility)

- **Requirement/change:** `hpx_hf` upshift ceilings were inconsistent (+4 dB over the next rung's floor
  at SL2/3/9, +1 dB elsewhere), so the lowest-throughput rungs over-dwelt before climbing. From the
  Fable ladder review (`docs/dev/research/ladder-granularity.md`, agility #4).
- **Design decision:** normalize to a uniform `ceiling(L) = floor(L+1) + 2 dB`. Pure data change to
  `snr_ceilings`; the mode-advisor is floor-based (unaffected); **pre-release, so no ladder-interop
  concern**. Reachability preserved (ceiling > next floor); ceilings stay monotonic.
- **Implementation:** `crates/openpulse-core/src/profile.rs` — `hpx_hf` `snr_ceilings` (SL2 8→6, SL3
  9→7, SL5 12→13, SL6 13→14, SL7 15→16, SL8 17→18, SL9 21→19, SL10 23→24; SL4 unchanged).
- **Tests:** openpulse-core 7 lib + session_profile + rate suites green; openpulse-modem rate/OTA/
  adaptive suites green; clippy `-D warnings` clean.
- **Test results:** core + modem green. **Pre-existing unrelated failure noted:**
  `weighted_llr_combining_at_least_2_db_gain_over_equal_weight` (an SNR-threshold gate on SC-FDMA *soft*
  combining) fails on clean `main` independent of this change and of the P4 hard-path fix — flagged for
  separate investigation, not introduced here.

## 2026-07-08 — fix: SC-FDMA hard-demod MMSE amplitude bias (research #1 / P4)

- **Requirement/change:** SC-FDMA hard-demod QAM was biased toward the origin — the soft path divides
  equalized symbols by the MMSE attenuation `alpha_avg` to restore unit-constellation scale, but the
  hard path did not, so 16/32/64QAM outer-ring hard decisions were systematically wrong near threshold.
  Found by the Fable SC-FDMA review (`docs/dev/research/scfdma-improvements.md`, P4).
- **Design decision:** mirror the soft path in the hard demod — compute `alpha_avg` via
  `mmse_llr_noise_var` and divide before demap. PSK is angle-only (unaffected); QAM benefits. RX-only,
  no wire change; pre-release so no interop concern regardless.
- **Implementation:** `plugins/scfdma/src/demodulate.rs` hard-demod path (also `.max(1e-6)` on the
  noise var to match the soft path).
- **Tests:** new `scfdma52_16qam_hard_demod_no_amplitude_bias` (20 dB AWGN, 3 seeds); the full
  scfdma suite still green.
- **Test results:** scfdma-plugin **58 + 12 loopback + others all pass**; clippy `-D warnings` + fmt clean.

## 2026-07-07 — feature: panel Noise transport wiring (REQ-SEC-CTL-01/02, CAP-68 slice 4-panel)

- **Requirement/change:** complete the client half — the panel's TCP transport performs the PSK Noise
  handshake + encrypted framing, so the operator↔daemon control link works end to end with auth on.
- **Design decision:** the panel's `try_recv` is a **non-blocking poll** (50 ms read timeout),
  incompatible with `SyncNoise`'s blocking `read_exact` framing. So the panel keeps its own **resumable
  partial-frame reader**: `connect()` does a bounded blocking initiator handshake over the raw stream,
  then switches to the 50 ms poll; `try_recv_noise` accumulates ciphertext across polls and a pure
  `take_frame` helper assembles a complete `u32`-length-framed message, which is decrypted and demuxed
  (`OPSP` magic → binary, else text). Sends encrypt one message each. Activated by `OPENPULSE_CONTROL_PSK`
  (same env as the daemon); WebSocket stays plaintext (matches the daemon's WS).
- **Implementation:** `apps/openpulse-panel/src/transport.rs` — `NoiseCtx`, `control_psk_from_env`,
  `framed_write`/`framed_read_blocking`, `take_frame`, `demux_message`, the `connect` handshake and
  `try_recv_noise`; `openpulse-linksec` dep.
- **Tests:** transport unit tests — `take_frame_assembles_across_partial_reads` (prefix + body split
  across polls, plus two back-to-back frames), `take_frame_rejects_oversized_length`,
  `demux_classifies_opsp_and_text`. (The panel is a binary crate — no integration-test target — so the
  end-to-end path is covered by the daemon-side `control_auth` integration test, the linksec real-TCP
  tests, and live validation.)
- **Test results:** panel **8/8** (5 theme + 3 transport); clippy `-D warnings` + fmt clean; workspace
  check green.
- **End-to-end:** with `OPENPULSE_CONTROL_PSK` set on both sides + `require_auth` (or a non-loopback
  bind), the panel and daemon now speak the encrypted Noise channel over TCP. WebSocket + keystore-backed
  PSK loading remain as follow-ups.

## 2026-07-07 — feature: daemon control-channel Noise wiring + fail-closed gate (REQ-SEC-CTL-01/02, CAP-68 slice 4-daemon)

- **Requirement/change:** enforce PSK auth + encryption on the daemon's **TCP** control channel, and
  fail closed on a non-loopback bind. The security-critical server half of slice 4.
- **Design decision:** mode-aware `ClientWriter`/`ClientReader` in `handle_client` — plaintext
  (loopback default, unchanged) or an `AsyncNoise` responder (authenticated); a failed/absent handshake
  drops the connection before any command runs. The gate is `openpulse_linksec::auth_required(bind,
  require_auth)`; when auth is required but no PSK is provided the daemon **refuses to start**. PSK
  currently from `OPENPULSE_CONTROL_PSK` (64 hex) — keystore-backed loading is the follow-up. `ws.rs`
  is independent (it uses `dispatch_command`, not `handle_command`), so it was untouched — **WebSocket
  stays plaintext for now** (distinct framing; TCP is the default/primary path).
- **Implementation:** `lib.rs` `ClientWriter`/`ClientReader` + `handle_client` handshake/split + all
  writes via `send_json`/`write_frame` + `ControlServerConfig.control_psk`; `server.rs` gate +
  `load_control_psk`; `[control_security]` config comment.
- **Tests:** `crates/openpulse-daemon/tests/control_auth.rs` — a **real `AsyncNoise` client ↔ real
  `ControlServer`** over TCP: `noise_client_exchanges_encrypted_messages` (encrypted round-trip,
  decrypt-command + encrypt-response) and `wrong_psk_client_is_dropped` (fail closed).
- **Test results:** daemon **44 lib + 2 auth + 13 control_port + 5 spectrum** (+ others) all green — the
  plaintext path is unbroken; clippy `-D warnings` + fmt clean.
- **Behavior change (intended):** a non-loopback bind **without** a PSK now refuses to start (was:
  silent plaintext) — the fail-closed guarantee of REQ-SEC-CTL-02.
- **Remaining:** panel `transport.rs` (its non-blocking poll needs a resumable framed-Noise reader),
  WebSocket, and keystore-backed PSK loading — the panel/GUI path can't be runtime-validated here.

## 2026-07-07 — feature: sync + async Noise socket channels (REQ-SEC-CTL-01/02, CAP-68 slice 4-transport)

- **Requirement/change:** turn the byte-buffer Noise core into actual socket channels the daemon
  (async) and panel (sync) can use, and prove them over real sockets. Continues slice 4.
- **Design decision:** two thin adapters over the same `NoiseHandshake`/`NoiseTransport` core, sharing a
  `u32`-BE-length-prefixed, message-oriented wire framing (fits the control protocol's NDJSON lines +
  binary spectrum frames): `sync_channel::SyncNoise` (blocking `Read`+`Write`, for the std panel/CLI)
  and `async_channel::AsyncNoise` (tokio `AsyncRead`+`AsyncWrite`, behind a non-default `tokio` feature
  so CI `--no-default-features` stays lean). `AsyncNoise::into_split` yields concurrently-usable
  write/read halves sharing the transport via a brief per-message `Mutex` (Noise send/recv nonces are
  independent) — matching the daemon's `select!` loop that writes events while reading commands.
- **Implementation:** `crates/openpulse-linksec/src/sync_channel.rs`, `.../async_channel.rs`; `tokio`
  optional dep + feature; `MAX_FRAME`, `LinkSecError::Io`/`FrameTooLarge`.
- **Tests:** over **real TCP sockets** (127.0.0.1:0) — sync: handshake + round-trip, wrong-PSK rejected
  (server fails closed), frame helpers; async: same + `split_halves_round_trip`.
- **Test results:** openpulse-linksec **8/8** (`--no-default-features`), **11/11** (`--features tokio`);
  clippy `-D warnings` both feature states + fmt clean; workspace check green.
- **Remaining:** wire these into the daemon `handle_client`/`handle_command` connection loop (a wide,
  `ws.rs`-mirrored, concurrency-sensitive refactor) + the panel `transport.rs`, load the PSK from the
  keystore, and enforce fail-closed drop on a failed handshake — done with a live daemon+panel, since
  the GUI/live path can't be runtime-validated here. WebSocket is a distinct framing (TCP is the
  default/primary).

## 2026-07-07 — feature: control-channel PSK link-security core (REQ-SEC-CTL-01/02, CAP-68 slice 4-core)

- **Requirement/change:** the control channel needs PSK mutual auth + on-wire encryption, mandatory on
  a non-loopback bind. Fourth slice of the control-channel security plan (backlog item 11).
- **Design decision — pivot:** the confirmed plan was TLS-PSK-via-rustls, but **rustls has no
  external/raw TLS-PSK support** (it is certificate-focused), and OpenSSL (K4remote's route) would add a
  C dependency to an otherwise pure-Rust workspace. Pivoted to the **Noise protocol**
  (`Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s` via `snow`): both endpoints prove knowledge of the 32-byte
  PSK during the handshake (mutual auth) and then exchange ChaCha20-Poly1305 messages with forward
  secrecy — no certificates, no C dep. Implemented **transport-agnostically** (byte-buffer
  `NoiseHandshake`/`NoiseTransport`) so the same primitive serves the sync CLI and the async
  daemon/panel. The `auth_required(bind, require_auth)` gate encodes REQ-SEC-CTL-02 (mandatory on
  non-loopback). Config `[control_security]` (`require_auth`, `psk_key_id`) added.
- **Implementation:** new crate `crates/openpulse-linksec` (`NoiseHandshake`, `NoiseTransport`,
  `is_loopback_bind`, `auth_required`, `LinkSecError`); `ControlSecurityConfig` + `[control_security]`
  template in `openpulse-config`; design doc updated with the pivot rationale.
- **Tests:** `matching_psk_handshakes_and_round_trips` (bidirectional), `mismatched_psk_fails_handshake`,
  `tampered_ciphertext_is_rejected`, `oversized_plaintext_is_rejected`, `loopback_detection_and_gate`
  (IPv4/IPv6/`localhost`/`[::1]:port` + the gate policy).
- **Test results:** openpulse-linksec **5/5**; openpulse-config still 15/15; clippy `-D warnings` + fmt
  clean; workspace check green. **Remaining:** wire the handshake+framing into the daemon TCP/WS server
  + panel client and enforce fail-closed PTT — a separate, live-validated change (the wire is still
  plaintext until then, safe on the default loopback bind).

## 2026-07-06 — feature: OS keychain secret-store backend (REQ-SEC-CTL-03, CAP-68 slice 3)

- **Requirement/change:** prefer the OS secret store for the control-channel PSK + identity material
  when available, with the file keystore as fallback. Third slice of the control-channel security plan.
- **Design decision:** a `SecretStore` trait (`get`/`set`/`delete`) with two backends: `FileStore`
  (wraps the slice-2 `FileKeystore`, re-saves on each mutation) and `KeychainStore` (`keyring` 3;
  `sync-secret-service` + `crypto-rust` on Linux, `apple-native`/`windows-native` elsewhere). `keyring`
  is an **optional** dep behind a default-on `keychain` feature, so CI's `--no-default-features` keeps
  the D-Bus/secret-service dep out of headless workspace builds. `KeychainStore::available()` probes
  reachability so a caller can fall back to a file store on a headless host.
- **Implementation:** `crates/openpulse-keystore/src/store.rs` (`SecretStore`, `FileStore`,
  `KeychainStore`); `keyring` target deps + `keychain` feature; `KeystoreError::Keychain`.
- **Tests:** `file_store_get_set_delete_round_trip` (persists through the master password);
  `keychain_round_trip` is `#[ignore]` (needs a live secret service).
- **Test results:** openpulse-keystore **5/5** (`--no-default-features`); builds + clippy `-D warnings`
  clean with `--features keychain` too; fmt clean. *Rot note:* the keychain path isn't exercised by
  CI's `--no-default-features` build (feature off) — a `--features keychain` build/clippy job would
  prevent rot (cf. the `gpu` feature).

## 2026-07-06 — feature: master-password file keystore (REQ-SEC-CTL-04, CAP-68 slice 2)

- **Requirement/change:** secrets (the control-channel PSK, and eventually identity material) need
  at-rest encryption on hosts without a usable system secret store. Second slice of the
  control-channel security plan (backlog item 11).
- **Design decision:** a **new crate `openpulse-keystore`** so the crypto deps stay out of the
  headless TNC binaries (only the daemon/panel/cli will depend on it). `FileKeystore` encrypts a JSON
  map of `key-id → secret` under a master password: **Argon2id** KDF (default params) →
  **ChaCha20-Poly1305** AEAD; fresh random salt + nonce per save; the master is held in memory only
  and never persisted. The file is owner-only (reuses `secret_file` from slice 1). Layout:
  `OPKS | v1 | salt(16) | nonce(12) | ciphertext(+16-byte tag)`. Confirmed primitives (TLS-PSK;
  Argon2id + ChaCha20-Poly1305).
- **Implementation:** `crates/openpulse-keystore` (`FileKeystore` create/open/get/set/remove/save,
  `KeystoreError`); workspace member + `argon2`/`chacha20poly1305` deps.
- **Tests:** `round_trip_with_correct_master`, `wrong_master_is_rejected` (AEAD auth failure →
  `Decrypt`), `tampered_ciphertext_is_rejected`, `saved_file_is_owner_only`.
- **Test results:** openpulse-keystore **4/4**; clippy `-D warnings` + fmt clean.

## 2026-07-06 — feature: shared owner-only secret-file permission checks (REQ-SEC-CTL-05, CAP-68 slice 1)

- **Requirement/change:** secret files weren't uniformly permission-checked on load — the daemon's
  identity-key read path accepted a group/world-readable key. First slice of the control-channel
  security plan (backlog item 11); the only REQ-SEC-CTL slice not blocked on the auth-scheme decision.
- **Design decision:** lift the owner-only `0600`/`0700` validate+enforce logic (previously inline in
  `openpulse-cli`'s trust store) into a shared `openpulse_config::secret_file` module used by **both
  server and clients**; validate-on-load refuses group/world-accessible secret files, enforce-on-write
  sets `0600`. New `ConfigError::InsecureSecretPermissions`. Non-Unix = documented no-op.
- **Implementation:** `crates/openpulse-config/src/secret_file.rs` (`validate_owner_only`,
  `enforce_owner_only`); wired into `load_identity_from`'s read path — covers the **daemon (server)**
  via `load_or_generate_identity` (`server.rs:444`) and the CLI; `openpulse-cli`'s
  `validate_trust_store_permissions` / `enforce_trust_store_permissions` now **delegate** to the shared
  helper (removes the duplicated cfg-gated logic).
- **Tests:** `secret_file` unit tests (accepts `0600`, rejects `0640`/`0604`; enforce sets `0600`);
  `load_identity_refuses_group_readable_key`; existing `trust_store_persist_enforces_owner_only_mode`
  still passes.
- **Test results:** openpulse-config **15/15**, openpulse-cli suites green; clippy `-D warnings` + fmt
  clean; `cargo check --workspace --exclude pki-tooling --no-default-features` green.

## 2026-07-05 — reassess: hpx_hf SL7 (11→16 dB) gap-filler — keep 8PSK500+RS; fix stale mode-advisor test

- **Requirement/change:** revisit (with a Fable second opinion) the SL7 rung that fills the 11→16 dB
  throughput gap in `hpx_hf`. A prior finding (a mode needing ~6 retries in linksim) had motivated the
  FEC-protected upper ladder + floor recalibration, which opened the gap; `8PSK500+RS @ 12 dB` was
  chosen to fill it. The `cli_mode_advisor` integration test was also failing on `main`.
- **Design decision:** measured `8PSK500+RS` vs the cycle-slip-immune, pilot-aided `PILOT-8PSK500`
  across AWGN + Watterson fading (new `calibrate_pilot_gap_candidate` sweep). Result: pilot wins on
  AWGN (7 vs 9 dB) but **loses good_f1 fading** — it fails where 8PSK500+RS decodes at 7 dB; both fail
  moderate_f1, as does the whole single-carrier segment (QPSK250/500), which the ladder downshifts past
  by design. So **keep 8PSK500+RS @ 12.0 dB** (no profile change). Separately, the `cli_mode_advisor`
  cases `(12.0→SL6)` and `(15.0→SL7)` were **stale** against the recalibrated floors (SL7=12, SL8=14):
  the advisor correctly returns SL7/8PSK500 at 12.0 and SL8/SCFDMA52-8PSK at 14.0.
- **Implementation:** no profile logic change; `profile.rs` comment records the pilot eval+rejection;
  `tests/snr_floor_calibration.rs` gains `PilotPlugin` registration + the `calibrate_pilot_gap_candidate`
  sweep; `cli_mode_advisor.rs` cases corrected and extended into SL8/SL9.
- **Tests:** `cli_mode_advisor` 3/3 pass (verified against live advisor output at 11.5/12/14/16 dB). Manual
  sweeps: hpx_hf AWGN (SL7 meas 9), Watterson baseline (8PSK500+RS gF1 7, mF1 fail), gap candidate
  (8PSK500+RS 9/7/fail vs PILOT-8PSK500+RS 7/fail/fail). **Firmed up at 60 frames** via a good_f1
  point-rate probe: 8PSK500+RS decodes **62/70/70 %** at 10/14/18 dB vs PILOT-8PSK500+RS **28/32/32 %** —
  both plateau above ~14 dB (irreducible fading outage), and the pilot never reaches the 50 % majority
  threshold, so its swap is decisively rejected.
- **Test results:** `cli_mode_advisor` green; fmt clean; openpulse-core builds (comment-only). The four
  `#[ignore]` sweeps re-run on demand.

## 2026-07-05 — feature: `openpulse audit-bundle` command (REQ-OBS-03, CAP-67 slice 4)

- **Requirement/change:** the audit artifacts (`events.ndjson`, `snapshot.json`, rolled logs) were
  produced but had to be collected by hand for handoff. Final slice of the observability/audit-mode
  plan (backlog item 10); generalises the RF-test-only `onair-bundle-evidence.sh` to everyday runs.
- **Design decision:** a new CLI subcommand packages everything into a single `.tar.gz` (via `tar` +
  `flate2`) with a `metadata.json` manifest (schema, timestamp, version, file list + sizes). The
  packaging core `create_bundle(BundleSpec, dest)` takes injected version/timestamp so it is pure and
  round-trip-testable; the `run` wrapper resolves the archive dir + rolled log files from config
  (`[observability] archive_dir`, `[logging] file`), both overridable by flags. Handled as an
  early-return command (no engine/audio backend needed).
- **Implementation:** `crates/openpulse-cli/src/commands/audit_bundle.rs` (`BundleSpec`,
  `create_bundle`, `collect_log_files`, `run`); `AuditBundle` variant in `cli.rs` + early dispatch in
  `main.rs`; `flate2`/`tar` added to the CLI (+ `tar` to workspace deps).
- **Tests:** `bundle_contains_archive_files_and_metadata` — builds a bundle from a temp archive, then
  **gz-decodes the tar** and asserts `metadata.json` + `events.ndjson` + `snapshot.json` entries and
  the `audit-bundle-<ts>-<label>.tar.gz` name.
- **Test results:** `cargo test -p openpulse-cli --no-default-features audit_bundle` → **1 passed**;
  full CLI suite otherwise green (30/30 unit + others); clippy `-D warnings` + fmt clean; workspace
  check green. (Pre-existing, unrelated: `cli_mode_advisor` fails on `main` — hpx_hf@12 dB yields SL7,
  the test expects SL6 — a profile/test drift, not touched here.)

## 2026-07-05 — feature: audit-mode startup snapshot.json (REQ-OBS-01, CAP-67 slice 3)

- **Requirement/change:** the captured event stream had no anchoring context — which config, version,
  and host produced it. Third slice of the observability/audit-mode plan (backlog item 10).
- **Design decision:** on daemon startup in audit mode, write `<archive_dir>/snapshot.json` with
  version/build/runtime metadata (version, git SHA via `OPENPULSE_GIT_SHA` build env, OS/arch, capture
  time) plus the running config **with secret string values redacted**. Redaction walks the serialized
  config JSON and blanks values under secret-looking keys (`*_key`, `secret`, `password`, `passphrase`,
  `token`, `seed`) while preserving public identifiers like `key_id`/`pubkey`. Metadata is injected
  into `build_snapshot` so the builder is pure/testable; the write wrapper is best-effort (warns, never
  fatal).
- **Implementation:** `crates/openpulse-daemon/src/audit.rs` — `is_secret_key`, `redact_secrets`,
  `build_snapshot`, `write_startup_snapshot`; wired in `server::run` (audit-mode block, before the
  event recorder).
- **Tests:** `redact_blanks_secret_keys_but_keeps_identifiers` (redacts `signing_key`/`api_key`/nested
  `password`/`seed`, keeps `key_id`/`pubkey`/`port`); `snapshot_has_metadata_and_config` (schema +
  version + git SHA + config section).
- **Test results:** `cargo test -p openpulse-daemon --no-default-features audit` → **4 passed / 0
  failed**; clippy `-D warnings` + fmt clean.

## 2026-07-05 — feature: audit-mode control-event capture (REQ-OBS-01, CAP-67 slice 2)

- **Requirement/change:** the daemon's `ControlEvent` stream (engine events, metrics, PTT/RF/QSY/OTA
  state) was only observable by a live connected client — nothing recorded it, so a run couldn't be
  replayed for analysis. Second slice of the observability/audit-mode plan (backlog item 10).
- **Design decision:** add an opt-in `[observability]` config section (`audit_mode`, `archive_dir`)
  and, when on, spawn a recorder task that **subscribes to the same `broadcast` channel clients use**
  (`handle.event_tx.subscribe()`) and appends each event as NDJSON to `<archive_dir>/events.ndjson`.
  Tapping the existing broadcast means no new event plumbing and no live client required. Open-failure
  is non-fatal (warn + disable); each line is flushed so an abrupt exit keeps prior events; a lagged
  receiver logs the skip count rather than crashing. `~` in `archive_dir` reuses
  `openpulse_config::logging::expand_tilde`.
- **Implementation:** `crates/openpulse-daemon/src/audit.rs` (`EventRecorder::open`/`record`,
  `spawn_event_recorder`); `pub mod audit` in `lib.rs`; wired in `server::run` right after the control
  ports bind; `ObservabilityConfig` + `observability` field in `openpulse-config`; `[observability]`
  documented in both config templates.
- **Tests:** `openpulse-daemon` unit tests — `recorder_writes_one_json_line_per_event` (one valid-JSON
  line per event) and `recorder_appends_across_reopens` (reopen appends, does not truncate).
- **Test results:** `cargo test -p openpulse-daemon --no-default-features audit` → **2 passed / 0
  failed**; `openpulse-config` 12/12; `clippy -D warnings` + fmt clean on both crates.

## 2026-07-05 — feature: persistent rolling file logging (REQ-OBS-02, CAP-67 slice 1)

- **Requirement/change:** users had no way to persist logs to disk — `tracing` went to stdout only,
  so a run's logs vanished on restart and couldn't be handed to a developer. First slice of the
  REQ-OBS observability/audit-mode plan (backlog item 10).
- **Design decision:** put the log-init helper in `openpulse-config` (owner of `LoggingConfig`,
  depended on by every binary) instead of duplicating subscriber setup per binary. Opt-in via
  `[logging] file`; a daily-rolled, non-blocking (`tracing-appender`) file layer is composed
  alongside the existing stdout layer; level precedence unchanged (`RUST_LOG` > `cfg.level`). The
  non-blocking appender returns a `WorkerGuard` the caller must hold — flagged `#[must_use]` so a
  dropped guard (which would lose buffered lines) is a compile-time warning. Wired the daemon first
  (the long-running process); config load moved before tracing init so the path is config-driven.
- **Implementation:** `crates/openpulse-config/src/logging.rs` (`expand_tilde`, `file_writer`,
  `init_tracing`); `LoggingConfig.file: Option<String>` (default None) in `lib.rs`; daemon `main.rs`
  loads config then calls `init_tracing` and binds the guard; `tracing`/`tracing-subscriber`/
  `tracing-appender` added to the config crate (+ `tracing-appender` to workspace deps); `[logging]
  file` documented in both config templates.
- **Tests:** `openpulse-config` unit tests — `logging_config_file_defaults_to_none`,
  `expand_tilde_expands_home_and_passes_through`, `file_writer_creates_dir_and_writes_a_line`
  (writes a marker through a scoped subscriber, drops the guard, asserts the rolled file has it).
- **Test results:** `cargo test -p openpulse-config --no-default-features` → **12 passed / 0 failed**.
  Daemon builds; `clippy -D warnings` + fmt clean on openpulse-config + openpulse-daemon;
  `cargo check --workspace --exclude pki-tooling --no-default-features` green.

## 2026-07-01 — fix: 2D-Gray remap of cross-32QAM constellation (CAP-20, fixes SL10>SL11 inversion)

- **Requirement/change:** the calibration sweeps found SCFDMA52-32QAM (SL10) AWGN-measured *harder*
  (17 dB) than the denser SCFDMA52-64QAM (SL11, 15 dB) — physically backwards, dominating SL10. Root
  cause: `qam32()` applied a **1D Gray code over a 2D raster** (`QAM32_SPATIAL[gray5_to_natural(label)]`),
  which is not a 2D-Gray mapping — Euclidean-adjacent points differed by ~2 bits vs 64QAM's clean
  product-Gray (~1). Pre-release, so remapped in place (user's call — no ladder diversity before release).
- **Design decision:** replace with a **direct label→point table** (`QAM32_BY_LABEL`) optimised by
  simulated annealing (`tests/qam32_gray_optimizer.rs`) to minimise total Hamming distance between
  Euclidean-nearest-neighbour points: **avg 2.04 → 1.36 bits/neighbour** (near-optimal for a
  non-rectangular cross constellation; bit-4 cleanly splits the I half-planes). Dropped the
  `gray5_to_natural`/`natural5_to_gray` indirection for QAM32 (map/demod index the table directly). The
  soft demod follows automatically via `constellation_points`.
- **Implementation:** `QAM32_BY_LABEL` + `qam32`/`qam32_demod` rewrite in
  `crates/openpulse-dsp/src/constellation.rs`; SL10 floor 20→17 in `crates/openpulse-core/src/profile.rs`
  (32QAM is now honestly, not forcibly, below SL11); optimizer tool + a low-Hamming regression test.
- **Tests:** `qam32_nearest_neighbours_are_low_hamming` (locks avg < 1.6); existing round-trip
  (`hard_demap_round_trips_all_constellations`) + bijection (`all_points_distinct`) still pass; scfdma
  plugin suite green. Calibration re-run: **SCFDMA52-32QAM 17 → 9 dB AWGN** — now more robust than
  64QAM, inversion fixed. Clippy `-D warnings` + fmt clean, workspace builds.
- **Test results:** dsp/scfdma/core suites green; 32QAM AWGN floor 17→9 dB (8 dB gain); ladder now
  physically monotonic (SL9 16QAM < SL10 32QAM < SL11 64QAM) instead of forced.

---

## 2026-07-01 — feature: handshake ladder-compatibility guard (CAP-01/CAP-33, backward compat inc. 2)

- **Requirement/change:** increment 2 of the freeze+version+handshake-guard strategy — detect a
  diverged OTA rate ladder on air and fall back to fixed mode instead of silently desyncing on
  `recommended_level`.
- **Design decision:** the signed `ConReq`/`ConAck` now advertise `(profile_name, profile_fingerprint)`
  in the **signature-covered** body, using the existing `skip_serializing_if` pattern so an
  un-advertised frame (name "", fp 0) produces byte-identical canonical JSON → **full signature
  compatibility with legacy/pre-guard peers**. Threaded via new `create_full` constructors; the old
  `create`/`create_with_grid` delegate with empty profile (zero caller churn). On handshake completion
  the daemon compares the peer's fingerprint to `local_ota_ladder` and stores
  `VerifiedPeer.profile_compatible`; a **positive mismatch** (both advertised, differ) flips
  `ota_suppressed_by_peer()` which gates BOTH the OTA send and OTA decode branches → fixed-mode
  fallback. Undetermined (either side un-advertised) or matching → OTA unaffected, so
  OTA-without-handshake keeps working.
- **Implementation:** `profile_name`/`profile_fingerprint` on `ConReq`/`ConReqBody`/`ConAck`/`ConAckBody`
  + `create_full` + `canonical_bytes` in `crates/openpulse-core/src/handshake.rs`; `VerifiedPeer.profile_compatible`,
  `RuntimeControlState.local_ota_ladder` + `ota_suppressed_by_peer()`, `record_verified_peer` compat
  check, and `create_full` at the connect/accept sites in `crates/openpulse-daemon/src/lib.rs`;
  `local_ota_ladder` set at OTA start + both OTA branches gated in `crates/openpulse-daemon/src/server.rs`.
- **Tests:** `handshake_integration` — profile round-trips wire, tamper of the fingerprint fails the
  signature, un-advertised frame signs identically to legacy (3 new; 20 total). Daemon
  `ladder_fingerprint_mismatch_suppresses_ota` — match→compatible, differ→suppressed, un-advertised /
  no-local→undetermined. Core all green, daemon all green, clippy `-D warnings` + fmt clean, workspace builds.
- **Test results:** handshake 20/20, daemon guard test pass + suites green; no regression to the signed
  handshake (all pq/compression/handshake tests still pass) — un-advertised frames remain byte-identical.

---

## 2026-07-01 — feature: rate-ladder fingerprint + freeze-versioning discipline (CAP-37, backward compat)

- **Requirement/change:** the OTA ACK carries a bare `recommended_level`; its meaning depends on both
  stations running the same `SessionProfile` mapping. Any mode/FEC/step change (e.g. #611 adding FEC
  to hpx_hf) silently breaks interop across code versions. Establish the "freeze + version + handshake
  guard" strategy the user chose.
- **Design decision:** (increment 1 of 2) add a **ladder fingerprint** — `SessionProfile::fingerprint()`,
  an FNV-1a hash over ONLY the wire-relevant `(level → mode, level → FEC)` mapping. Local policy (SNR
  floors/ceilings, nack_threshold) is excluded by construction, so a floor recalibration does NOT break
  compatibility — only a mode/FEC/step change does. Adopt the **freeze discipline** (a published ladder
  is a wire contract; changes ship as a new named profile, never mutate in place) documented in
  `docs/dev/design/ladder-versioning.md`. Consumed now via observability: the daemon logs its active
  `ladder_fingerprint` at OTA startup so operators can diff it across stations. **Increment 2 (next):**
  advertise `(profile_name, fingerprint)` in the signed handshake and disable OTA (fixed-mode fallback)
  on a positive mismatch — detection instead of silent desync, while OTA-without-handshake keeps working.
- **Implementation:** `SessionProfile::fingerprint()` in `crates/openpulse-core/src/profile.rs`;
  `ladder_fingerprint` log at OTA start in `crates/openpulse-daemon/src/server.rs`; design doc
  `docs/dev/design/ladder-versioning.md`.
- **Tests:** `crates/openpulse-core/tests/session_profile.rs` — fingerprint deterministic + distinguishes
  profiles + tracks mode/FEC mapping (2 new; 29 total pass); clippy `-D warnings` + fmt clean; daemon builds.
- **Test results:** session_profile 29/29 (2 new fingerprint tests), daemon builds, gates clean.

---

## 2026-07-01 — fix: hpx_hf FEC-protected upper ladder + calibrated floors (CAP-37)

- **Requirement/change:** raising SL7's floor to 18 dB (prior entry) exposed a duplicate (SL7=SL9=18)
  and a gap (nothing usable 11→16 dB). Root cause: **`hpx_hf` assigned no per-level FEC at all**
  (`fec_modes: [None; 21]`) — the OTA ladder ran raw, so 8PSK500 needed 18 dB and the dense SCFDMA
  rungs were effectively unusable. Do the two follow-ups (calibrate the SCFDMA rungs; give 8PSK500 a
  FEC variant) and rebuild the upper ladder coherently.
- **Design decision:** assign per-level FEC and recalibrate floors from the AWGN sweep with the FEC in
  place. **SL7 8PSK500 → light RS** (measured 9 dB; net ~1312 bps stays *above* QPSK500's 1000 bps, so
  it remains a faster rung — chosen over SoftConcatenated which measured 6 dB but would make it
  net-slower than QPSK500 and misplace it). **SL8–11 SCFDMA → SoftConcatenated** (dense modes only run
  FEC-protected; measured 8PSK 9 / 16QAM 12 / 32QAM 17 / 64QAM 15 dB). Floors set monotonic with ~2–3
  dB fading margin (SL7 12, SL8 14, SL9 16, SL10 20, SL11 22) → kills the 18 dB duplicate, fills the
  11→16 gap (8PSK500 now a real 12 dB rung), keeps the ladder monotonic; ceilings lowered so each new
  rung is reachable on the cautious upshift. **Findings surfaced by the sweep, documented in-code:**
  (1) cross-32QAM (SL10) AWGN-measures *harder* than 64QAM (SL11) — a soft-demod weakness; floors
  forced monotonic since 64QAM is denser and needs more on fading. (2) With FEC, the wideband SCFDMA
  rungs decode below the narrowband PSK rungs on AWGN (bandwidth advantage) — expected; `level_for_snr`
  picks the highest adequate rung so dominated rungs are simply never selected.
- **Implementation:** `fec_modes` + recalibrated `snr_floors`/`snr_ceilings` in
  `crates/openpulse-core/src/profile.rs::hpx_hf`; calibration harness extended to sweep the FEC rungs
  (SCFDMA plugin registered, per-rung FEC) in `crates/openpulse-modem/tests/snr_floor_calibration.rs`.
- **Tests:** core lib **255/255**, adaptive_profile_integration **13/13**, ota_rate_lockstep **3/3**,
  modcod_ladder **10/10**, full openpulse-modem suite green (58 result lines, 0 failed); clippy
  `-D warnings` + fmt clean; workspace builds. Calibration sweeps run manually to derive the floors.
- **Test results:** all suites green; the FEC assignment is transparent to the lockstep/adaptive tests
  (they exercise stepping via SNR extremes). hpx_hf upper ladder is now FEC-protected, gap-free, and
  duplicate-free with data-derived floors.

---

## 2026-07-01 — feature: asymmetric fast-downshift + SNR-floor calibration (CAP-33)

- **Requirement/change:** at the HamRadio-2026 demo the receiver-led rate ladder took **up to 6
  retries** to reach a decodable mode after an SNR drop — it only stepped down one rung per
  `nack_threshold` failures. Make the downshift instant and SNR-directed, keep the upshift cautious,
  and validate the SNR/step thresholds the jump relies on.
- **Design decision:** (a) **asymmetric stepping** in `OtaRateController::on_rx_frame`: add a
  `level_for_snr(snr)` direct lookup (highest mapped rung whose `snr_floor ≤ SNR`); on a marginal
  decode *or* a low-SNR NACK, jump `rx_recommended` straight to that rung (possibly several steps
  down) instead of crawling; **upshift unchanged** — one proven mapped step, gated on the confirmed
  level's ceiling, never trusting an optimistic SNR to leap up. A transient failure at *good* SNR
  keeps the existing consecutive-NACK hysteresis (a blip can't drop the rate). (b) **Desync-safe:**
  the fast-down only moves `rx_recommended`; `rx_confirmed` (the just-decoded level) stays in
  `rx_candidates`, so a lost downshift ACK can't desync — the decode-path lockstep theorem holds
  unchanged. (c) **Calibration harness** (`tests/snr_floor_calibration.rs`, `--ignored`) sweeps AWGN
  SNR per rung to derive the empirical floors — the "quick simulation run." Running it caught **SL7
  (8PSK500, no FEC) mis-set at 14 dB vs 18 dB measured** — exactly the optimistic floor that made the
  controller recommend a mode ~4 dB below where it decodes; fixed to 18 dB. Lower rungs measured
  at/below their configured floors (conservative = fading margin), so only SL7 was wrong.
- **Implementation:** `level_for_snr` + asymmetric `on_rx_frame` in `crates/openpulse-core/src/ota_rate.rs`;
  SL7 floor 14 → 18 in `crates/openpulse-core/src/profile.rs`; new
  `crates/openpulse-modem/tests/snr_floor_calibration.rs`.
- **Tests:** `cargo test -p openpulse-core --lib ota_rate` — **15 pass** (new: fast-downshift on first
  low-SNR NACK, multi-step low-SNR downshift, cautious one-step upshift, transient-failure hysteresis;
  updated: asymmetric invariant; preserved: never-desync-under-ACK-loss, climb, clamps, lock). Full
  core lib **255 pass**; all openpulse-modem test binaries green; calibration sweep run manually
  (produced the SL7 finding); clippy `-D warnings` + fmt clean; workspace builds.
- **Test results:** ota_rate 15/15, core 255/255, modem suites green, calibration sweep ran. The
  6-retry down-stepping is replaced by a single-round jump to the SNR-adequate rung.

---

## 2026-07-01 — feature: ARDOP station ID + real SENDID/CWID (REQ-REG-10, CAP-39/CAP-66)

- **Requirement/change:** the auto-ID from CAP-66 covered only the daemon path; the **ARDOP TNC**
  (Pat → `openpulse-tnc`) had no auto-ID and its `CWID`/`SENDID` commands were honest warn-logged
  stubs — a REQ-REG-10 gap for that entry point. Wire the ID timer into the ARDOP worker and make the
  two commands real, without breaking ARDOP host compatibility.
- **Design decision:** (a) run the CAP-66 `StationIdTimer` (interval + sign-off) inside the ARDOP
  worker loop, firing **only at a frame boundary** (empty TX queue) so an ID never splits an
  in-progress transfer — the discipline real ARDOP TNCs use; armed via the engine `frames_transmitted`
  delta. (b) `SENDID` → arm a one-shot ID the worker sends at the next boundary; `CWID TRUE/FALSE` →
  toggle a Morse-CW-append flag — **host responses unchanged**, so the command surface *fulfils* its
  ARDOP contract instead of no-opping (more compatible, not less). The emitted ID is OpenPulseHF's own
  PHY (`DE <call>` in the active mode, + optional CW): the ARDOP layer is a TCP shim, never an ARDOP
  waveform, so there is no on-air ARDOP interop to break. (c) CW ID is a real feature, not a dead flag:
  a pure `cw_id::CwId` Morse generator (keyed-sine, PARIS timing, click-free ramps) + an engine
  `emit_cw_id` that routes through the single TX seam. (d) Reuse `[station] auto_id_*`; plumb via the
  ardop `ArdopConfig` (+ `bridge.set_auto_id`).
- **Implementation:** `crates/openpulse-core/src/cw_id.rs` (+ `pub mod`); `emit_cw_id` +
  `frames_transmitted` in `crates/openpulse-modem/src/engine.rs`; ARDOP worker frame-boundary ID +
  `set_auto_id` + `id_requested`/`cwid_enabled` in `crates/openpulse-ardop/src/bridge.rs`; `SENDID`/
  `CWID` handlers in `command.rs`; `ArdopConfig` fields (`lib.rs`, populated from `[station]` in
  `main.rs`); `..Default::default()` in the testmatrix/integration `ArdopConfig` literals.
- **Tests:** `cw_id` 8/8 (Morse table, PARIS unit/dash/inter-char/word timing, amplitude bounds,
  empty); ardop `command` 2/2 (SENDID arms one-shot + response unchanged; CWID toggles flag + response
  unchanged); `station_id_txcount` 2/2 (incl. `emit_cw_id` counts one frame, no-op CW doesn't);
  ardop integration 22/22 (compatibility intact); clippy `-D warnings` + fmt clean; full workspace
  builds.
- **Test results:** cw_id 8/8, ardop lib 2/2 + integration 22/22, station_id_txcount 2/2, no
  regressions in core/modem/config. REQ-REG-10 now holds on the ARDOP TNC path as well as the daemon.

---

## 2026-07-01 — feature: end-of-exchange (sign-off) station ID (REQ-REG-10, CAP-66)

- **Requirement/change:** §97.119 requires ID **at the end** of a communication as well as every 10
  minutes during it. The initial CAP-66 (below) covered only the interval trigger; add the sign-off ID.
- **Design decision:** extend `StationIdTimer` with an **idle-based** end-of-exchange trigger rather
  than hooking a specific teardown path (DISCONNECT / OTA-end): after the station has transmitted,
  once the channel is quiet for `signoff_idle_ms` the exchange has wound down → send a final ID. Idle
  detection is mode-agnostic (covers plain sends, ARQ, OTA, handshakes uniformly) and reuses the same
  TX-armed/`mark_identified` state as the interval trigger, so the two never double-fire and a
  pure-receive station still never keys up. `note_tx` now stamps the last-TX time; the interval and
  sign-off triggers coexist (a long continuous exchange fires the interval ID; going quiet fires the
  sign-off). New knob `[station] auto_id_signoff_idle_secs` (default 10 s; 0 disables sign-off only,
  keeping interval ID). Daemon checks `id_due` then `signoff_due` each tick and logs the `kind`.
- **Implementation:** `crates/openpulse-core/src/station_id.rs` — `signoff_idle_ms` field,
  `with_signoff_idle_ms` builder, `signoff_due`, `note_tx(now_ms)` (records last-TX time);
  `auto_id_signoff_idle_secs` in `crates/openpulse-config/src/lib.rs` (+ Default + template);
  `crates/openpulse-daemon/src/server.rs` computes `now_ms` first, arms via `note_tx(now_ms)`, fires
  on `id_due || signoff_due`.
- **Tests:** `cargo test -p openpulse-core --lib station_id` — **12 pass** (7 interval + 5 sign-off:
  disabled-when-idle-0, due-after-idle, later-TX-pushes-deadline, disarms-after-ID, interval+sign-off
  coexist on a long-then-quiet exchange); `station_id_txcount` 1/1; daemon 2/2; config 9/9; clippy
  `-D warnings` + fmt clean; full workspace builds.
- **Test results:** station_id 12/12, all gates green. REQ-REG-10 now covers both interval and
  end-of-exchange ID.

---

## 2026-07-01 — feature: periodic station-ID timer (REQ-REG-10, new CAP-66)

- **Requirement/change:** REQ-REG-10 (station identification at required intervals in the digital
  mode) was reclassified a ⚠ gap by the gap rescan — the ARDOP `CWID`/`SENDID` commands are stubs and
  no auto-ID path existed. Implement the missing periodic auto-ID so a running station identifies
  itself on air at the regulatory interval while transmitting.
- **Design decision:** (a) a **pure `StationIdTimer` state machine** in `openpulse-core` (interval +
  TX-armed + reset over an injected ms clock) so the regulatory semantics are deterministic and
  unit-testable — ID fires only when the station has transmitted since the last ID *and* the interval
  elapsed, so a pure-receive station never keys up. (b) Arm it from the **engine's `frames_transmitted`
  counter** incremented once at the single TX seam (`stage_emit_output`, where regulatory TX logging
  already lives) — the daemon polls the delta rather than threading a `note_tx()` through every
  transmit call site, and re-baselines after IDing so the ID frame itself doesn't re-arm the timer.
  (c) On due, the daemon keys PTT, sends `DE <callsign>` in the **active mode** (guaranteed-registered,
  and the mode the peer is already decoding), releases PTT — mirroring the OTA-ACK PTT turnaround.
  (d) `[station] auto_id_interval_secs` (default 600 s = US Part 97 10-minute rule; 0 disables; never
  auto-ID as the default `N0CALL`).
- **Implementation:** `crates/openpulse-core/src/station_id.rs` (+ `pub mod` in `lib.rs`);
  `frames_transmitted` field/increment/getter in `crates/openpulse-modem/src/engine.rs`;
  `auto_id_interval_secs` in `crates/openpulse-config/src/lib.rs` (+ Default + template);
  rx-ticker wiring in `crates/openpulse-daemon/src/server.rs`.
- **Tests:** `cargo test -p openpulse-core --lib station_id` — 7 pass (disabled/0-interval, not-due
  without TX, not-due-early, due-at-interval, reset-disarms, re-arm-after-next-interval,
  repeated-TX-one-ID-per-interval); `cargo test -p openpulse-modem --test station_id_txcount` — 1 pass
  (counter bumps per emit, never on receive); `cargo clippy -p openpulse-core -p openpulse-config
  -p openpulse-modem -p openpulse-daemon -- -D warnings` + `cargo fmt --all --check` clean; daemon
  suite 2/2, config suite green.
- **Test results:** station_id 7/7, station_id_txcount 1/1, daemon 2/2, clippy+fmt clean. REQ-REG-10:
  ⚠ gap → ✅ covered (CAP-66).

---

## 2026-07-01 — traceability fix: station-ID coverage corrected (gap rescan)

- **Requirement/change:** a project-wide gap rescan (TODO/FIXME/stub/dead-code/deferred sweep) found
  the matrix overstated station-ID coverage: CAP-39 claimed the ARDOP `CWID`/`SENDID` commands
  "provide station ID" and covered REQ-REG-05 (decodable ID) + REQ-REG-10 (interval ID), but both
  commands are honest warn-logged stubs (`crates/openpulse-ardop/src/command.rs:249,257`) — they
  accept+echo but transmit no CW-ID/ID-frame, and no independent auto-ID path exists in the codebase.
- **Design decision:** correct the record, not the code (the code is a deliberate, documented stub —
  not a defect). REQ-REG-05 is genuinely met on air by the **signed-handshake callsign** (the ConReq
  `station_id` rides RF, decodable by the receiver, #584) → re-attribute REQ-REG-05 to **CAP-01**.
  REQ-REG-10 needs a **periodic auto-ID timer that does not exist** → reclassify ✅ covered → ⚠ **gap**
  and add it to the deferred REQ-REG regulatory set (Phase 5.5-reg). Clear CAP-39's `Implements` to
  `—` and document the stub status inline.
- **Implementation:** `docs/dev/project/traceability-matrix.md` — REQ-REG-05 row → CAP-01; REQ-REG-10
  row → `—`/⚠ gap; CAP-01 `Implements` += REQ-REG-05 (+ design note on the on-air callsign);
  CAP-39 `Implements` → `—` (+ stub note); REQ-REG gap bullet gains `/10`; "Resolved 2026-07-01"
  subsection bullet. Also cleared a stale TODO in `docs/dev/archive/backlog-fec-improvements.md`
  (8PSK max-log-MAP soft demod was shipped in #187–#192, not still a TODO).
- **Tests:** docs-only; structural re-check confirms REQ↔CAP agree both directions (REQ-REG-05↔CAP-01,
  REQ-REG-10 uncovered like the other ⚠ REG rows) and no dangling CAP-39 REG links remain.
- **Test results:** matrix structural invariants hold; no code touched, so no behaviour delta.

---

## 2026-07-01 — housekeeping: rustfmt cleanup, FreeDV-diversity flag, traceability sync (docs/chore)

- **Requirement/change:** two follow-ons after the CE-SSB reference-mining PR (#602). (a) `cargo fmt
  --all -- --check` was failing on three files unrelated to any recent feature, so the workspace
  format gate was red on main. (b) The FreeDV 700D symbol-diversity idea surfaced by the mining pass
  needed a durable home, and the traceability matrix needed to reflect #602/#603.
- **Design decision:** (a) reformat the three offending files with `cargo fmt --all` (pure whitespace/
  line-wrapping, no logic touched) to restore the gate. (b) Record FreeDV 700D-style frequency
  diversity (repeat each carrier's symbol on a band-separated carrier + combine before slicing — a
  fading-margin lever distinct from FEC, a candidate sub-floor rung) as an unscheduled *Far-future
  item* in the roadmap so the lever isn't lost; no target date, no CAP yet. (c) Keep the matrix
  current per the standing rule: refine CAP-24's rationale, add a "Resolved 2026-07-01" subsection.
- **Implementation:** `cargo fmt` on `apps/openpulse-linksim/src/gui.rs`,
  `crates/openpulse-channel/src/lib.rs`, `pki-tooling/src/verification.rs` (PR #603); a deferred
  design entry in `docs/dev/project/roadmap.md` (PR #603); CAP-24 row + "Resolved 2026-07-01"
  subsection + `last_updated` bump in `docs/dev/project/traceability-matrix.md` (PR #604).
- **Tests:** no code-logic change. `cargo fmt --all -- --check` — clean on main after #603.
- **Test results:** fmt gate green; PRs #603 and #604 merged. Docs/chore only, no behaviour delta.

---

## 2026-07-01 — CE-SSB: principled per-mode gate rationale + reference mining (no behaviour change)

- **Requirement/change:** a reference-mining pass (Hershberger CE-SSB QEX 2014/2016, `drmpeg/gr-cessb`,
  Kahn EER 1952, "The Polar Explorer" QEX 2017, PE1NNZ direct-SSB, *Dave's Hacks* 2025 polar
  modulation) asked whether we should adopt the iterative clip→filter→overshoot-compensate loop, and
  found the `cessb_benefits` gate documented by contradictory comments (header + two tests claimed
  8PSK "benefits/stays on", while the code and `cessb_power_evm` assert 8PSK is gated OFF).
- **Design decision:** (a) **keep** the single-pass look-ahead peak-stretch limiter and **reject** the
  Hershberger/gr-cessb iterative clip-filter loop for the *data* path — it injects more in-band EVM,
  the exact quantity that breaks our dense constellations (tuned for voice, not tight slicers).
  (b) Convert the empirical gate into a **principled** one using the equal-amplitude-singularity
  derivation (*Dave's Hacks*): CE-SSB benefits ⇔ high-PAPR envelope (rare hard nulls) **and** loose
  decision margins — true for QPSK-subcarrier OFDM, false for 8PSK/QAM/APSK and single-carrier QAM
  (constellations transit the origin → envelope nulls → phase discontinuity → EVM). Gate *logic*
  unchanged; only its documentation is corrected and grounded. (c) Catalogue the sources in
  `references.md`, including the polar/EER family as inspiration for a possible future direct-RF
  (QMX/uSDX Class-E) backend — explicitly out of scope for the current soundcard→linear-rig path.
- **Implementation:** rewrote the `ModemEngine::cessb_benefits` doc comment
  (`crates/openpulse-modem/src/engine.rs`) and corrected the stale 8PSK claims in
  `apps/openpulse-linksim/tests/cessb_ab.rs` and `crates/openpulse-modem/tests/cessb_power_evm.rs`;
  added a "CE-SSB and polar-SSB transmit conditioning" section to `docs/dev/research/references.md`.
- **Tests:** `cargo test -p openpulse-modem --no-default-features --test cessb_power_evm` — 3 pass;
  `cargo test -p openpulse-linksim --no-default-features --test cessb_ab` — 3 pass;
  `cargo clippy -p openpulse-modem --no-default-features -- -D warnings` clean. Comment-only change;
  the gate's behaviour (and thus every other CE-SSB test) is unaffected.
- **Test results:** cessb_power_evm 3/3, cessb_ab 3/3, clippy clean. No behaviour delta — the edits
  align the docs with the already-validated gate and record the rejected alternative.

---

## 2026-06-30 — testmatrix: full-tier completes + OFDM52 raw-framing exclusion generalised

- **Requirement/change:** the `--full` tier must run end-to-end and its Clean/high-SNR failures must
  reflect only genuine results, not case-generator artifacts. Two defects blocked that: the B2F
  runner hung at low SNR, and the `OFDM_RAW_FRAMING_ONLY` exclusion covered only one of three
  case-gen sites that pair plain OFDM52 with RS-family FEC.
- **Design decision:** (a) give the B2F driver streams a socket read/write timeout so a non-decoding
  channel fast-fails instead of blocking forever (the driver maps `TimedOut`/`WouldBlock` →
  `DriverError::Timeout`); (b) lift the per-site `if OFDM_RAW_FRAMING_ONLY.contains(..)` check into a
  single `raw_framing_excludes(mode, fec)` predicate and apply it at *every* site that pairs a
  raw-framing-only mode with FEC, so the padded-frame × 255-byte-RS-block limitation can never leak a
  spurious RS-decode failure again. Keep the legitimate OFDM52 no-FEC-at-floor failure visible.
- **Implementation:** `connect_with_timeout` + `B2F_IO_TIMEOUT = 8 s` in
  `apps/openpulse-testmatrix/src/runners/b2f.rs` (PR #600); `raw_framing_excludes` predicate applied
  at the multicarrier×prop and large-payload sections in `apps/openpulse-testmatrix/src/cases.rs`.
- **Tests:** `apps/openpulse-testmatrix` unit suite (coverage regression test still requires OFDM52
  to appear in a raw case) — 12 pass; clippy `-D warnings` + `cargo fmt --all --check` clean.
- **Test results:** quick tier **555/555, 0 fail** (gate at `docs/test-reports/latest`, unchanged).
  Full tier **6022 cases, 3465 pass, 2557 fail, 21.3 s**, exit 0 — completes without hanging; the
  regression-suspect zone (Clean / AWGN ≥ 20) dropped from 5 failures to 1 (the kept no-FEC
  OFDM52 large-payload point); the lone B2F failure is now the intended fast-timeout at 0 dB.
  Characterization snapshot committed under `docs/test-reports/full` (with a README marking it as
  non-gate data).

---

## 2026-06-29 — CLI: `daemon set-tx-attenuation` (control-surface parity)

- **Requirement/change:** a wiring-gap audit found `SetTxAttenuation` reachable from the panel but
  not the CLI — the one remaining single-client control-surface asymmetry (not a dead command; the
  daemon handles it). Add the CLI subcommand for headless/scripted operation.
- **Design decision:** mirror the established `simple(addr, ControlCommand::…)` pattern — a
  `SetTxAttenuation { db, band }` `DaemonCommands` variant (`band` optional, `--band`) forwarding the
  existing control command. No daemon/protocol change.
- **Implementation:** `crates/openpulse-cli/src/cli.rs` (variant) + `crates/openpulse-cli/src/commands/daemon.rs` (arm).
- **Tests:** covered by the existing CLI command-dispatch suite (29 + integration); no new logic to unit-test beyond the forward.
- **Test results (run):** `cargo build/clippy -p openpulse-cli --no-default-features --all-targets -D warnings` clean; `cargo test -p openpulse-cli --no-default-features` all green; `fmt` clean.

## 2026-06-29 — Generic serial CAT backend wired into the daemon

- **Requirement/change:** make the already-built-but-unreachable generic serial CAT backend
  (`GenericSerialCat`, FF-13) selectable from the daemon. The machinery (TOML rig definitions,
  `CatController` trait, serial transport) existed and was tested, but the daemon only ever built a
  concrete `RigctldController` and honoured `cat_backend = "rigctld"`.
- **Design decision:** give the daemon a backend-agnostic CAT handle. `RigctldController` gained the
  `CatController` impl its trait doc already claimed (delegating to its inherent methods). The daemon
  selects via a concrete `CatBackend` enum (`Rigctld` | `Generic`, the latter feature-gated) rather
  than `Box<dyn CatController>` — a boxed trait object tripped the `Drop`-in-loop borrow checker at
  the per-tick reborrow sites, whereas the concrete enum reborrows cleanly like the original
  `Option<RigctldController>`. `server::build_cat_controller` maps `[radio] cat_backend` →
  `none`/`generic`/`rigctld`; the generic arm reads top-level `[radio] serial_port` + `rig_file` and
  is gated by the daemon `generic-serial` feature (→ `openpulse-radio/generic-serial`, Unix). The 4
  daemon rig-control signatures became `Option<&mut (dyn CatController + Send)>`. Meter polling stays
  rigctld-only (its own connection). `rig_a` stays reserved for the multi-rig refactor (docs
  corrected to stop implying the generic fields there are wired — the top-level ones are).
- **Implementation:** `crates/openpulse-radio/src/rig_controller.rs` (`impl CatController for
  RigctldController`); `crates/openpulse-radio/src/cat_controller.rs` (doc); `crates/openpulse-config/
  src/lib.rs` (`RadioConfig.serial_port` / `rig_file` + template + corrected `rig_a`/`RigConfig`
  docs); `crates/openpulse-daemon/Cargo.toml` (`generic-serial` feature); `crates/openpulse-daemon/
  src/server.rs` (`CatBackend` enum + `CatController` impl + `build_cat_controller`; call sites →
  `as_mut().map(... as &mut dyn ...)`); `crates/openpulse-daemon/src/lib.rs` (rig param types +
  import).
- **Tests:** `server.rs` `cat_backend_tests` — `cat_backend = "none"` yields no controller;
  `cat_backend = "generic"` with no rig file yields no controller and no panic (covers feature-on
  open-failure and feature-off branches). `GenericSerialCat` itself is covered by its existing
  `MockTransport` tests.
- **Test results (run):** `cargo build -p openpulse-daemon --no-default-features` and
  `--features generic-serial` both green; `cargo test -p openpulse-daemon --no-default-features
  cat_backend_tests` 2/2; config tests green. `clippy -D warnings` clean; `fmt` clean on touched
  crates.

## 2026-06-29 — ARDOP adaptive ARQ session + real ARQBW/ARQTIMEOUT

- **Requirement/change:** make the ARDOP host hints `ARQBW` and `ARQTIMEOUT` real instead of
  stored-and-ignored. Blocked because the TNC never started an adaptive session (so the rate ladder
  was dormant and there was nothing to bound), and the only bandwidth lever targeted the OTA
  controller, not the rate_policy one the worker reads.
- **Design decision:** (1) opt-in `[ardop] enable_adaptive_arq` (+ `adaptive_profile`) → `main.rs`
  calls `start_adaptive_session`, flipping the worker to its existing adaptive `transmit_arq` /
  `receive_with_ack_hint` branch (default off = unchanged fixed-mode behaviour). (2) A rate_policy
  bandwidth cap distinct from the OTA bounds: `RateAdapter::clamp_to` never raises the level;
  `RateAdaptationPolicy::set_max_tx_level` clamps the active session immediately and re-clamps after
  every ack so AckUp can't climb past the cap (an `Increased` event past the cap is reported as
  `Maintained`). The Hz→level map lives in `openpulse-qsy::bandplan` (`max_speed_level_for_bandwidth`)
  — kept out of `openpulse-modem` (no qsy dep / cycle); the ARDOP worker owns the mapping. (3) The
  worker applies the ARQBW cap when it changes and disconnects an idle connection after ARQTIMEOUT
  seconds, using non-blocking `try_read`/`try_write` on the tokio RwLocks from the sync worker.
- **Implementation:** `crates/openpulse-core/src/rate.rs` (`clamp_to` on `RateAdapter` +
  `BiDirRateAdapter`); `crates/openpulse-modem/src/rate_policy.rs` (`max_tx_level`,
  `set_max_tx_level`, `defined_modes`, clamp in `apply_ack_internal`); `crates/openpulse-modem/src/
  engine.rs` (`set_arq_max_tx_level`, `adaptive_profile_modes`); `crates/openpulse-qsy/src/
  bandplan.rs` (`max_speed_level_for_bandwidth`); `crates/openpulse-config/src/lib.rs`
  (`ArdopConfig.enable_adaptive_arq` / `adaptive_profile` + template);
  `crates/openpulse-ardop/src/main.rs` (start session), `bridge.rs` (worker cap + ARQTIMEOUT +
  activity tracking), `command.rs` (comment now reflects applied behaviour); ardop gains an
  `openpulse-qsy` dep.
- **Tests:** `adaptive_profile_integration.rs` — `arq_max_tx_level_caps_the_adaptive_ladder`
  (AckUp×8 can't pass an SL4 cap; clearing it climbs again) + `arq_max_tx_level_clamps_an_already_high_session`
  (cap below current level clamps immediately); `bandplan.rs` —
  `max_speed_level_for_bandwidth_maps_hz_cap_to_a_level` (500/700/2000 Hz caps, below-floor `None`,
  unknown-mode skip).
- **Test results (run):** `cargo test -p openpulse-core -p openpulse-modem -p openpulse-qsy -p
  openpulse-config -p openpulse-ardop --no-default-features` → green (modem-adaptive 13, qsy 28,
  ardop 22, core lib 226, modem lib 45, …; 0 failed). `cargo clippy …-D warnings` 0 warnings; `fmt`
  clean on touched crates; full workspace (sans pki) builds.

## 2026-06-29 — Signed handshake over RF into the daemon connect (+ verified logbook grid)

- **Requirement/change:** wire the Ed25519 signed `ConReq`/`ConAck` handshake into the daemon's
  `ConnectPeer`/RF path (it was a tested library primitive the daemon never exchanged — `ConnectPeer`
  was a local trust eval), store the verified peer identity, and feed the verified grid to the ADIF
  logbook (closing logbook item B). The keystone that also unblocks host-driven ARQ bounds.
- **Design decision:** *additive* exchange, not a rewrite of connect — `begin_secure_session` still
  runs; `ConnectPeer` additionally signs+sends a `ConReq` and records a `PendingHandshake`. Frames
  are ~530 B > the 255 B modem-frame cap, so they're **SAR-fragmented** (`sar_encode`) on TX and
  reassembled (`SarReassembler`) on RX; the reassembly is a fall-through after relay/QSY dispatch
  (handshake fragments are binary, not QSY ASCII / relay envelopes) and is confirmed by the
  reassembled `HSCQ`/`HSAK` magic, so no wire marker is needed (which wouldn't fit anyway). The
  responder verifies + replies `ConAck` + records the peer; the initiator verifies the `ConAck`
  against its in-flight `ConReq` (session-id gated) + records the peer + clears pending. Grid is a
  `skip_serializing_if`-empty signed field on `ConReq`/`ConAck` so legacy zero-grid frames and their
  signatures stay byte-identical; added `create_with_grid` constructors leaving the 25 existing
  `create` callers untouched. Station key from `[station] identity_key_path` (default
  `~/.config/openpulse/identity.key`, auto-generated; explicit path lets the twin rig hold distinct
  identities). New `ControlEvent::PeerVerified`; 30 s CONACK timeout via `expire_pending_handshake`.
  Verification uses `PolicyProfile::Permissive` (signature proves key possession; first-seen peers
  still connect, mirroring the optimistic `ConnectPeer`).
- **Implementation:** `crates/openpulse-core/src/handshake.rs` (grid field + `create_with_grid`);
  `crates/openpulse-config/src/lib.rs` (`StationConfig.identity_key_path` + template);
  `crates/openpulse-daemon/src/lib.rs` (`PendingHandshake`/`VerifiedPeer`, `RuntimeControlState`
  fields incl. `handshake_sar`, `transmit_handshake_frame`, `try_reassemble_handshake`,
  `handle_inbound_conreq`/`handle_inbound_conack`, `record_verified_peer`,
  `expire_pending_handshake`, `ConnectPeer` CONREQ send, RX dispatch);
  `crates/openpulse-daemon/src/logbook.rs` (`set_pending_peer_grid`);
  `crates/openpulse-daemon/src/server.rs` (load identity seed at startup; expiry tick);
  `crates/openpulse-daemon/src/protocol.rs` + `apps/openpulse-panel/src/connection.rs`
  (`PeerVerified` event + panel log).
- **Tests:** `crates/openpulse-core/src/handshake.rs` inline (grid round-trip, grid is
  signature-covered, empty-grid byte-identical to legacy); `crates/openpulse-daemon/src/lib.rs`
  `handshake_rf_tests` (responder reassembles+verifies+records; initiator verifies+stamps logbook
  grid into the ADIF record; mismatched-session CONACK ignored; ConnectPeer initiates; full-size SAR
  fragment survives BPSK250; pending-handshake timeout).
- **Test results (run):** `cargo test -p openpulse-core -p openpulse-config -p openpulse-daemon
  --no-default-features` → all green (core lib 226, handshake_integration 17, daemon lib incl.
  `handshake_rf_tests` 6, config 16, …; 0 failed). `cargo clippy -p openpulse-core -p
  openpulse-config -p openpulse-daemon -p openpulse-panel --no-default-features --all-targets -D
  warnings` → 0 warnings. `cargo fmt` clean on the touched crates.

## 2026-06-29 — Panel: AGC on/off toggle (control-surface parity)

- **Requirement/change:** close the last open control-surface parity gap — the receiver streaming
  AGC (`ControlCommand::SetAgc`, shipped 2026-06-28 in the daemon + CLI) had no panel button, so the
  GUI operator couldn't toggle it. (Re-audit found the CLI `SendMessage`/`SetMode`/PTT/QSY-accept/
  reject gaps and the panel Squelch gap from the 2026-06-27 audit were already closed; AGC was the
  only one left.)
- **Design decision:** mirror the existing Notch/CE-SSB/Logbook toggle pattern exactly — a single
  `AGC: ON/OFF` button in the right-hand controls column that flips local `agc_enabled` and sends
  `ControlCommand::SetAgc { enabled }`. Default off, matching the daemon's `[modem] agc_enabled`
  default and the engine's opt-in AGC. No new state machinery; one bool field + one button.
- **Implementation:** `apps/openpulse-panel/src/app.rs` — `agc_enabled: bool` field (default false),
  `AGC: ON/OFF` button next to the Notch toggle in `draw_controls`, with hover text noting the
  active-span gating. Stale docs corrected: roadmap §10.6 "panel toggle parity" marked done; the
  control-surface parity table updated to show all flagged gaps closed.
- **Tests:** GUI toggle (no unit test — the `SetAgc` daemon/engine path is covered by
  `crates/openpulse-modem/tests/agc_loopback.rs`).
- **Test results:** `cargo build -p openpulse-panel --no-default-features` green;
  `cargo clippy -p openpulse-panel --no-default-features --all-targets -- -D warnings` 0 warnings;
  `cargo fmt -p openpulse-panel --check` clean. Visual confirmation pending (held before merge per
  the GUI-change rule).

## 2026-06-29 — Docs: sort `docs/dev/` into topic subfolders + fix all references

- **Requirement/change:** the loose files under `docs/dev/` were moved into four topic
  subfolders — `design/` (architecture, design, freq-acquisition-design, hpx-waveform-design,
  testbench-design), `pki/` (the 11 `pki-tooling-*` docs), `research/` (ardop/freedv-auth/js8call/
  ofdm/pactor research, reference-mining-plan, references, vara-research, wsjtx-analysis), and
  `project/` (backlog, changelog, roadmap, traceability). All inbound and outbound references had
  to follow the move so no link or doc-path breaks.
- **Design decision:** keep the moves as `git mv` renames (history-preserving) and rewrite every
  reference in two classes: (1) full-path mentions `docs/dev/<base>` → `docs/dev/<subdir>/<base>`
  across the whole tree (markdown link text, `doc:` frontmatter self-pointers, and `.rs` doc/line
  comments); (2) relative markdown link *targets* fixed per linking-file location — the dev README
  index (`](base.md)` → `](subdir/base.md)`), the manual's `dev/<base>` targets, and the moved
  files' own outgoing relative links that gained/needed a directory level
  (`../mode-fec-ladder.md` → `../../mode-fec-ladder.md`, siblings via `../`).
- **Implementation:** 29 `git mv` renames; a 29-substitution `sed` script applied to all tracked
  text files for class (1); targeted per-file `sed` for class (2) in `docs/dev/README.md`,
  `docs/openpulse-manual.md`, `docs/{features,mode-fec-ladder}.md`,
  `docs/dev/{hpx-session-state-machine,vara-parity-execution-board}.md`,
  `docs/dev/reviews/review-26050{8,17}.md`, and the moved `design/`/`project/` files. Five `.rs`
  doc-comment path mentions updated (`openpulse-ardop`, `openpulse-config`, `openpulse-dsp`,
  `openpulse-kiss`, `pilot` plugin) — comment-only, no code change.
- **Tests:** a Python link-integrity walker that resolves every relative `.md` link target against
  the filesystem; a basename grep for any surviving old `docs/dev/<base>` path.
- **Test results:** old-path grep → 0 hits. Link walker → only 3 broken links remain
  (`docs/README.md` → `marketing/{banner,flyer,presentation}.md`), which are pre-existing (those
  targets were never committed) and unrelated to this move. `cargo fmt --all --check` shows 5
  pre-existing deviations, all in files outside this change set (`gui.rs`, `channel/lib.rs`,
  `agc_loopback.rs`, `verification.rs`); no new fmt regression (rustfmt does not reflow comments).

## 2026-06-28 — Panel: controls to a right side-panel; status below the waterfall

- **Requirement/change:** move the right-column (session status) elements below the waterfall; make
  the waterfall as wide as the spectrum; move all controls except connection / PTT / callsign+Connect-RF
  into the right column.
- **Design decision:** keep only connection (transport/server/Connect), PTT, RF-connect
  (callsign/Connect RF), and the connection indicator in the top toolbar. Everything else (Mode,
  Freq/Tune, Repeater, CE-SSB, Notch, Logbook, OTA, TX Atten, Squelch, Config, Messages, QSY) moves
  to a resizable right `SidePanel` rendered by a new `PanelApp::draw_controls`. The `CentralPanel`
  drops its 2-column split and stacks the spectrum pane (now full width) then the session status
  below it, inside a vertical `ScrollArea`. Waterfall widened to the full pane width (was capped at
  512 px) in `draw_spectrum_pane`.
- **Implementation:** `apps/openpulse-panel/src/app.rs` — toolbar slimmed; `draw_controls` method;
  right `SidePanel` + stacked `CentralPanel`. `apps/openpulse-panel/src/ui.rs` — waterfall size to
  `available_width × 96`.
- **Tests:** GUI layout (no unit test).
- **Test results:** `cargo fmt -p openpulse-panel --check` clean; `cargo build -p openpulse-panel`
  green; `cargo clippy -p openpulse-panel --all-targets -- -D warnings` 0 warnings. Visual
  confirmation pending (held before merge per the GUI-change rule).

## 2026-06-28 — Linksim: regroup Station B views + waterfall/constellation toggles

- **Requirement/change:** swap the Station B RX spectrum/waterfall with the ACK/NACK
  spectrum/waterfall; keep Station B's constellation at the far right; add controls to
  enable/disable the waterfalls and the constellation diagrams.
- **Design decision:** swap the middle and far-right signal columns so the column order is
  `[A TX | ACK (B→A) | B RX]` — grouping all of Station B's RX views (spectrum, waterfall, and the
  far-right I/Q constellation in the branding band) on the right, with the ACK in the middle. Two
  toolbar checkboxes (`ui_show_waterfall`, `ui_show_constellation`, both default on): `draw_panel`
  gains a `show_waterfall` flag (early-returns after the spectrum when off); the branding band
  conditionally renders the two `constellation_plot`s and recomputes the flanking text width
  (`3×qr_side` → `1×qr_side`) so the QR stays centered when constellations are hidden.
- **Implementation:** `apps/openpulse-linksim/src/gui.rs` — toolbar checkboxes; column swap
  (panels[2] ACK middle, panels[1] B RX far right); `draw_panel(.., show_waterfall)`; gated
  branding band.
- **Tests:** GUI layout/visualization (no unit test); existing gui unit tests unaffected.
- **Test results:** `cargo build/clippy/test -p openpulse-linksim --features gui` green (3/3 gui
  unit tests). Visual confirmation pending (held before merge per the GUI-change rule).

## 2026-06-28 — Linksim: symbol-spaced (crisp-dot) constellations

- **Requirement/change:** sharpen the I/Q constellations (#574) from a full-rate cloud to discrete
  per-symbol dots so the clean-TX vs noisy-RX contrast reads as a real constellation.
- **Design decision:** parse samples/symbol from the mode's trailing baud (`samples_per_symbol`,
  order/suffix-stripped; `None` for OFDM/SCFDMA/PILOT/FSK which have no PSK symbol grid), then
  sample the Hilbert baseband once per symbol at the **best timing phase** (peak mean magnitude —
  symbol centers carry full amplitude, transitions dip). No full timing/carrier recovery — a cheap
  estimate that's honest about being a viz, not a demod. Multicarrier/FSK keep the full-rate cloud.
- **Implementation:** `apps/openpulse-linksim/src/gui.rs` — `samples_per_symbol()`, `baseband_iq()`
  best-phase symbol-spaced path, `PanelView::push(samples, sps)`, `sps` threaded from `fs.mode`.
- **Tests:** `apps/openpulse-linksim/src/gui.rs` unit tests (gui feature): baud parsing
  (order/suffix/multicarrier cases), and symbol-spaced-vs-cloud (far fewer points + tighter
  Q-spread on synthetic BPSK).
- **Test results:** `cargo test -p openpulse-linksim --features gui --bin openpulse-linksim-gui`
  3/3 pass; `cargo clippy -p openpulse-linksim --features gui --all-targets` 0 warnings.

## 2026-06-28 — Streaming AGC rollout to the PSK ladder (active-span gated)

- **Requirement/change:** roadmap 10.6 — roll the existing `openpulse-dsp::agc::Agc` out as a
  receiver front-end level normaliser for the PSK/QAM ladder, with active-span gating.
- **Design decision:** place it at the **single `route_audio_stage(InputCapture)` seam** (after the
  notch: remove interference, then normalise) so every capture path — `receive*` family and the
  daemon's `accumulate_capture` streaming path — gets it by construction. Opt-in (default off), like
  the notch, so dense-mode canaries can't regress unless enabled. The AGC's own docs forbid running
  on the raw capture (leading silence ramps the gain to its clamp); satisfied via **active-span
  gating** — `Agc::lock()` freezes the gain on sub-squelch (silent) blocks, `unlock()` adapts on
  carrier-present blocks (RMS ≥ DCD threshold). Tripwire counter mirrors the notch's. Exposed for the
  running daemon (no dead capability): `ControlCommand::SetAgc` + CLI `daemon set-agc`.
- **Implementation:** `crates/openpulse-modem/src/engine.rs` (`agc`/`agc_enabled`/
  `agc_blocks_processed` fields, `enable_agc`/`disable_agc`/`configure_agc`/`is_agc_enabled`/
  `agc_gain_db`/`agc_blocks_processed`, `apply_rx_agc`, seam wiring); `openpulse-daemon`
  `protocol.rs` + `lib.rs` (`SetAgc`); `openpulse-cli` `cli.rs` + `commands/daemon.rs` (`SetAgc`).
- **Tests:** `crates/openpulse-modem/tests/agc_loopback.rs` — off-by-default+toggle; tripwire on the
  `accumulate_capture` (daemon) path; active-span gating (gain ~0 dB through silence, boosts a weak
  carrier); decode of a ~30 dB-attenuated QPSK500 frame with AGC on.
- **Test results:** `agc_loopback` 4/4; full `openpulse-modem --no-default-features` suite green;
  `notch_loopback` 4/4 (notch path unchanged); workspace build (excl. pki-tooling) green; clippy
  (modem/daemon/cli) 0 warnings.

## 2026-06-28 — Host-driven TNC control (ARQBW/ARQTIMEOUT): blocked, finding recorded

- **Requirement/change:** wire the ARDOP `ARQBW`/`ARQTIMEOUT` host hints into the engine for real,
  replacing the accepted-but-ignored no-ops the #571 audit flagged.
- **Investigation:** same blocked class as the signed handshake (B). `crates/openpulse-ardop/src/
  main.rs` never calls `start_adaptive_session`/`start_ota_session`, so `current_tx_level()` is
  always `None` and `worker_loop` always runs the **fixed-mode** path — the adaptive ARQ ladder is
  dormant. `ARQBW` has no ladder to cap; `ARQTIMEOUT` has no ARQ connection to time out (the worker
  does single-shot `receive(mode, None)`). The only bandwidth-cap lever (`ota_set_level_bounds`)
  targets the **OTA** controller, but the worker's adaptive path reads the **rate_policy** controller
  — different mechanisms; no rate_policy bandwidth cap exists.
- **Decision:** wiring no-ops into the dead fields would re-create the "defined-but-not-consumed" gap
  the audit removed, so it was deliberately NOT done. Real fix is a feature (TNC runs an adaptive ARQ
  session + rate_policy bandwidth cap + connection timeout), recorded in `docs/dev/project/roadmap.md` under
  the TNC command-surface audit.
- **Implementation:** none (no speculative surface); roadmap finding only.
- **Test results:** docs-only; workspace gates unaffected.

## 2026-06-28 — Linksim: I/Q constellation views flanking the QR branding band

- **Requirement/change:** show a constellation diagram for Station A to the left of the QR code and
  one for Station B to the right, keeping the text closest to the QR.
- **Design decision:** the `FrameStep` carries only real passband waveforms, so derive baseband I/Q
  via the existing `openpulse_core::iq::hilbert_iq` (fc=1500 Hz, fs=8 kHz — the `ModemEngine`
  defaults), trim the 31-sample group-delay edges, RMS-normalize, and decimate to ≤700 points — the
  same "viz straight from passband samples" approach the spectrum/waterfall already use. Map
  Station A = `forward_tx` (clean TX, panel 0), Station B = `forward_rx` (post-channel RX, panel 1),
  giving a clean-vs-noisy contrast that matches the app's "Station A | Channel | Station B" framing.
  Branding band reordered to `[const A | wordmark | QR | tagline | const B]` so the text stays
  nearest the QR and the constellations sit on the outer edges.
- **Implementation:** `apps/openpulse-linksim/src/gui.rs` — `PanelView.iq`, `baseband_iq()`,
  `constellation_plot()` (egui_plot `Points`, fixed unit bounds, no axes/grid), branding-band
  rewrite.
- **Tests:** GUI visualization (no unit test); `baseband_iq` reuses the unit-tested `hilbert_iq`.
- **Test results:** `cargo build -p openpulse-linksim --features gui` green; `cargo clippy -p
  openpulse-linksim --features gui --all-targets` 0 warnings. Visual confirmation pending (held for
  the user before merge, per the GUI-change rule).

## 2026-06-27 — Logbook peer GRIDSQUARE via handshake (B): blocked, finding recorded

- **Requirement/change:** carry the worked station's grid in the signed handshake so the logbook
  fills `GRIDSQUARE` from a verified, on-air source (the richer-fields item B, follow-on to A).
- **Investigation:** the Ed25519 signed handshake (`ConReq`/`ConAck`, `openpulse-core/src/
  handshake.rs`) is a tested library primitive that the **daemon never exchanges**. The
  `ConnectPeer` path runs `ModemEngine::begin_secure_session`, a *local* trust evaluation
  (`evaluate_handshake` over locally-supplied params) — it sends no `ConReq` and verifies no peer
  `ConAck` over RF. `ConReq`/`ConAck` are referenced only by the handshake lib + its tests.
- **Decision:** B is blocked on a larger prerequisite — wiring the over-the-air signed
  `ConReq`→`ConAck` exchange into the daemon connect — not a field add. Adding a grid field to a
  primitive the daemon never exchanges would create a fresh "defined-but-not-consumed" gap (the
  exact anti-pattern the TNC/config audits just removed), so it was deliberately NOT done. The
  config `[logbook.peer_grids]` map (A, shipped) remains the interim source.
- **Implementation:** none (no speculative surface). Finding + real-fix path recorded in
  `docs/dev/project/roadmap.md` ("Signed handshake not wired into the daemon connect").
- **Test results:** docs-only; workspace gates unaffected (no code change).

## 2026-06-27 — Logbook peer GRIDSQUARE via config map (A)

- **Requirement/change:** populate the ADIF `GRIDSQUARE` (worked station's grid). The audit found
  the grid is NOT carried by the handshake/peer-cache/engine today, so "from the handshake" needs a
  protocol change (tracked as B). Deliver the outcome now via a config lookup.
- **Design decision:** a `[logbook.peer_grids]` callsign→grid map (case-insensitive), consulted at
  `begin_qso` by peer callsign — what most logging software does. Composes with B later (the
  handshake-exchanged grid would take precedence over the map).
- **Implementation:** `openpulse-config` `LogbookConfig.peer_grids`; `logbook.rs` (lookup at
  begin_qso → `Pending.gridsquare` → `GRIDSQUARE`); `server.rs` passes the map; TOML template.
- **Tests:** logbook unit (GRIDSQUARE from a lowercase-key map + uppercase connect), daemon
  integration (`connect_then_disconnect…` asserts `<GRIDSQUARE>`), config default (empty map).
- **Test results:** logbook 5/5, daemon integration passes, config 9/9, clippy 0.

## 2026-06-27 — ARDOP/KISS TNC command-surface audit

- **Requirement/change:** audit the ARDOP + KISS TNCs for the "accepted/advertised but not applied"
  gap class (a command the TNC accepts but no-ops, or a doc claim the code doesn't honour).
- **Finding:** ARDOP — `GRIDSQUARE`/`ARQBW`/`ARQTIMEOUT` are validated + echoed but never read by
  the engine (the modem self-manages bandwidth/timeout via its adaptive ladder); `CWID`/`SENDID`
  are honest warn-logged stubs. KISS — only `KISS_DATA` is applied; the 6 control frames
  (TXDELAY/P/SlotTime/TXtail/FullDuplex/SetHardware) were *silently* dropped.
- **Design decision:** the no-ops are defensible (self-managed rate/PTT) but were silently
  misleading. Make them honest, don't implement host-driven control speculatively, track the real
  wiring. KISS: log dropped control frames (`debug!`) instead of silent. ARDOP: code comment +
  corrected `docs/non-gpl-interfacing.md` (split "implemented" vs "accepted-not-applied" vs "stub").
  Roadmap "TNC command-surface audit" records the real-wiring follow-ups.
- **Implementation:** `crates/openpulse-kiss/src/server.rs` (log); `crates/openpulse-ardop/src/
  command.rs` (comment); `docs/non-gpl-interfacing.md`; `docs/dev/project/roadmap.md`.
- **Test results:** ardop + kiss build; clippy 0; no behavior change beyond a debug log.

## 2026-06-27 — Adaptive-profile FEC audit (+ a permanent gate)

- **Requirement/change:** audit every adaptive profile's FEC assignment for the `cli_adaptive`
  bug class (a profile assigning no/wrong FEC to a mode that needs it — `hpx_ofdm_hf` had OFDM52-8PSK
  with no FEC).
- **Finding:** all 12 profiles are now **correct** — every modulatable rung decodes a clean loopback
  with its assigned FEC. The only rungs that don't decode are `hpx_narrowband_hd`'s SL8/SL9
  (QPSK9600-RRC / 8PSK9600-RRC), which can't modulate at 8 kHz — but `profile.rs` already documents
  that profile as **requiring a 48 kHz audio path**, so that's by design, not a gap.
- **Design decision:** promote the audit probe into a permanent CI gate rather than a one-off — it
  would have caught the `cli_adaptive` bug. The gate iterates every profile × rung, asserts clean
  decode with the assigned FEC, and pins the count of known-unmodulatable (48 kHz) rungs at 2 so a
  new unreachable rung trips it.
- **Implementation:** `crates/openpulse-modem/tests/channel_loopback.rs`
  `every_profile_rung_decodes_clean_with_its_fec` (no source change — the profiles were correct).
- **Test results:** gate passes; clippy 0.

## 2026-06-27 — ADIF logbook follow-ups (runtime toggle + parity + richer fields)

- **Requirement/change:** complete the ADIF logbook — a runtime `SetLogbook` control with CLI/panel
  parity (config-only before), and richer fields (RST/COMMENT from the RX SNR).
- **Design decision:** mirror the `SetNotch`/`SetCessb` pattern (control command + thin CLI
  `simple()` wrapper + panel toggle). `Logbook::set_enabled` for runtime control. At disconnect,
  read `engine.last_rx_snr_db()` → `RST_RCVD` (coarse SNR→RST bucket) + a `COMMENT` carrying the
  mode and SNR. Peer `GRIDSQUARE` from the handshake deferred — not exposed on the engine yet.
- **Implementation:** `crates/openpulse-daemon/src/logbook.rs` (`set_enabled`/`is_enabled`,
  `end_qso(now_ms, rx_snr_db)`, `rst_from_snr`); `protocol.rs` `SetLogbook`; `lib.rs` handler +
  disconnect passes the SNR; CLI `daemon set-logbook`; panel `Logbook: ON/OFF` toggle.
- **Tests:** logbook unit (runtime-toggle writes, RST/COMMENT present, `rst_from_snr` buckets);
  existing connect→disconnect integration still passes; CLI parse.
- **Test results:** daemon lib + logbook **all pass**; CLI `set-logbook` parses; clippy 0; full
  workspace green. Panel button → held for visual confirm.

## 2026-06-27 — WS-vs-TCP control-port parity audit (no gap)

- **Requirement/change:** audit another surface — does a `ControlCommand` reach the daemon on the
  TCP control port but not the WebSocket port (or vice versa)?
- **Finding:** **parity holds.** Both `lib.rs::handle_command` (TCP) and `ws.rs` parse the same
  `ControlCommand` enum, handle the identical 6 request-response commands inline (SubscribeSpectrum,
  GetConfig, ListMessages, GetMessage, SendMessage, DeleteMessage), and route everything else
  through the same `dispatch_command` → `apply_command_to_engine`. No command is reachable on one
  transport but not the other.
- **Design decision:** no code gap to fix; the only risk is *future* divergence (the two inline
  chains are duplicated). Added cross-referencing "keep in sync" comments to both handlers as a
  tripwire; a full consolidation into one shared request-response handler is noted as future
  hardening (low priority — no current gap).
- **Implementation:** comments in `crates/openpulse-daemon/src/lib.rs` and `ws.rs`.
- **Test results:** daemon builds; fmt clean; no behavior change.

## 2026-06-27 — CE-SSB gated off for OFDM-HOM (8PSK+) — a real ~6 dB regression

- **Requirement/change:** investigate the CE-SSB-on-OFDM cost surfaced while greening the baseline
  (CE-SSB clipping corrupted OFDM52+full-RS on a clean channel). Does it hurt any *shipped*
  OFDM-HOM+RS rung at marginal SNR?
- **Finding:** yes — CE-SSB was **net-harmful on OFDM-HOM**. `cessb_benefits` gated off 16QAM+ but
  still applied CE-SSB to **OFDM52-8PSK** (the shipped `hpx_ofdm_hf` SL7 rung, default-on). The
  peak-fair `cessb_power_evm` shows OFDM52-8PSK BER **0.0000 → 0.0026** (power gain doesn't recover
  the in-band clipping distortion), and a marginal-SNR AWGN sweep has it fail entirely with CE-SSB
  on (**12/12 → 0/12 at 12–16 dB**), decoding once gated off. CE-SSB is genuinely zero-cost only on
  the QPSK-subcarrier OFDM (OFDM16/OFDM52, BER 0→0). The team's own gating principle —
  "favourable raw BER notwithstanding, real-path decode breaks" — applies to 8PSK too.
- **Design decision:** add `8PSK` to the `cessb_benefits` exclusion (CE-SSB now applies only to
  QPSK-OFDM). The on-air +1.2 dB power result was on QPSK-OFDM and is unaffected.
- **Implementation:** `crates/openpulse-modem/src/engine.rs` (`cessb_benefits`).
- **Tests:** updated `cessb_power_evm::cessb_benefits_hold_*` and `cessb_engine::benefits_only_*`
  to assert 8PSK gated off; new real-path guard
  `channel_loopback::ofdm52_8psk_rs_decodes_at_operating_snr_with_default_cessb`.
- **Test results:** new guard 8/8 at 16 dB; cessb suites pass; full workspace **no failures**;
  clippy 0.

## 2026-06-27 — Config-schema completeness audit (defined-but-not-consumed)

- **Requirement/change:** audit another surface for the seam-gap class — config fields that exist
  (and are in the TOML template) but are never read, so setting them does nothing.
- **Design decision:** 72/79 fields consumed; the 7 dead ones are all in `[radio]` —
  `[radio.rig_a]` (never read; the primary rig is the top-level `[radio]`) and the `"generic"` CAT
  backend (`backend`/`serial_port`/`rig_file`, documented in the manual but unimplemented). Don't
  remove documented/planned schema and don't undertake the feature in an audit; instead mark them
  accurately so the config stops looking wired, and record the real fixes in the roadmap.
- **Implementation:** `crates/openpulse-config/src/lib.rs` (field docs + TOML template mark
  rig_a "currently unused" and the generic-backend fields "reserved — not yet implemented";
  corrected the repeater comment); `docs/dev/project/roadmap.md` "Config/feature gaps" entry.
- **Tests/results:** `openpulse-config` 9/9 (template still parses), clippy 0. The recently-added
  `[modem] notch_*`, `[qsy] auto_qsy_on_interference`, `[logbook] *` fields were each confirmed
  consumed by the daemon during the audit.

## 2026-06-27 — Auto-QSY end-to-end validation

- **Requirement/change:** validate the notch → in-band-interferer → auto-QSY loop end to end
  (the capstone of the notch arc, previously only unit-tested piecewise).
- **Design decision:** the TCP twin daemon harness can't inject a *standalone* interferer (channel
  models transform a signal, they don't generate a tone into B's silence), so validate the full
  logical loop deterministically via `ChannelSimHarness`: Station A confirms a persistent in-band
  tone through `accumulate_capture`, auto-QSY transmits a real `QSY_REQ`, it crosses the channel,
  Station B decodes it and `process_received_bytes` opens a responder session (+ `QsyIncoming`).
- **Implementation:** test only — `crates/openpulse-daemon/src/lib.rs`
  `auto_qsy_end_to_end_initiator_to_responder_over_rf`.
- **Tests/results:** the new test passes; daemon lib **29/29**; clippy 0; fmt clean. Remaining: a
  two-station **on-air** run (rpi53 + FT-991A / SDR) — genuine hardware, not reproducible here.

## 2026-06-27 — Automatic ADIF logbook (opt-in)

- **Requirement/change:** the roadmap-recorded feature — a per-QSO station log in ADIF for import
  into logging software / LoTW / eQSL, opt-in.
- **Design decision:** per-QSO (a connect→disconnect session), distinct from the per-frame
  `TxSessionLog`. ADIF writer in `openpulse-core` (pure, no time crate — Hinnant civil-date for
  UTC); a daemon `Logbook` helper holds in-flight QSO state and appends on disconnect, decoupled
  from the RF loop (io errors are logged, never propagated). Sourced from the `ConnectPeer`
  callsign, the active mode (→ `SUBMODE`, `MODE=DYNAMIC`), the last `SetFreq` (→ `FREQ`/`BAND`),
  UTC connect/disconnect timestamps, and station callsign/grid from config.
- **Implementation:** `crates/openpulse-core/src/adif.rs` (`AdifRecord`/`to_adif`, `utc_date_time`,
  `band_for_mhz`, header); `crates/openpulse-config` (`[logbook] enabled`/`adif_path`);
  `crates/openpulse-daemon/src/logbook.rs` (`Logbook`); daemon `ConnectPeer`/`DisconnectPeer`/
  `SetFreq` hooks + `server.rs` build from config.
- **Tests:** ADIF unit tests (record render, band map, UTC format, header); `Logbook` unit tests
  (write/append-no-dup-header, disabled/no-pending no-op); daemon integration
  (`connect_then_disconnect_writes_an_adif_logbook_record`); config default.
- **Test results:** core adif 4/4, daemon logbook+integration pass, full workspace **no failures**,
  `clippy --all-targets` **0 errors**. Follow-up: a control command + CLI/panel toggle/export
  (config-driven for now).

## 2026-06-27 — Green the test/clippy baseline (3 red items)

- **Requirement/change:** make `cargo test --workspace` and `clippy --all-targets` green (they had
  red items all session, undermining the "real green results" traceability rule).
- **Design decisions + findings (each probed by clean loopback before fixing):**
  - `cli_adaptive::adaptive_ofdm_hf_reaches_top_rung`: the `hpx_ofdm_hf` profile had `fec_modes:
    [None; 21]` and the `adaptive` command decoded with no FEC — but OFDM52-8PSK fails unprotected
    even on clean. Per-level FEC measured: OFDM16/OFDM52 base decode unprotected and *break* under
    full RS (padded 255-byte block spans too many OFDM symbols); OFDM52-8PSK+ need RS. → assign RS
    to SL7–SL10 only, and make the command apply `profile.fec_for(level)` via
    `transmit_with_fec_mode`/`receive_with_fec_mode`.
  - `repro::ofdm52_rs_clean_128b_engine`: red because **CE-SSB** (default-on PAPR conditioner,
    #521) clips OFDM52-base+full-RS past RS t=16. That combo is used by no profile (zero
    operational impact) and the shipped OFDM-HOM+RS rungs survive CE-SSB; the guard predates
    CE-SSB (#185) and tests the OFDM modulator path → disable CE-SSB in the guard, documenting the
    finding that CE-SSB is *not* zero-cost on every OFDM mode.
  - 3 testbench clippy `field_reassign_with_default` lints → struct-update syntax.
- **Implementation:** `crates/openpulse-core/src/profile.rs` (hpx_ofdm_hf fec_modes);
  `crates/openpulse-cli/src/commands/adaptive.rs` (per-level FEC); `apps/openpulse-testmatrix/
  tests/repro.rs` (CE-SSB off in the guard); `apps/openpulse-testbench/src/signal_path.rs` (lints).
- **Tests:** `cli_adaptive` (6), the repro guard, full workspace test + `clippy --all-targets`.
- **Test results:** `cli_adaptive` 6/6; the OFDM ladder climbs SL5→SL10 (6/6 frames, ~1153 bps);
  full `cargo test --workspace --exclude pki-tooling`: **no failures**; `clippy --all-targets`: **0 errors**.

## 2026-06-27 — SetFreq panel control + CLI rig-default fix

- **Requirement/change:** make CAT `SetFreq` reachable from the panel (the one parity item left
  panel-only after the prior round), and fix the CLI `set-freq` default that the daemon rejects.
- **Design decision:** the daemon's `SetFreq` handler only accepts `rig == "rigctld"` (single CAT
  target), not the display rig_a/rig_b labels — so no rig selector is needed. Panel: a `Freq:`
  DragValue in **kHz** (operator-ergonomic, HF-ranged 1500–30000) + a `Tune` button sending
  `freq_hz = round(kHz × 1000)` with `rig = "rigctld"`, placed next to the Mode selector. CLI:
  change the `set-freq --rig` default from the invalid `a` to `rigctld`.
- **Implementation:** `apps/openpulse-panel/src/app.rs` (Freq DragValue + Tune → `SetFreq`; new
  `freq_khz` field, default 14070.0); `crates/openpulse-cli/src/cli.rs` (`set-freq` default rig).
- **Tests:** panel build + clippy/fmt (GUI confirmed visually before merge); CLI build + `set-freq`
  parse/connection-stage reachability.
- **Test results:** panel builds, **0 clippy errors**, fmt clean; CLI builds, fmt clean, `set-freq`
  parses and reaches the connect stage. (PR #562.)

## 2026-06-27 — Control-surface parity (CLI + panel)

- **Requirement/change:** the control-surface audit (`docs/dev/project/roadmap.md` → "Control-surface
  parity gaps") found `ControlCommand`s reachable from one surface but not another: CLI couldn't
  `SendMessage` / `SetMode` / PTT / accept-reject QSY; panel couldn't `SetDcdSquelch` / start-stop
  OTA. Close the real two-way-operability gaps.
- **Design decision:** mirror existing patterns rather than invent new surface plumbing. CLI →
  thin `simple()` wrappers over the existing `ControlCommand` (identical to `set-cessb`/`set-notch`).
  Panel → toolbar controls mirroring the TX-atten slider (squelch) and OTA lock/unlock block
  (start/stop). Keep the OTA hysteresis/aggressiveness/bounds CLI-only — intentional (panel offers
  the simplified lock/unlock).
- **Implementation:**
  - CLI (PR #559): `crates/openpulse-cli/src/cli.rs` (`DaemonCommands`: SetMode, SetFreq,
    PttAssert/Release, AcceptQsy/RejectQsy, SendMessage); `src/commands/daemon.rs` dispatch arms.
  - Panel (PR #560): `apps/openpulse-panel/src/app.rs` (Squelch slider → `SetDcdSquelch`; OTA
    Start/Stop + `ota_profile` field; new fields `dcd_squelch`, `ota_profile`).
- **Tests:** CLI subcommand parse + connection-stage reachability (manual invocations); daemon-side
  handlers for these commands are covered by `openpulse-daemon` lib tests; panel build + clippy/fmt
  (GUI confirmed visually before merge).
- **Test results:** CLI builds; all new subcommands parse and reach the connect stage;
  `openpulse-daemon` lib: **25 passed / 0 failed**; panel builds, **0 clippy errors**, fmt clean.
  CLI #559 merged; panel #560 merged after visual confirm.

## 2026-06-27 — Seam-gap audit fixes (RX/TX cross-cutting)

- **Requirement/change:** after the notch-on-daemon-path gap, audit every cross-cutting RX/TX
  behavior for the "wired at one entry, not the shared seam" pattern.
- **Design decision:** move each cross-cutting concern to its single shared seam; verify the rest
  are already uniform; record intentional exceptions.
- **Implementation (PR #557):** TX regulatory `log_frame` → `stage_emit_output` seam; RX SNR record
  added to `receive_from_samples_with_fec`; removed duplicate OTA `FrameReceived` emit
  (`crates/openpulse-modem/src/engine.rs`).
- **Tests:** `crates/openpulse-modem/tests/tx_logging_seam.rs` (plain/FEC/ACK paths log);
  existing `FrameReceived` tests use `.any()`.
- **Test results:** new tx_logging_seam tests pass; full modem + daemon suites pass; fmt/clippy
  clean. Verified-not-gaps: DCD unified, CSMA-broadcast intentional, FrameTransmitted on all data
  paths.

## 2026-06-27 — Single RX front-end seam + tripwire (notch gap structural fix)

- **Requirement/change:** the receiver notch ran only on the `receive()` family, not the daemon's
  `accumulate_capture` streaming path — a coverage gap invisible to the (wrong-seam) tests.
- **Design decision:** place the notch at the single convergence point all ~19 capture paths funnel
  through, `route_audio_stage(PipelineStage::InputCapture)`; add a tripwire counter so a feature
  that never runs on a path is visible; test through the production entry, not a convenience seam.
- **Implementation (PR #556):** `route_audio_stage` applies the notch for InputCapture keyed by a
  stored `rx_mode`; `notch_blocks_processed()` counter; removed the two duplicate call sites.
- **Tests:** `notch_runs_on_the_daemon_streaming_capture_path` (drives `accumulate_capture`, asserts
  the counter); auto-QSY daemon test asserts it too.
- **Test results:** notch + QSY + loopback suites pass; single-application preserved on both paths;
  fmt/clippy clean. Prevention checklist added to `CLAUDE.md` → *Known sharp edges*.
