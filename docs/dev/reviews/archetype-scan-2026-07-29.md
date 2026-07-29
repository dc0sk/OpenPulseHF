---
project: openpulsehf
doc: docs/dev/reviews/archetype-scan-2026-07-29.md
status: review
last_updated: 2026-07-29
---

# Defect-archetype scan — 2026-07-29

Sixteen defect archetypes (two families) were hunted across the workspace; every finding below
survived an adversarial verification pass (re-derivation from source, and in most cases an
independent ablation or reproduction run) before being included. Findings are grouped by the
**shape** they take, not by the file they happen to live in — the point of an archetype scan is to
show which shapes this codebase is prone to, so future audits know where to look first.

Totals: **18 unique findings** (one duplicate pair merged) — **3 high, 7 medium, 8 low**. No
critical-severity finding survived verification (the one candidate, `BURST_MAX_SAMPLES`, was
downgraded critical→high on rereview: it is real and reachable on the default config, but confined
to 2 of 14 `hpx_hf` rungs and fails closed rather than silently).

---

## Archetype cluster 1 — container-derived length / unreachable guard (Family A)

This is the dominant shape in the scan: **four independent findings**, all in
`crates/openpulse-modem/src/engine.rs`, where a completeness/recovery precondition is sized from a
buffer or reserve *derived from the mode's maximum geometry* rather than from the frame that
actually arrived. All four hit the slow end of the `hpx_hf` ladder (BPSK31/BPSK63) or the LDPC/
RsInterleaved FEC families, and all four are invisible to their nominal regression gate because
that gate's fixture happens to sit on the side of the boundary where the defect can't fire.

1. **[HIGH, confirmed] `BURST_MAX_SAMPLES` (30 s) force-flushes mid-frame on `hpx_hf` SL2/SL3.**
   `crates/openpulse-modem/src/engine.rs:514` (const), `:1478` (fire site). Measured real TX sample
   counts: BPSK31+Rs = 66.6 s, BPSK63+Rs = 33.3 s — both exceed the 30 s cap, which force-splits the
   carrier into two preamble-less bursts on every normal frame. Reachable on the daemon's default
   config (`profile: "hpx_hf"`, `initial_level` = SL2 = BPSK31). The four tests that reach
   `accumulate_capture` all use short modes (OFDM52-16QAM, QPSK500, BPSK250) — none can fail given
   this defect. `profile.rs:410` already states the BPSK31 frame length in prose; the two constants
   were simply never compared.

2. **[HIGH, confirmed] The scanning FEC receive's LDPC decode loop consumes the whole
   container-derived window and hard-fails on trailing-noise codewords.**
   `crates/openpulse-modem/src/engine.rs:5528`/`:5537`. `chunks_exact(n_bits)` decodes every
   whole codeword in the over-reserved slice (`fec_slice_factor`: Ldpc ×3, LdpcHighRate ×2), so a
   real capture (which always outlasts the frame — DCD hold alone adds ~100 ms) produces a full
   trailing-noise codeword that fails BP and aborts the whole frame with `?`. Reproduced by
   ablation: varying only trailing padding on an otherwise-identical QPSK250 frame flips OK→`LDPC
   did not converge` at a padding threshold that matches the codeword-size arithmetic exactly;
   swapping only the `?` for break-on-first-failure fixed all 24 failing cases. The shipped
   long-capture gate (`tests/fec_scan_long_capture.rs`) covers only Rs/RsStrong — no LDPC case
   exists, so it structurally cannot see this. Reachable via `openpulse receive --fec ldpc` and the
   daemon's production OTA path; affects `hpx_hf` SL12–14 (LdpcHighRate) and `hpx_modcod` SL2/SL4
   (Ldpc). The failure message reads as a channel problem, not a length problem — costly for on-air
   triage.

