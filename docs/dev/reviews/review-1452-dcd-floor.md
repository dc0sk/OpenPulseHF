---
project: openpulsehf
doc: docs/dev/reviews/review-1452-dcd-floor.md
status: resolved
last_updated: 2026-09-29
---

# Adversarial review — #1452 stage 1: a squelch floor that follows the band behind any filter

Reviews by Fable, all read-only and prompted for falsification: design v1 (verdict: revise; its
option 1 was falsified on the corpus), the corrected measurement and design v2, and the finished
stage. The design drafts and every probe log are parked outside the repo
(`~/parked/openpulse-1452/`).

## Consumer

- The threshold is set at the `InputCapture` seam (`engine.rs`, `update_dcd_at_seam`), which every
  capture path passes.
- Burst gathering: `accumulate_routed` → the daemon rx tick (`server.rs`, `accumulate_capture`) →
  the OTA arm, the non-OTA arm, the monitor and the repeater.
- Channel busy: `DcdState::is_busy` → CSMA (`server.rs`), the repeater's carrier sense
  (`repeater/src/lib.rs`), discovery's beacon deferral.
- The operator squelch: `set_dcd_squelch` ← `[modem] dcd_squelch`, `dcd_squelch_bands`,
  `SetDcdSquelch`, `apply_band_squelch`.

Found by: `grep -n "set_threshold\|is_channel_busy\|accumulate_capture\|set_dcd_squelch"` over the
modem, daemon and repeater crates.

## Prior art

- Mercury `modem/channel_busy.c` (read from source during review): a lower quartile of ~150 Hz band
  averages and a spectral-excess busy criterion — the across-band quartile that fails behind a narrow
  filter, and the criterion stage 2 takes up.
- Martin's minimum statistics / IMCRA: per-bin noise estimation under coloured noise.
- #1254 (chunking invariance), #1055 (the adaptive squelch), #1304 (the wideband mirror), #1255 (a
  cap flush is not ladder evidence), #1443 (no pre-trigger ring), #1150 (the busy hold).

## Twins

