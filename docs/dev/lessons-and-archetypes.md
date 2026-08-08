---
project: openpulsehf
doc: docs/dev/lessons-and-archetypes.md
status: living
last_updated: 2026-08-08
---

# Lessons ↔ defect archetypes

The **general, reusable defect archetypes** — the recurring *shapes* a bug takes, and the probe that
exposes each — now live in an external, project-agnostic library: the `defect-archetypes` skill in
[coding-agent-skills](https://github.com/dc0sk/coding-agent-skills). That library is deliberately
domain-neutral so it can be reused across projects; it carries the shapes and probes, not our
constants.

**This doc is the OpenPulse-specific half.** It is where the DSP/HF specifics stay: which archetypes
this codebase has actually been bitten by, with the concrete instances — issue IDs, file:line, the
measured numbers — mapped to the general shape. Two reasons it exists:

1. **Retain the hard-won learnings.** The specifics were kept out of the general library on purpose;
   they must not be lost, so they live here with full provenance.
2. **Know where to look first.** An archetype this codebase has repeated is one it is *prone* to —
   a scan should start where we have already bled.

The authoritative long-form write-ups remain in **[`../../CLAUDE.md`](../../CLAUDE.md) → "Known
sharp edges"** and the point-in-time **[archetype scan](reviews/archetype-scan-2026-07-29.md)** (18
findings, all closed); this doc distills and indexes them by shape rather than restating them. New
lessons: add the one-line operator note to [`memories.md`](memories.md) and, if it is a new *shape*
or a new instance of a known one, a bullet here.

> Archetype numbers below follow the skill's current catalog (Family A #1–9, Family B #10–17). Names
> are the stable key; if the skill renumbers, match on the name.
>
> **Cite symbols, not line numbers.** This doc is `status: living` and its first line-number
> citations were already stale when checked ten days later — `BURST_MAX_SAMPLES` had been renamed to
> `BURST_MAX_CAP_SAMPLES` and moved several hundred lines, and a paired constant no longer existed at
> all. A symbol name survives refactoring and greps to the truth; `file.rs:514` silently becomes a
> confident pointer at unrelated code. Qualify the crate too — this repo has two `server.rs`.

---

## Family A — blind verification

*Found by auditing the apparatus: read the check/fixture/constant and ask what it structurally cannot
see.*