3. **[HIGH, confirmed] `RsInterleaved` on the scanning receive fails at *every* capture length,
   including the length its own regression gate uses.** `crates/openpulse-modem/src/engine.rs:3054`.
   `Interleaver::deinterleave` builds its permutation from `data.len()`, i.e. the capture window —
   so a window-length buffer produces a *different* permutation than the transmitter used and the
   frame bytes are scattered; `decode_prefix` (which rescues Rs/RsStrong from the same class of
   bug) cannot rescue this because trimming to a candidate length before deinterleaving does not
   recover the original bytes (checked directly: only padding=0 round-trips). Reproduced through
   the production entry (`receive_with_fec_mode_timeout`): fails at every non-trivial capture
   length (2k/2k, 8k/40k, 40k/120k), while an `Rs` control on the identical route passes every time
   — the "fails at all inputs" signature the project's own culture treats as a bug tell. The only
   gate (`tests/fec_timeout_receive.rs::bpsk_rs_interleaved_timeout`) uses `route()`, which hands
   the RX loopback a buffer that *is* the frame — the exact fixture gap `route_embedded` exists to
   close, and it was never applied here. User-reachable via `--fec rs-interleaved` (documented in
   `docs/openpulse-book.md`, offered by three loopback runner scripts) though it is in no shipped
   `SessionProfile` rung, so the ladder itself is unaffected.

4. **[MEDIUM, confirmed] The `#1021` settle-recovery precondition is sized from the FEC reserve,
   not the real frame — unreachable for BPSK31 in every configured on-air harness.**
   `crates/openpulse-modem/src/engine.rs:2690`. `window_complete` requires
   `accumulated.len() >= onset + max_frame_samples`, where `max_frame_samples` is rebound to the
   `frame_plan` reserve (2× the real coded frame for Rs). BPSK31 needs 149.2 s of post-onset audio
   to satisfy this; the dual-card harness's listen window is 109.6 s — unreachable. BPSK63 is
   unreachable on that harness but reachable on the on-air runner's fixed 130 s window — the
   exposure is harness-dependent, which is itself telling. The one regression gate for this
   recovery path (`tests/coded_noise_settle_recovery.rs`) pins BPSK250 exclusively, the one mode
   where the precondition is satisfiable, so a fix here has no test that would validate it.

**Why this cluster matters**: three of the four findings sit on the exact two rungs
(`hpx_hf` SL2/SL3, i.e. BPSK31/BPSK63) that CLAUDE.md's own history names as the ladder's most
fragile — the entry rung a session must confirm before it can climb. A fourth (LDPC) and fifth
(RsInterleaved) hit different FEC families with the identical *shape*: a scanning-receive window
sized from the container rather than the payload. This is a single systemic pattern, not five
unrelated bugs, and the fix shape is already proven in the repo (`FecCodec::decode_prefix` for
Rs/RsStrong) — it simply was not extended to the other three consumers.

---

## Archetype cluster 2 — proxy metric that doesn't track the objective (Family B)

Three findings where a green gate measures something *correlated* with the real property under
uncoded/noiseless conditions where the correlation happens to be perfect, then silently stops
tracking it under the real (coded, noisy) conditions the objective actually cares about.

5. **[MEDIUM, confirmed] Nine of twelve shipped `SessionProfile`s assign `FecMode::None` to every
   rung, while their own docs and the hardware-validation record claim soft-concatenated FEC.**
   `crates/openpulse-core/src/profile.rs:803` (`hpx_wideband_hd`) and eight siblings. Ablation
   confirms the mechanism: at each rung's own configured `snr_floor` with `FecMode::None`,
   SCFDMA26-8/16/32-{PSK,QAM} (SL9–11) decode 0–1/3; with `SoftConcatenated` substituted, the same
   floors decode 3/3. The linksim (`apps/openpulse-linksim/src/lib.rs:235`) already has a documented
   fallback for exactly this condition ("rungs the profile leaves unprotected") — meaning every
   sweep that produced the published SL9–11 floors was run through a FEC the shipped
   `ota_rate.rs → profile.fec_for()` path never applies. Scope corrected down from the original
   "9 of 12 profiles are broken" framing: `hpx500`'s own doc self-consistently derives its floors
   uncoded, and the defective profile (`hpx_wideband_hd`) is off the 1.0 on-air critical path
   (VHF/UHF, not `hpx_hf`).