- The repeater senses rig_b through the same seam and estimator (#1325), so a narrow-filter rig_b had
  the same defect and gets the same fix.
- The CLI `EnergyGate` keeps its own time-domain floor; not changed.

---

## Prompt

Every round was read-only, prompted for falsification ("test the instinct rather than confirm it;
flag anything wrong or unproven in the framing"), and handed the raw logs and the code, not a summary.

- **Round 1 — design v1 and the first probe.** Three options for the floor (a spectrally-shaped
  across-band estimate, a per-bin time quantile, a very long window), the 0/32 narrow-filter probe,
  and the claim that the slab mechanism was the defect.
- **Round 2 — the corrected measurement, design v2 and the texts.** Probe v2 (in-band SNR, a
  reach-control that decodes the same slab from the frame's offset, a decodability control), the
  per-bin estimator, a ring frozen while gathering, the operator value, the clamp, and a draft of
  the sensitivity issue (#1454).
- **Round 3 — the finished stage.** The diff, the eight tracker tests and seven acceptance tests,
  the sabotage matrix S1–S7 with logs, the full-suite runs, and the draft ledger entry.
- **Round 4 — this write-up**, after the round-3 corrections and the maintainer's step-up and
  flicker decisions.

## Verdict

- **Round 1: revise; do not build option 1.** Option 1 was falsified on the corpus at every
  passband mask. Option 2's per-bin time quantile reads the true idle RMS within 0–6 % on all five
  captures. The probe's narrow-filter columns sat below BPSK250 + Rs's decode cliff on in-band SNR,
  so 0/32 was not attributable to the slab without an offset-decode control and an in-band-SNR
  control. The 1.25 margin is ~2σ on a 250 Hz filter. The seam overwrote the manual squelch every
  window. And for the entry rungs, what limits sensitivity is the total-power criterion, not the
  floor — a separate question, filed as #1454, with Mercury's spectral-excess criterion as prior art.
- **Round 2: probe v2 sound; stage 1 revise.**
  - Report the 500 Hz rows with their uninformative trials named.
  - "+17 dB" for BPSK31 was wrong: ≈ +12.5 dB in-band with an unbiased floor.
  - The premise that a frame shorter than 0.75·W cannot move the quantile was false (floor
    1.09×–1.99× at f = 0.1…0.8), so the ring must freeze while gathering and release on a cap flush.
  - The 0.001 clamp already governed the FT-991A capture (squelch/idle 1.67).
  - The flicker guard must derive its length from the candidates, not from `min_frame_samples`.
  - Two acceptance criteria were unsatisfiable as written ([1.2, 1.5] → [1.20, 1.35]; one-block idle
    bursts).
  - The operator value should be a lower bound.
  - Stage 1 may ship alone only with the flicker guard, because a failed flicker decode demotes
    the rate controller.
- **Round 3: fix first.** Paste-able verdict:

  > Round 3 (finished stage 1, read-only): the estimator (per-bin trimmed mean over a 16 s history, derived constants re-checked), the hold/commit/discard seam, the operator lower bound and the 1e-4 clamp are sound and sabotage-verified on their main arms; the seven acceptance tests and the twin rig discriminate, and every headline number in the ledger matches its log except three that must be corrected — the deviation-(c) "each alone 1.17–1.18" (the discard arm on the final code reads 1.142, S6), the `bpsk31_long_frame` "pre-existing flake" (binary times quoted as test times; `main` was never run under load; failure was `invalid magic`), and the claim that a full suite has passed (the last full run predates the final HEAD). Four design consequences are unstated and must be: an upward band step is followed only at the next cap flush (≈5 min on `hpx_hf`'s entry candidates); the flicker guard is inert above SL6 (OFDM preamble 288 samples < one tick) while the wide-filter false-trip rate is now a measured 0.1 %, each trip keying a budgeted NACK and counting toward demotion — v2's k-of-n question is still open; one-shot paths can freeze; and `dcd_squelch()`/`FrontEndState` now read back the effective squelch, breaking the panel slider's round trip. Three pre-registered acceptance criteria (one-block idle bursts, the PTT spy on 250 Hz idle under OTA, the wide/BPSK250 non-regression) were dropped without a deviation note, and the long-frame gate should assert the split count it prints. Fix these, fill the review artifact's placeholder sections, then run the gate.

  Applied, besides the text: the long-frame gate asserts its split count (0/8); the idle gate is
  bounded by the BPSK250 preamble; `FrontEndState` reads back the operator value; the noise-floor
  docs are corrected. The step-up consequence led to a maintainer decision (a longest-frame flush),
  which was built and then removed: sized from the uncoded frame it split every coded long frame, and
  over every FEC it equals the cap (#1455). The OFDM flicker gap led to a persistence rule, and the stale-hold case to a guard; each has its
  own test and sabotage.
- **Round 4: fix first (text).** Paste-able verdict:

  > Round 4 (the write-up, read-only; one targeted run 9/9 in 12 s): every headline number matches its log except the squelch/idle range (1.22–1.26, not 1.23–1.26), the "9/9 in 13 s" (two logs, 8+1, no combined run logged) and the test file's "0/5 … 5/5" (the ledger's 0/8 … 5/8 with three trials named uninformative). The band-bound removal is complete and REQ-DCD-01's "no longer than the longest candidate frame" still holds (Turbo 37.0 s under a 37.3 s cap). The stale-hold test exercises its scenario (S9 proves the silence went through the hold) and S8 discriminates the block clause. Three overclaims must go: "idle flicker cannot key a NACK — including on the OFDM rungs" (two-block flickers, measured at 800 samples on 250 Hz, still count above SL6); the persistence rule's "block" is a READ, and a read holding a whole frame (every twin-rig frame; a daemon read after a blocking decode, the cpal ring being unbounded) makes that frame's failure non-evidence with no test observing it and the twin suite not yet re-run; and "recovers at the first cap flush" is a code read — which also shows the block after the flush re-arming a hold the accumulator never releases, so the floor freezes at the committed value until the next burst ends. Round 3's one-shot step-up freeze is still absent from Limitations; the book's `dcd_squelch = 0.01` line, the reachability baseline's deleted symbol, CAP-72's tests list and one 1.17 in the seam comment are stale. Fix the text, then run the gate; nothing here needs code.

  Applied, and one finding went further than the verdict. The orphaned hold after a cap flush was
  read as benign ("frozen at a correct value"). Measured through the accumulator — 20 s of idle, a
  6 dB step up for 60 s, then back down for 60 s — the squelch recovered at the cap flush and then
  stayed frozen: 2.488× the quiet idle for the whole minute after the drop, so it was a code defect,
  not text. The accumulator now releases a hold no burst owns;
  `the_floor_follows_the_band_up_at_the_cap_and_back_down` gates it (1.240× after the drop), and
  sabotage S10 (release removed) fails that test alone.
- **Round 5: the flicker rule, before rebuilding it (design).** Verdict: revise, then build. The
  persistence rule counted reads, and measured, a 16 000-sample failure delivered in one read was
  dropped as flicker. The proposed two-term duration bound (candidate preamble, profile-wide
  `min_frame_samples`) was shown dominated by one term: the candidates' shortest `min_frame_samples`,
  which is the engine's scan floor, not a frame (corrected in 6a) (Round 2 objected to the configured mode's geometry;
  the term is the same field taken over the candidates). And no persistence bound makes idle
  flicker non-evidence — it sets a rate (~p^k per block), and the NACK streak has no time decay.
  Maintainer: build the duration rule, file the decay (#1456). Built with five of the six gates the
  review specified (merged into two tests plus the existing ones); the preamble and `max_frame_samples`
  sabotages each fail them. The sixth, a real coded frame delivered in one read, is covered by the
  twin suite rather than this file (it uses 16 000 samples of loud noise, which proves the rule, not a
  decode).
- **Lessons (reviewed).** L1 (a decision option carrying an unmeasured bound) and L3 (a gate below
  the mechanism's threshold) are the same lesson and already in `second-opinion`, `defect-archetypes`
  and this file's #1000/#1384 rows; no skill change. L2 (a reviewer's "benign" answered with prose)
  earned one edit to `second-opinion`: "this is benign / harmless" joins its list of negative claims,
  and its trigger names a Limitations sentence written instead of the fixture. L4 (read size as the
  regime) is already in `defect-archetypes` #2.
- **Round 6 (the write-up) and 6a (the SL1 term).** Fix first: the branch failed clippy
  `--all-targets` on two test-file lints, and the only gate log on the branch ended mid-run with no `GATE:` line, the
  twin suite had not run since the OTA path changed, and `min_frame_samples` is the whole 17 s frame on
  MFSK16 — with SL1 the sole candidate every fade-split fragment keyed no NACK, a regression against
  `main`. Design answer: build, taking the term the engine already computes (`frame_scan_geometry`'s
  `acq + step`), and state the `hpx_pilot*` shift. Applied: the SL1 edge is pinned both ways (2 047
  drops, 2 048 counts; the preamble sabotage fails 2 047, the `min_frame_samples` sabotage fails 2 048
  and the pilot cell), and the text corrections are in.
- **Round 6b (final text).** Every number in the ledger, the CLAUDE.md row, the engine comment and
  the decay issue matches its log and the 6a pilot figures; ship after re-citing a run made on the
  committed tree, renaming a stale helper in the ledger, and one leftover "floor" in the engine
  comment. All three applied.
- **Round 7 (the repeater's cold sense, design).** The workspace gate on the first final HEAD failed
  the #1325 carrier-sense gate: the repeater senses rig_b only just before a relay, so its floor is
  cold, and a cold floor learns an occupant as the band. Maintainer: fail closed while cold. Verdict:
  build, with five revisions, all applied. The verdict order is Busy > Unreadable > Cold > Clear (Cold
  ranked first would let a dead card defer forever). "Warm" is a new tracker-level definition. The
  floor is primed at session start (without it a real card would, by construction, defer the first relay and learn the whole
  inter-relay backlog as the floor — the capture buffer has no cap; not measured on hardware). The names say warm, not calibrated. The ledger states the residual as
  an occupant filling most of the first cold read. The three sabotages each fail the tests the ledger names for them (the
  cold-check removal fails two). Other transmit decisions on a cold floor (KISS CSMA, discovery's beacon deferral) share a
  one-window exposure and go with #1454.
- **Round 7b (the write-up).** Fix first: the twin bullet named repeater tests the twin suite does not
  have (the `carrier_sense = false` tests are in `openpulse-daemon`'s other files); the 13/13
  citation predated the counter removal; "relays are rarely deferred" and "hardware defers the first
  two relays" were unmeasured inferences; the session-start prime is exercised by no discriminating
  test and joins the not-sabotage-verified list; the #1454 reproduction's probe source was not
  retained. Numbers otherwise match their logs. All applied; the probe source is parked with its log.
- **Round 7c (applied without a round).** The gate on the corrected HEAD passed every test (2 619)
  and failed only the reachability ratchet: `warm_sensor` was public for an integration test. It is
  private, with an in-crate unit test; the full-duplex integration test warms rig_b through a
  deferred first relay instead. Mechanical, per the ratchet's own rule; the no-op-prime sabotage was
  re-run and fails the new unit test.
- **Round 8 (the held-out suites).** After `GATE: PASS` on `5de83961`, `scripts/slow-tests.sh` was
  run: `ota_channel_adaptation` 3/3, `notch_rescues_interferer` 2/3, the rescue test failing its own
  negative control. The same test fails the same assertion on `main` (`4eb13f95`), so it predates
  this change (#1457). Fix first on the write-up: the ledger had claimed the held-out suites were not
  reached (both pass the seam; only the OTA one consumes the result); "fails identically" overstated
  a comparison across profiles and concurrency; the cause window is `884d96ed..4eb13f95` (18
  acquisition-path merges), not two named PRs; and the REQ-QRM-01 row claimed a proof route that is
  red. All applied; the row now says it is not currently proven.