### #1 Self-consistent checker — the test shares the code's wrong convention
- **Wire-whitening unit test in the wrong regime (#1021 / scan #17).** `scramble.rs` —
  `an_all_ones_block_becomes_transition_rich` unpacked **MSB-first while the wire packs LSB-first**,
  counted identical-bit runs where the real NRZI dead-carrier is a run of *zero* bits, and checked
  keystream offset 0 instead of the real RS-padding offset (~28) — three regime mismatches at once.
  A constructed real-regime dead-carrier sequence passes both assertions, so the defect the test
  guards could recur unseen. The module's own doc already warned of this trap for a sibling function.

### #2 Artificially-easy fixture — the harness makes easy what reality does not
- **`ChannelSimHarness` hands the receiver a buffer that *is* the frame.** A real receiver listens
  for seconds and the frame sits *somewhere inside*; every `route*` variant filled the RX loopback
  with exactly the transmitted samples, so a whole class of frame-*location* bug was structurally
  invisible. It hid a live one: `QPSK250+rs` **passed at a ~7 s capture window and failed at the
  default 45 s one** — same mode/FEC/level/payload. Use `route_embedded(lead, trail)` for anything
  that must prove the receiver can *locate* a frame.
- **Frame-lock test discards the mislocation field (scan #13).** `waveform_lock_watterson.rs` read
  only `res.rho` and never checked `res.offset`, so 35–45 % of "locked" frames were locked onto the
  delayed multipath ray and the test counted them as success. Cited as "Frame lock reliability ≥99%".

### #3 Artifact-calibrated constant — a number measured under conditions that no longer hold
- **A ρ threshold cannot come from an AWGN decode column + an SSB-noise corpus (#1053).** The QPSK
  extension of the #1049 preamble veto was built on the ρ ceiling of two recorded captures and the
  weakest ρ that still decodes on AWGN. Both too narrow: `QPSK250-D` on `moderate_f1` at its own 7 dB
  floor decodes to **ρ = 0.276**, *below* its recorded idle-noise ceiling of 0.291 — distributions
  overlap, no threshold separates them. A 500 Hz receive filter lifts idle ρ above even BPSK250's
  shipped 0.40. Two captures from two rigs is a corpus of **one regime** (SSB bandwidth). Fix:
  publish the threshold *with the template* (`PreambleTemplate`), corroborated against codec2/modem73.
- **`fec_slice_factor` measured on BPSK250 only (scan #15).** Ratio table measured on one plugin for
  6 of 10 `FecMode` variants; the "every geometry already reserves a full 255-byte RS block"
  justification is false for MFSK16 (one block, zero margin) → permanent 2× reserve.
- **The gate for hand-listing had itself hand-listed (#1093, PR #1096).** `fec_scan_long_capture`
  exists *because* a length fix reached two arms of a five-arm dispatch; its acceptance row claimed
  "every FEC arm, not just the two that were fixed first" while the test named **five of the ten**
  variants. Line coverage cannot see this — the missing arms *are* executed by other tests; what was
  absent is the **combination** (arm × long capture). Fix shape, reusable for any closed set:
  enumerate from the type (`FecMode::ALL`), make a new member **fail to compile** (`all_index`'s
  exhaustive match), and state a reason for every exclusion in the output. First run: two arms nobody
  had ever swept passed, and `Turbo` failed — unreceivable on the scanning path while holding the
  *highest* negotiation strength, so `negotiate` prefers a mode it cannot serve (#1093).
- **Stale `×3` FEC-reserve rationale (scan #16).** `coded_noise_settle_recovery.rs` documents a `×3`
  factor a same-week commit already changed to `×2`; the `223,872`-sample figure is stale
  (`149,248` is correct at factor 2).

### #4 Seam gap — cross-cutting behaviour attached to one caller, not the shared seam
- **RX front-end DSP must sit at `route_audio_stage(InputCapture)`, not a caller.** Captured audio
  reaches demod by two routes — the `receive*` family and the daemon's streaming path
  (`accumulate_capture`, what `server::run`'s `rx_ticker` actually uses). The original receiver-notch
  bug put the transform in `stage_capture_input` only: covered the `receive`-family tests, never ran
  in the daemon. Tripwire: `notch_blocks_processed()` stays 0 if an enabled feature skips a path.
  See the **Cross-cutting RX/TX checklist** in CLAUDE.md.
- **`transmit_iq` bypasses the wire-whitening seam (scan #8).** 13 of 15 modulate sites go through
  `stage_modulate_payload` (whitens); `transmit_iq` reaches `plugin.modulate_iq()` via
  `route_wire_stage`, which applies no transform, while RX un-whitens unconditionally → `invalid
  magic` on any same-build receiver. Latent (no production callers) — the reason it is medium.

### #5 Environment-as-SUT — mutable rig/environment state mistaken for the system
- **Eight modes misclassified as "analog-path limited" (#1008–#1010).** A clean-looking three-rung
  comparison (in-process pass, snd-aloop pass, dual-card fail) actually measured a **live capture
  AGC**: unplugging USB adapters reset their mixers and the runner `continue`d past unresolved cards
  with `|| true`. Ablated, `SCFDMA52-16/32QAM` **FAIL 2/2 AGC-on, PASS 2/2 AGC-off**. "Amplitude
  modes fail, phase modes pass" read as physics because an AGC moves level *during* a frame. Lessons:
  "passes on A, fails on B" isolates a variable only if everything else is equal; the AGC had been
  struck off by **reading** the control (`sget` said off) *after* runs that reset it, not by
  ablating it; a script that *sets* state must *read it back* (`AGC_PREFLIGHT`).

### #6 Blind sibling path — one of two twin paths is instrumented, the other silent
- **RX SNR recorded only on the soft-demod branch (scan #10).** Every hard-FEC rung (differential
  SL6, the whole Rs/RsStrong lower half) recorded nothing; the gate was keyed on soft-demod
  *capability* rather than SNR-estimator availability. Consumers affected: QSY scan, ADIF `rx_snr`.
- **Non-OTA burst decode failures swallowed while the OTA sibling logs (scan #11).**
  `openpulse-daemon/src/server.rs` `res.payload.unwrap_or_default()` with no log vs the OTA arm logging every failure — a
  one-line mirror fix.

### #7 Unreachable guard — the trigger can't occur in the case it exists to handle
- **Settle-recovery precondition sized from the FEC reserve, not the frame (#1021 / scan #4).**
  `window_complete` required `onset + max_frame_samples` where `max_frame_samples` is the `frame_plan`
  reserve (2× real). BPSK31 needs **149.2 s** of post-onset audio; the dual-card listen window is
  **109.6 s** — unreachable. The one regression gate pinned BPSK250, the single mode where the
  precondition is satisfiable, so a fix had no test.

### #8 Container-derived length — a size taken from the container, not the content *(the systemic one)*
This is OpenPulse's dominant shape — **scan cluster 1, four findings**, all a receive-side window
sized from the *container* (a FEC reserve, a fixed burst cap) rather than the *payload*, all on the
slow/coded end of the ladder, all invisible to gates whose fixture never grows the buffer past where
the defect fires.
- **A flat 30 s burst cap force-flushed mid-frame (scan #1).** BPSK31+Rs = 66.6 s,
  BPSK63+Rs = 33.3 s both exceeded the cap → carrier force-split into two preamble-less bursts on
  every normal frame. Reachable on the daemon default (`hpx_hf`, SL2). The cap and the ladder's
  actual frame lengths were two numbers nobody had compared.
  **Fixed:** the flat cap became a per-mode `ModemEngine::burst_cap_samples` with the 30 s figure
  demoted to a runaway *floor* (`BURST_MAX_CAP_SAMPLES`, `openpulse-modem/src/engine.rs`), gated by
  `tests/burst_cap_frame_length.rs`.
- **Scanning FEC window byte count is a function of the window (`decode_prefix` fix).** `end =
  (start + max_frame_samples).min(len)` → the exact-multiple-of-255 gate rejected every attempt
  before RS ran. Fix: `FecCodec::decode_prefix` tries successively longer prefixes.
- **LDPC decode loop hard-fails on trailing-noise codewords (scan #2).** `chunks_exact` decodes every
  codeword in the over-reserved slice; a real capture always outlasts the frame → a trailing-noise
  codeword fails BP and `?` aborts the whole frame, reported as "LDPC did not converge" — a channel
  message for a length bug.
- **`RsInterleaved` fails at *every* capture length, including its gate's (scan #3).**
  `Interleaver::deinterleave` builds its permutation from `data.len()`, so a window-length buffer is
  unscrambled with a different permutation than the transmitter used. The **organizing lesson of the
  whole scan**: the sentence *"`RsInterleaved` is untouched since it deinterleaves first and needs
  the exact length"* was written as a reason to skip a sibling arm and it **described the defect** —
  now a CLAUDE.md rule: *a reason to skip a sibling deserves the same measurement as the fix.*

### #9 Subject-conditioned sample — the sample was selected by an outcome the subject influenced
- **Threshold-tuning measured only among inputs that decoded (#1049/#1053).** Asking whether raising
  the 0.40 detection threshold would discard usable inputs, by measuring the miss rate among inputs
  that **decoded** — while the shipped 0.40 gate ran *inside* that decode. Inputs under 0.40 were
  rejected, never decoded, and left the sample; the miss rate at 0.40 was zero by construction. The
  gate's rejection counter showed 1010–1696 rejections per 30 trials, **zero** ever succeeding. This
  instance is the one carried (de-identified) in the general skill.
- **Coverage certifies what it cannot see (#1092).** A reachability sweep found **139** public items
  with no production caller but *with* test callers, against **16** with neither. Coverage reports
  the 139 as **covered** — it does not merely miss them, it vouches for them, and a stricter
  threshold makes this worse by rewarding tests that exercise unwired code. The 16 report 0 %,
  indistinguishable from a genuinely untested live function. Cross-checked: all 10 resolvable
  orphans show execution count 0 in `cargo llvm-cov`, against a positive control at count 20 —
  two independent instruments agreeing. Coverage baseline at that commit: 77.78 % lines.

---

## Family B — unfalsified explanation

*Found by distrusting the story: take an accepted explanation and construct the observation that
would refute it. This is where the expensive weeks go.*

### #10 Flat response = bug, not limitation
- **A modem that fails at *every* SNR has a bug, not a limitation (#685).** SC-FDMA
  `dft_ce_estimate` mis-reconstructed every frequency-selective channel; its signature was a **flat
  2–7 % Watterson decode rate from 8 to 32 dB**, recorded as "correct and by design" for two
  releases. Found by *taking the noise away* — a static two-ray FIR inside the CP at 90 dB SNR, where
  a receiver that cannot decode has nowhere to hide. Replacement: `channel::DelayCe` (live in `plugins/scfdma`). `dft_ce_estimate` no longer exists — it is named here as history, not as a pointer.
- **SNR estimator reads the fade as noise (#934).** BPSK had no `estimate_snr_db`, so the M2M4
  fallback read a **flat ≈ −4 dB across 20 dB** of true SNR and the rate controller decided on a
  constant. Fix shape: remove a per-window least-squares complex gain first
  (`constellation::additive_snr_db_windowed`). *Third occurrence* of the identical bug.

### #11 Symmetric fixture, asymmetric defect
- **Sync must lock ahead of the peak, never on it (#688).** A matched filter's argmax sits on
  whichever multipath ray is instantaneously strongest — the delayed one about half the time — and a
  late window start pulls in the next symbol (the CP only protects an *early* start). SC-FDMA lost
  half of all Watterson frames. The tell: a **symmetric** static two-ray test passes either way; the
  reproduction needs `a_delayed > a_direct`.

### #12 Generalized past the validity boundary
- **"`RsStrong` is free" is true only ≤191 B.** Measured "free" at 64 B and generalised straight
  past the block boundary; at 192–223 B it needs a second RS block and doubles airtime, dropping
  `hpx_hf` AWGN goodput 310→199 bps through the CI goodput floor, which caught it. `Rs` is the
  ladder-wide default; `RsStrong` only where frames stay under 191 B.
- **`condemned_floor` measured on BPSK250, applied to every mode (#1045).** On QPSK500 (no preamble
  template) each condemnation raised the floor through `.max()` until the gate sat *above the signal*
  — FAIL where removing it is OK. Visible only once #1049 removed the BPSK justification. Removed
  2026-07-31; the removal is the lesson.

### #13 Proxy metric that does not track the objective
- **Nine profiles claim FEC while assigning `FecMode::None`; the gate can't see it (scan #5/#6).**
  The regression gate `every_profile_rung_decodes_clean_with_its_fec` measures decode on a
  **noiseless** channel, where uncoded decode always succeeds — so reintroducing the exact
  missing-FEC bug it names leaves it green. (Scope corrected on measurement: one profile actually
  defective, not nine — most derived their floors uncoded.)
- **HARQ goodput gate discards the decode result (scan #12).** `harq_rate_selection_watterson.rs`
  computes goodput from arithmetic over the decision struct and `let _ = ` the decode — green with
  **0 of 105 frames** actually decoding (confirmed by instrumentation).
- **An uncoded-BER win is not a win.** SC-FDMA's IBDFE halved uncoded BER and moved coded frame
  success by *zero* (confidently-wrong bits destroy soft FEC). Always take the coded number.
- **Vacuous AFC/Watterson test (scan #7).** `afc_doppler_watterson.rs` has no `WattersonChannel`, no
  decode; its `<±5 Hz` assertion is unreachable (guard needs >50 estimates, fed 64 symbols) and
  passes with an impossible threshold. Cited as done with three `[x]` boxes.

### #14 Inert mechanism
- **`HarqPolicy` has zero production callers (scan #12).** Retry-FEC escalation + SNR-scaled ACK
  timeout run only inside the test suite; `transmit_arq_ota_within` takes FEC from
  `SessionProfile::fec_for` + `free_rs_strengthening`, never `HarqPolicy`. Two traceability docs mark
  it "FUNCTIONALLY COMPLETE" — true only because the selector never runs.
- **`RsInterleaved` is inert; code strength is the lever.** A ≤223-byte payload is one RS block and a
  single block is position-agnostic — nothing to spread (BPSK250 @5/8 dB identical to `Rs`). The docs
  say the opposite; measure the rung, don't trust the table.
- **Cold-start AGC seams with zero callers (scan #14).** `route_with_capture_agc` /
  `route_embedded_with_cfo` start the AGC cold → an ×80 startup spike no real radio produces; a
  prospective trap for whoever wires them up.

### #15 Double-applied correction
- **LLRs already carry `1/σ²` — do not weight them by it again (#686).** `symbol_llrs` divides every
  distance by `noise_var`, so `combine_llrs_map` *is* inverse-noise weighting; the engine re-weighted
  that sum by a `1/mean(|LLR|)` proxy — a second `1/σ²`, costing 0.75 dB on graded HARQ sets.
  `combine_llrs_weighted` is only for the noise-blind ±1.0 trait default. (The 2026-07-29 scan found
  no *new* instance — this is the standing one to guard against.)

### #16 The salient suspect — the obvious culprit gets fixed while the real mechanism survives
- **A hot RX capture level presents as an AFC/frequency bug (#1045).** The visible symptom is a large
  bogus AFC correction, so the reflex is to re-trim the rigs — but a Gate-5 FFT showed the carrier
  already at **+1.2 Hz**; a re-trim would have been pure damage. The real mechanism: a saturated
  `EnergyGate` settling AFC on noise seconds before the frame. `scripts/onair-rx-level-check.sh`.
- **Acquire on the normalised correlation, not the energy score (#689).** SC-FDMA frame loss looked
  like "fade dynamics" and was slated as a channel-estimate fix; **ablating `smooth_ce` entirely**
  left the numbers bit-identical — the sync was the mechanism. `smooth_ce` stays; it wasn't broken.

### #17 Fixing the wrong layer / patching a symptom repeatedly
- **One energy-gate mechanism patched five times (#1020/#1021/#1039/#1040/#1045).** An energy gate
  deciding *where a frame starts* and *triggering the AFC settle* was patched five times before the
  fix moved to the right layer — corroborating the settle with normalised preamble correlation
  (#1049), which removes the settle-on-noise class outright.
- **Code rate is the last lever, not the first.** Higher-rate FEC buys throughput by *spending* SNR
  (`LdpcHighRate` costs +4…+8 dB over `SoftConcatenated` for 2.03× rate — worse than one modulation
  order). Reaching for FEC when the real lever is the constellation is a wrong-layer fix.

---

## How to use this doc

- **Before an audit / review:** start with the archetypes this codebase repeats — **#8
  container-derived length** (cluster 1) and the **#13/#14** proxy-metric / inert-mechanism pair
  around FEC and HARQ have each produced multiple findings. New subsystems most resemble the shape of
  the old ones.
- **When a bug took several wrong hypotheses:** name its archetype, add the instance here, and check
  siblings for the same shape (cluster 1 was one shape in five places).
- **Mining for new archetypes:** CLAUDE.md's "Known sharp edges" and the DSP acquisition playbook are
  the richest corpus; abstract an entry past its domain and, if the abstract shape is not yet in the
  general `defect-archetypes` skill, it is a candidate to contribute upstream (domain-neutralised).