6. **[MEDIUM, confirmed] The regression gate meant to catch a missing-FEC assignment measures
   decode on a noiseless channel, where uncoded decode always succeeds.**
   `crates/openpulse-modem/tests/channel_loopback.rs:291`
   (`every_profile_rung_decodes_clean_with_its_fec`). Its own docstring names the exact bug class it
   was written to catch (the `hpx_ofdm_hf` missing-FEC incident). Direct ablation: reintroducing
   that named bug verbatim (`FecMode::None` on the same rungs) — nine modes including every OFDM/
   SC-FDMA variant — leaves the gate green (`uncoded == true` for all nine). The gate retains real
   teeth for the *wrong-FEC* half of its bug class but is structurally blind to the *missing-FEC*
   half, which is exactly finding 5 above.

7. **[LOW, confirmed] `afc_doppler_watterson.rs` is cited as validating "AFC under Watterson
   fading" but contains no `WattersonChannel`, no `ModemEngine`, and no decode — and its headline
   `<±5 Hz` assertion is unreachable.** `crates/openpulse-modem/tests/afc_doppler_watterson.rs`.
   The tested component (`DopplerTracker`/`AdaptiveAfcLoopBandwidth`) has zero production callers.
   Sabotage probe: replacing the `<±5 Hz` threshold with an impossible one (`< -1.0`) still passes —
   the loop guard (`doppler_estimates.len() > 50`) can never be satisfied with the 64 symbols the
   test feeds it, so the real assertion is `!doppler_estimates.is_empty()`. This is cited as done
   with three `[x]` boxes in `docs/dev/vara-parity-execution-board.md`, none of which were actually
   measured (no channel, no SNR, no 500-symbol window, no 100-frame stability run). Low severity:
   the board entry is already `status: resolved`/closed and the real fade-AFC gates
   (`bpsk_snr_tracks_a_fade`, `hpx_hf_rungs_survive_fade`) are independent and unaffected — the harm
   is confined to evidence integrity in a closed record plus one misleadingly-named vacuous test.

---

## Archetype cluster 3 — seam gap (Family A)

8. **[MEDIUM, confirmed] `transmit_iq` bypasses the wire-whitening seam added for `#1021`; every
   receive path un-whitens unconditionally.** `crates/openpulse-modem/src/engine.rs:2278`
   (bypass), claim asserted false at `:5116-5117`. Of 15 modulate call sites, 13 go through
   `stage_modulate_payload`, which whitens at `:5120` under the comment "this is the single TX seam,
   so every caller is covered by construction" — untrue: `transmit_iq` reaches
   `plugin.modulate_iq()` via `route_wire_stage`, which applies no transform. RX un-whitens
   unconditionally on both the hard path (`:5344`) and every `demodulate_soft` site. An IQ-
   transmitted frame is therefore XORed with a keystream it never received and decodes to
   `invalid magic` on any receiver of the same build — the identical failure signature that made
   `#1021` undiagnosable. `tests/iq_output.rs` has no decode round-trip at all (asserts only sample
   counts, Q-RMS, attenuation ratio), so nothing can catch it. Currently latent: `transmit_iq` has
   no production callers (grep confirms only the engine and its own tests), which is why this is
   medium and not high.

9. **[LOW, confirmed] The multi-mode receive monitor (REQ-RX-01) is wired into only one arm of the
   `rx_ticker` dispatch and goes silent for the entire lifetime of an OTA session.**
   `crates/openpulse-daemon/src/server.rs:874` (emit site), `:823` (bypassing arm),
   `engine.rs:1143` (`ota_active()`). The guarded OTA arm never touches `runtime_state.monitor`; the
   only `MonitorFrame` emit is in the fall-through arm. Because `start_ota_session` runs once at
   daemon startup (`server.rs:230`) whenever `ota_enabled` and is never cleared
   (`end_ota_session` has zero callers), the monitor is dark for the whole process lifetime under
   that config — exactly the on-air scenario configuration. The acceptance test for REQ-RX-01
   (`cargo test -p openpulse-daemon monitor::`) calls `MonitorRuntime::decode_all` directly and
   never reaches `server::run`'s dispatch, so it cannot see the bypass. Same shape CLAUDE.md already
   records for the receiver notch. Severity low because both features are opt-in and default off;
   only their intersection is affected, and the consequence is a passive observability gap, not a
   correctness or safety issue.

---

## Archetype cluster 4 — blind sibling path (Family A)

10. **[LOW, confirmed] RX SNR is recorded into `rate_policy` only on the soft-demod branch; every
    hard-FEC rung (including `hpx_hf`'s differential SL6 and the whole Rs/RsStrong lower half of
    the ladder) records nothing.** `crates/openpulse-modem/src/engine.rs:3022` (gate),
    `:2872` (sibling soft/hard split). The gate is keyed on the wrong predicate — soft-demod
    *capability* / FEC *family* — rather than SNR-estimator availability; `QPSK250-D`'s own plugin
    implements `estimate_snr_db` but is skipped because `supports_soft_demod` is false for
    differential modes, and profile SL1–SL6 are all `FecMode::Rs` regardless of plugin. The only
    test covering this (`engine_events.rs::receive_populates_last_rx_snr_db`) uses a soft-capable
    mode and says so in its own assertion message. Real but non-safety consumers affected: the QSY
    frequency scan (scores every candidate on a constant via `.unwrap_or(0.0)`) and the ADIF
    logbook's `rx_snr` field. The rate ladder itself is unaffected — it reads a value computed
    unconditionally in a separate call path.

11. **[LOW, confirmed] Non-OTA burst decode failures are silently swallowed while the OTA sibling
    arm in the same match block logs every one.** `crates/openpulse-daemon/src/server.rs:886`
    (`.unwrap_or_default()`, no log) vs `:861-864`/`:890-891` (both log at debug). Traced the
    downstream callee: the swallowed path still emits partial diagnostics one layer down
    (`receive_from_samples`'s own `info!`/`debug!` lines), so the finder's "yields an empty log" was
    corrected — only the terminal error reason (e.g. `PluginNotFound`, magic/CRC failure) is lost,
    and FEC-length rejection cannot occur on this arm at all (it hardcodes `FecMode::None`). Fix is
    a one-line mirror of the two logged siblings.

---

## Archetype cluster 5 — inert mechanism (Family B)

12. **[MEDIUM, confirmed] `HarqPolicy` (retry-FEC escalation + SNR-scaled ACK timeout, including
    the high-rate-LDPC tier) has zero production callers — it runs only inside its own test
    suite.** `crates/openpulse-modem/src/harq.rs:64`, reached only via `engine.rs:1967`/`:1981`.
    Confirmed by tracing the real ARQ path: `transmit_arq_ota_within` takes FEC from
    `SessionProfile::fec_for` + `free_rs_strengthening`, never consulting `HarqPolicy`; the ACK-
    timeout curve is doubly inert — the production timeout (`ota_ack_timeout_ms`) doesn't even
    share a numeric range with `HarqPolicy`'s. `docs/dev/vara-parity-execution-board.md` marks the
    corresponding roadmap item "FUNCTIONALLY COMPLETE" with passing `[x]` boxes, while its own
    "Current State" bullet on the next line still reads "Retransmit on NACK without rate change" —
    both are true only because the selector never runs. One cited gate
    (`harq_rate_selection_watterson.rs`) computes goodput from pure arithmetic over the decision
    struct and explicitly discards the decode result (`let _ = ...`), so it would pass with zero
    frames actually decoded. Medium, not higher: no runtime defect, the shipped MODCOD-table ladder
    is the deliberate real design — the harm is a completion claim in two traceability documents
    resting on code that cannot execute.

---

## Archetype cluster 6 — artificially-easy fixture / vacuous assertion (Family A)

13. **[MEDIUM, confirmed] The Watterson frame-lock test discards the one field
    (`offset`) that would reveal mislocation, so 35–45% of "locked" frames are actually locked
    onto the delayed multipath ray and the test cannot see it.**
    `crates/openpulse-modem/tests/waveform_lock_watterson.rs:32` (finder's cited line was refuted;
    real mechanism found by instrumentation). The finder's own claim (`search_bound = guard + 12`
    caps the fixture) was disproven by ablation — widening the bound changed nothing, because the
    fixture's own length already clamps the search. The real defect: lines 37-41 read only
    `res.rho` and never check `res.offset` against the known-correct value. Instrumented replica of
    the harness shows offsets of 16 (correct) *and* 20/24 (locked onto the delayed ray, per the
    per-profile Doppler delay) both counted as "locked" — 7–9 of 20 frames per case. This is the
    exact `#688` family CLAUDE.md already documents ("sync must lock ahead of the peak, never on
    it"), reproduced inside a test that is supposed to guard against it, and cited in
    `docs/dev/vara-parity-execution-board.md` as "Frame lock reliability ≥99%". Medium: test-
    evidence quality, not shipped-code behavior; production acquisition has its own separate gates.

14. **[LOW, confirmed] Two channel-simulation seams (`route_with_capture_agc`,
    `route_embedded_with_cfo`) that exist specifically to reproduce a hardware-diagnosed defect
    (#1009/#1010's capture-AGC finding) have zero callers, and the AGC one starts the AGC cold,
    producing an ×80 startup gain spike no real radio ever produces.**
    `crates/openpulse-modem/src/channel_sim.rs:268`. Confirmed by hand-simulating the AGC's own
    `apply()` loop: cold start on a 0.1-amplitude tone peaks at ×80.5 the input in the first ~80
    samples (the exact preamble region acquisition operates on); priming with 1 s of idle noise
    first drops the same transient to ×10.1 — same channel, ×8 different transient, and the cold
    one is in the wrong causal direction (ramping up from unity rather than settling down from an
    idle-set level). Neither of the AGC module's own two unit tests exercises the transient — both
    measure well past it by construction — so the ×80 spike passes every existing check. Low, not
    medium: the function is uncalled, so nothing is currently mis-measured; this is a prospective
    trap for whoever wires the seam up next, not a live defect.

---

## Archetype cluster 7 — generalized past the validity boundary (Family B)

15. **[LOW, uncertain] `fec_slice_factor`'s per-FEC ratio table is measured on one plugin
    (BPSK250) for 6 of 10 `FecMode` variants, and its boundary justification ("every plugin's
    frame geometry already reserves a full 255-byte RS block") is false for MFSK16, whose geometry
    already *is* exactly one block with zero margin, permanently doubling its scanning-receive
    reserve.** `crates/openpulse-modem/src/engine.rs:235` (doc `:219-234`); gate
    `tests/fec_slice_expansion.rs:25` hardcodes `MODE = "BPSK250"` and never exercises
    `Ldpc`/`LdpcHighRate`/`ShortRs`/`Turbo` despite the doc quoting two of them as "measured". Two
    of the finder's three specific harm claims were refuted on inspection: FSK4 and JS8 never
    route through this table at all (dedicated fixed-window / raw-audio paths), and the claimed
    "`#1021` unreachable-recovery reintroduction" doesn't apply to MFSK16 (it already exceeds the
    long-frame threshold at factor 1). What survives is narrower: MFSK16's reserve is a real,
    permanent 2× over-allocation invisible to the BPSK250-only gate, with no demonstrated decode
    failure — hence low, not the original high.

16. **[LOW, confirmed] The one regression test guarding this whole sweep's own reproduction
    constants documents a mechanism (`x3` FEC-reserve factor) that a same-week commit already
    changed to `x2`, and its own present-tense rationale no longer matches the code it cites.**
    `crates/openpulse-modem/tests/coded_noise_settle_recovery.rs:15-19,51-57`. Arithmetic check
    against the plugin and the current `fec_slice_factor` confirms the doc's `223,872`-sample
    figure is stale (`149,248` is correct at the current factor of 2); `git show --stat` on the
    factor-change commit confirms it never touched this test file. The test still passes and the
    classification it depends on is independently guarded elsewhere, so the harm is confined to a
    misleading rationale in a load-bearing on-air regression test (plus a self-contradicting doc
    comment four lines away in `engine.rs` that states "3×" nine lines above a table whose measured
    value is 2×) — not a live defect.

17. **[LOW, confirmed] The one unit test standing behind the `#1021` wire-whitening fix measures
    bit-transition density in the wrong bit order, counts the wrong run type, and checks the wrong
    keystream offset — all three regime mismatches simultaneously, though the underlying property
    happens to still hold in the real regime.** `crates/openpulse-core/src/scramble.rs:110-137`
    (`an_all_zero_block_becomes_transition_rich`). Verified: the test unpacks MSB-first while the
    real wire packs LSB-first (the same self-consistency trap the module's own doc already warns
    about for a sibling function); it counts identical-bit runs where the real NRZI dead-carrier
    condition is specifically a run of *zero* bits; and it checks keystream offset 0 rather than the
    real RS-padding offset (~28). Decisive probe: a constructed real-regime dead-carrier sequence
    (repeated 17-symbol zero runs, correct bit order) passes both of the test's assertions —
    the defect this test exists to prevent can recur and this, the only gate, would not catch it.
    Independently reproduced the LFSR in Python to confirm the property does genuinely hold at the
    real offset/order (max identical run 9, matching the test's own numbers) — so this is
    presently a blind spot, not a live regression; the miss window (17–30 dead bits) is far short of
    the 6.2 s that broke the link originally.

---

## Archetypes that came back empty

Of the sixteen archetypes hunted, the following produced **no surviving finding** in this scan and
that absence is itself a result, not a gap in coverage:

- **Environment-as-SUT** (mistaking rig/environment state for the system under test) — the
  project's own memory log shows this shape was real and costly in past sessions (#1008–#1010,
  #998), but no new instance was found this pass.
- **Double-applied correction** — no case found where the same statistical correction (e.g. a
  1/σ² weighting) is applied twice in a pipeline.
- **Salient suspect** (the obvious culprit gets fixed while the real mechanism survives) — no new
  instance found; the historical cases in CLAUDE.md's "known sharp edges" were already resolved.
- **Flat response** (a metric that reads a constant across a wide input range — the #934 shape) —
  no *new* instance found; the SNR-estimator family of this bug is already closed per memory.
- **Symmetric fixture** (a test whose symmetry hides a directional bug — the #688 sync-lock-ahead
  shape) — the closest hit (finding 13) is a discarded-field vacuous assertion, not a symmetric
  fixture per se, so it's filed under cluster 6 rather than here.
- **Wrong layer** (fixing a defect one architectural layer away from where it lives) — the
  `transmit_iq` finding was initially filed under this label but is better characterized as a plain
  seam gap (cluster 3); no clean wrong-layer instance survived independently.

## Overall assessment

The codebase is not broadly unsound — every finding above is a real, verifiable gap, but none
represents corruption, a safety violation, or a transmit-key hazard, and several of the highest-
severity candidates from the initial pass were downgraded on adversarial rereview (critical→high,
high→medium, high→low) once their blast radius was actually traced. The genuinely repeated shape
worth acting on first is **cluster 1**: four separate places where a receive-side completeness or
recovery precondition is sized from the *container* (a FEC reserve, a fixed burst cap) rather than
the *payload*, all concentrated on the slow/coded end of the ladder that on-air validation cares
about most, and all invisible to their nominal gates for the same reason — the gate's own fixture
never grows a buffer past the point where the defect fires. The proven fix shape
(`FecCodec::decode_prefix`) already exists in the repo for two of five affected consumers; extending
it to the LDPC and RsInterleaved arms, and re-deriving `BURST_MAX_SAMPLES` and the settle-recovery
window against real per-mode frame lengths, would close the whole cluster at once.
