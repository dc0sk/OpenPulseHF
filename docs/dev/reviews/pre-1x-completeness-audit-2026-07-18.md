# OpenPulseHF — Pre-1.0 Completeness Report

> **Provenance.** Produced 2026-07-18 by a multi-agent audit: 8 parallel finders, one adversarial
> verifier per candidate finding (refute-by-default), synthesis on a separate model. 53 candidates →
> **38 kept, 15 refuted**. Agent output; treat every item as a lead until re-derived.
>
> **Independently hand-verified by the maintainer session** (grep/file read, shown in the PR):
> the missing `estimate_snr_db` in `pilot`/`64qam`/`fsk4`; CSMA reachable only from the KISS TNC;
> `openpulse-keystore` zero dependents; `RendezvousWith` with no CLI/panel emitter; all four
> `llr_reliability` gates failing open (`worst` initialised to `0.0`); `CRATES_TESTED` hardcoded with
> the testmatrix never invoking cargo; 3 zero-assertion tests remaining in `bpsk_hardening.rs`; the
> on-air plan mentioning `hpx_hf` zero times; and no `-D`/MFSK16/JS8 mode in any loopback record.
> The remaining items are **not** independently confirmed.
>
> **Workspace gate at the time of the audit** (`f2a423d`): `cargo fmt` clean;
> `cargo test --workspace --no-default-features` → **254 suites, 2147 passed, 0 failed, 38 ignored**.
> A first attempt at this measurement reported "145 passed" because the run was piped through `tail`
> — which both truncated the output and made `$?` report *tail's* exit status. Recorded here because
> it is the same class as §4.2's fail-open gates: **the harness failed in a flattering direction.**

**Question:** *"Is everything implemented, reviewed, tested via virtual audio loopback, tested via hardware audio loopback, and covered by tests for all requirements and features — is anything left undone or incomplete pre-1.x?"*

---

## 1. The answer

**No — but the shortfall is narrower and more specific than "incomplete."** The implementation is broad, genuinely reviewed, and the core modem/protocol stack is backed by substantive simulator-channel and integration tests; nothing found is a broken shipped feature except two real functional gaps (the pilot/64QAM profiles gate their rate ladders on an SNR estimator that measurably cannot move, and CSMA is unreachable from the daemon/mesh — the surfaces REQ-MAC-02 is about). What is open falls into **five classes**: (1) **coverage-truth defects** — roughly a dozen traceability rows and acceptance cells marked ✅ on the strength of tests that do not exercise the requirement (vacuous, TX-only, misattributed, or a 13-version-stale dirty-tree report), so criteria B2/D3 of your own 1.0 gate are provably unmet today; (2) **unwired shipped code** — the keystore, `DopplerTracker`, half the ACK taxonomy, and the `RendezvousWith` operator surface exist but nothing reaches them; (3) **loopback evidence quality** — virtual and hardware audio loopback validation *happened*, but the two docs recording it contradict each other and almost no run artifacts were retained; (4) **on-air validation has not happened at all** and the test plan that would produce it is stale to the point of being unexecutable (§6.4) and exercising the wrong ladder (§6.2); (5) low-severity doc drift. None of the software-fixable items needs radio hardware; the on-air class does, by definition.

---

## 2. Proven vs. unproven, by feature area and evidence tier

| Feature area | Status | Strongest evidence tier |
|---|---|---|
| Waveform decode, all plugin families (BPSK/QPSK/8PSK/64QAM/MFSK16/JS8/OFDM/SC-FDMA/pilot) | **Proven** | Integration tests through simulator channels (AWGN, Watterson, Gilbert-Elliott) |
| Fade-aware `hpx_hf` ladder (every rung decodes on `moderate_f1`; goodput counterweight) | **Proven** | Simulator gates: `hpx_hf_rungs_survive_fade`, `goodput_gate` |
| FEC (RS/Conv/LDPC), HARQ combining, LLR calibration | **Proven** (but gates fail-open — §4.2) | Integration + calibration tests |
| Handshake/trust/PQ, relay origin auth, ACK auth, SAR poison resistance | **Proven** | Integration tests incl. forgery/tamper negatives |
| B2F/Winlink protocol + driver hardening | **Proven** (software) | Integration tests + mock-CMS round trip; real CMS over RF **unproven** |
| File transfer (FF-16 A–E) | **Proven** (software) | Twin-daemon test — two real daemons bridged |
| JS8 discovery + rendezvous (FF-15 A–G) | **Proven** (software) | Two-runtime GFSK-audio end-to-end test; −18 dB gate |
| Virtual audio loopback (snd-aloop) | **Validated, thin evidence** | Documented runs; no retained gate artifacts |
| Hardware audio loopback (dual-card, dual-clock) | **Validated, thin & contradictory evidence** | 14/14 no-FEC pass narrative (2026-06-19); only retained artifact is a single BPSK250 smoke JSON; the two loopback docs contradict each other on SCFDMA52 (§4.4) |
| Pilot/64QAM profile SNR floors (`hpx_pilot*`, `hpx_wideband_hd` SL15) | **Disproven** | Measured: reported SNR saturates ~7–9 dB; 35 dB floor unreachable (§4.1) |
| CSMA on daemon/mesh/repeater (REQ-MAC-02) | **Unproven / unreachable** | No caller, no config key (§4.1) |
| 1 Hz/s TX drift (REQ-PHY-04), 150 ms acquisition (REQ-PHY-06), PTT ≤50 ms *on a rig* | **Unproven** | No test applies a frequency ramp; no latency measurement; PTT timed against a localhost mock only |
| Windows/WASAPI (REQ-PLAT-03) | **Unproven — never compiled** | No CI job, no test, no artifact |
| Rate ladder climbing/demoting on a **real fading channel**; §97 regulatory validation; station-ID on air | **Unproven — requires radios** | One-direction OTA decode (rpi53→SDR) is the only on-air datum |

---

## 3. Ranked top findings

| # | Finding | Fix sketch | Hardware? |
|---|---|---|---|
| 1 | **Pilot/64QAM plugins have no `estimate_snr_db`** — four `hpx_pilot*` profiles and `hpx_wideband_hd` SL15 gate on the M2M4 fallback, *measured* to saturate at ~7–9 dB against floors of 12/17/23/35 dB; the `snr_scale_boundary` gate is blind to this third state | Add symbol-domain estimators (the post-#934 `additive_snr_db_windowed` pattern), or restate the floors as unused; extend `snr_scale_boundary.rs` to assert every profile-referenced mode implements the estimator (fails today) | No |
| 2 | **REQ-PHY-04 (1 Hz/s drift) is a vacuous gate** — the "Watterson" file never touches Watterson/a modem, 2 of 5 tests pass with a null estimator, no test in the tree applies a frequency *ramp*, and `DopplerTracker`/`freq_acquire` are dead code cited as the implementation | Real test: modulate, apply a 1 Hz/s carrier ramp through `ChannelSimHarness`, assert coded decode; wire or drop the dead modules; fix matrix row 67 | No |
| 3 | **CSMA is never enabled on any production surface except the KISS TNC** — REQ-MAC-02 marked ✅ while daemon/mesh/repeater have no caller and no config key; both docs claim a `stage_emit_output` seam that does not exist (it's 15 per-caller sites; `emit_cw_id` already leaks past it) | Move `csma_check()` into `stage_emit_output` (making the doc true), add a `[modem]` config key wired in the daemon, add a mesh test | No |
| 4 | **`bpsk_hardening.rs` residue: ~10 vacuous tests** (no-op "multipath" tests, zero-assertion recovery/retransmit tests, `is_ok()||is_err()` tautologies) — 1.0 criterion D3 ("a fresh sweep finds no more") cannot be scored green | Delete or implement the ~10; mirror CLAUDE.md's narrowing caveat into matrix CAP-12/13 | No |
| 5 | **The traceability matrix's evidence column is rotten**: 16 rows infer "Pass" from a hardcoded `crates_tested` string array in a v0.3.0, dirty-tree, 13-versions-stale report that never runs the cited cargo tests | Rerun the quick tier on a clean tree at HEAD; derive `crates_tested` from execution or delete it; re-cite rows against real cargo runs with counts | No |
| 6 | **REQ-REG-10 (station ID, §97.119) emitters are covered by nothing** — the daemon's lives inside `server::run` which no test calls; the ARDOP one is unreachable in the suite (no test sets MYID+SENDID on the same server, interval defaults to 0) | Extract server.rs:939–983 into a pure helper (the `discovery_tick` pattern) and assert TX on interval elapse; ARDOP test with MYID→SENDID on one instance | No |
| 7 | **All four `llr_reliability` acceptance gates fail open** — `worst` defaults to 0.0 and both escape paths (`demodulate_soft` Err, underpopulated bins) silently pass; verified by experiment | Two lines per file: assert a bin qualified and a trial succeeded | No |
| 8 | **REQ-SEC-06 half-unimplemented**: key validity windows exist only in the PKI database; the station-side trust store has no expiry field and consults no clock — an expired key verifies forever | Restate matrix row as revocation-only + backlog an expiry field & check | No |
| 9 | **`openpulse-keystore` has zero dependents** — REQ-SEC-CTL-03/04 marked ✅ from the crate's own unit tests; `psk_key_id` has no reader; the PSK is env-var-only | Wire `psk_key_id` into `load_control_psk()` with a failing-without-it test, or downgrade the rows to ⚠ | No |
| 10 | **Discovery audio is teed pre-seam**, against the design doc's explicit post-seam decision; the seam's "covers all paths by construction" comment is now silently false, and the promised `dwell_samples_accumulated` tripwire + twin test were never built | Either move the tee post-seam (DCD gate suppressed) or document the exception at the seam + add the tripwire/twin test | No |
| 11 | **The on-air test plan cannot produce the 1.0 evidence**: §6.2 exercises HPX500 (not `hpx_hf`, the A2 subject) with adaptive ARQ off by default; §6.4 passes `--mode` to a binary that has no such flag and no RF path, with a mode list matching no profile rung | Rewrite §6.2/§6.4 against `SessionProfile::hpx_hf` via the daemon OTA path | No (desk fix; execution needs radios) |
| 12 | **The 1.0 criteria doc contradicts the tree**: C1 keys on a deleted field (`auth_tag`) and the non-goals claim relay auth is blocked when it shipped (#906) two days *before* the doc | Restate C1 against wire v2, mark closed by #906; delete the non-goal | No |
| 13 | **REQ-PLAT-03 (Windows) marked ✅ with zero evidence** — no CI job, no test, never compiled; scored more generously than ARM64, which has a real job and is marked "gap" | Mark gap, or add a windows-latest compile-check job | No |
| 14 | **Loopback docs contradict each other**: virtual-loopback.md still says SCFDMA52 "fails 0/8 on hardware" (SRO); dualcard-loopback.md records it passing on dual-clock six days later; reference-mining C1 is top-ranked on the refuted premise | Mark the 2026-06-13 diagnostic superseded; retain a full-tier dualcard JSON | No |
| 15 | **`RendezvousWith` has no operator client** — the plan's G6 "per-station rendezvous action" silently dropped scope under a "Phases A–G complete" claim; only reachable via hand-crafted control-port JSON | Add `openpulse daemon rendezvous <CALL>` + panel action, or record G6 as deferred to Phase H | No |

Remaining confirmed low-severity items (all desk fixes): REQ-PHY-06 latency unmeasured; CAP-59/CAP-13 stale acceptance cells; REQ-NFR-14 cited tests never construct a PeerCache; misnamed/no-assert tests (`bpsk_iq_output_lengths_match`, `hotplug_modem_engine_with_dynamic_devices`, `rs_vs_conv_ber_random_noise`, two in `qpsk_hardening.rs`); four inert `AckType` variants documented as live protocol; frontmatter validator scope vs. requirement wording; architecture.md/CLAUDE.md publishing the deleted v1 `auth_tag` envelope; no consolidated PHY spec for REQ-REG-02 (E2 points at a doc that excludes the PHY); manual §1.7 "two gates remain" vs. backlog items 8/9/11/15; `SessionProfile::hpx2300()` cited in CLAUDE.md but nonexistent; example config missing 5 sections; book/manual/criteria self-identifying as v0.15.0 inside v0.16.0; 7 stale doc paths (~19 sites).

---

## 4. Full detail by area

All items **[confirmed]** by adversarial verification against source (file reads, greps, and in two cases live experiments) unless noted.

### 4.1 Functional gaps (code, not docs)

- **[confirmed] Pilot/64QAM SNR floors are dead configuration.** `plugins/pilot/src/lib.rs:82` and `plugins/64qam/src/lib.rs:66-123` implement no `estimate_snr_db`; `engine.rs:5062-5077` falls back to M2M4, which is exact only for constant-modulus PSK. **Measured** (throwaway probe, since deleted): PILOT-32APSK500 reports ~6.8 dB at 30 dB true SNR — permanently below even the SL3 floor of 12.0; 64QAM2000-RRC saturates at 9.0 dB vs. its 35.0 floor (`profile.rs:281-289, :768,:776`). `hpx_pilot_fast`'s doc comment claims the engine "measures from the LLRs after the matched filter" — it does not. `snr_scale_boundary.rs:30-36` registers only BPSK+OFDM and cannot see this third state. Mitigation: the #934 evidence-climb still advances these ladders on decode success — degradation, not breakage.
- **[confirmed] CSMA unreachable outside the KISS TNC.** `csma_enabled: false` (engine.rs:451); `enable_csma` callers: KISS bridge/main + tests only. Zero `csma` hits in daemon/mesh/repeater/ardop/config sources. Matrix row 102 marks REQ-MAC-02 ✅; CAP-31 (matrix:223) and architecture.md:223 assert a `stage_emit_output` seam that does not exist — `csma_check()` is duplicated at 15 call sites and `emit_cw_id` (engine.rs:526-533) bypasses all of them. Mitigating: the JS8 beacon (the one automatic outward TX) *is* gated on `is_channel_busy()` (server.rs:1312, tested).
- **[confirmed] REQ-SEC-06 validity windows: station side has none.** `trust_store_file.rs:13-19` is `{station_id, key_id, trust}`; no key-lifetime concept in core; `valid_from/valid_until` live only in `pki-tooling/migrations/0002` and are not even fetched by the CLI. Handshake freshness (timestamp replay check) exists but is per-message, not key lifetime. Matrix:116 says ✅ unqualified.
- **[confirmed] `openpulse-keystore` orphan.** Zero dependents workspace-wide; `psk_key_id` (config lib.rs:185) has zero readers; daemon PSK is `OPENPULSE_CONTROL_PSK` env-var only (`server.rs:1613-1631`, whose own comment calls keystore loading "the production follow-up"). Matrix:165-166 ✅ contradicts its own CAP-68 row two lines down.
- **[confirmed] Four `AckType` variants (Break/Req/Qrt/Abort) decode-only.** No production constructor; their `RateEvent`s have zero non-test consumers (rate.rs:346-349 vs. rate_policy.rs). `HpxEvent::RemoteTeardown` is likewise never produced from a received frame — no in-band peer teardown path exists anywhere. Safe (inert), but protocol-wire-spec.md:221-224 documents them as live with no "reserved" marker.
- **[confirmed] `RendezvousWith` no CLI/panel emitter** — the only `ControlCommand` variant with none on either surface; plan doc G6 promised the per-station action, then "Phases A–G complete" was declared over its absence. Daemon side is implemented and unit-tested; reachable via raw control-port JSON only.
- **[confirmed] Dead DSP modules cited as implementation.** `DopplerTracker`/`AdaptiveAfcLoopBandwidth` and `freq_acquire`: only consumer is the `pub mod` line; CAP-21's implementation column cites both.

### 4.2 Test-integrity / vacuous & fail-open gates

- **[confirmed] `bpsk_hardening.rs`:** 10–11 of 20 tests vacuous — three "multipath" tests apply no channel (:145,:154,:163; comments describe nonexistent attenuation code); three with zero assertions (:174,:193,:205); two `is_ok()||is_err()` tautologies (:242,:253); `bpsk_recovery_exhaustion_transitions_to_failed` (:266) asserts the opposite of its name; module doc advertises multipath+recovery coverage that does not exist. Capability *is* proven elsewhere (`channel_loopback.rs` Watterson/GE tests); the defect is misattribution + D3.
- **[confirmed] `qpsk_hardening.rs`:** zero `receive` calls file-wide; two tests with no `assert!` at all (:57,:65). CLAUDE.md:482 is honest about it; matrix CAP-13:205 still cites it as the acceptance command.
- **[confirmed] Four `llr_reliability` gates fail open** (64qam:82, ofdm:87, pilot:80, scfdma:84): verified by experiment — with bins unable to qualify, the 64QAM gate still passes. Live and meaningful *today* (`ratio > 0.0` also passes), but a `demodulate_soft` regression to `Err` — exactly what `-D` modes deliberately do — silently converts all four to no-ops, and CLAUDE.md says no other metric can see LLR miscalibration.
- **[confirmed] REQ-PHY-04/PHY-06:** see §3 items 2; additionally no test anywhere measures acquisition latency (zero `Instant::now` in modem/dsp tests); pipeline_timing_integration asserts its own injected constants.
- **[confirmed] Misnamed/no-assert stragglers:** `bpsk_iq_output_lengths_match` (iq_output.rs:15 — no assert; sibling tests do check lengths), `hotplug_modem_engine_with_dynamic_devices` (hotplug_integration.rs:404 — constructor-only), `rs_vs_conv_ber_random_noise` (fec_comparison.rs:118 — unmarked printer, the last one in a file whose sibling was just fixed for exactly this; counts toward CAP-27's "6 passed").
- **[confirmed] REQ-REG-10:** timer/renderer/counter unit tests are real; the arm→due→key→transmit wiring on both production paths (server.rs:939-983; ardop bridge.rs:419 behind gates no test satisfies) is covered by nothing that would fail if it broke. Matrix:141 says covered.

### 4.3 Traceability & 1.0-gate accuracy

- **[confirmed] 16 matrix rows** infer Pass from membership in a **hardcoded** `CRATES_TESTED` array (testmatrix main.rs:23) in a report stamped v0.3.0, `git_dirty: true`, 2026-07-01, commit mismatching the matrix's own caveat; the testmatrix never invokes cargo, so even a fresh run would not substantiate the cited test files. Matrix line 383's "refreshed this session" claim names a third, different commit.
- **[confirmed] release-1.0-criteria.md C1** keys on the deleted `auth_tag`; the relay-auth non-goal is false (shipped #906, default-on, gated, two days before the doc was written). E2's cited artifact declares the PHY out of scope, so the criterion can't be scored by its own method (REQ-REG-02 remains an honestly-marked gap).
- **[confirmed] Stale acceptance cells:** CAP-59 → `noop.rs` bool-flip (CLAUDE.md:513 already corrected; and note even the rigctld test times a localhost mock, not "drop after last transmitted sample"); CAP-12/13 → TX-only hardening files without CLAUDE.md's caveats; CAP-48 → two test files that never construct a `PeerCache`; CAP-62 → a loopback test with zero CPAL/Windows content for REQ-PLAT-03, scored *more* generously than ARM64 which has a real cross-check job and is marked "gap".
- **[confirmed] REQ-DOC-02 scope mismatch:** validator covers 20 of 183 docs (`files=(docs/*.md)`, non-recursive) — deliberate and ledger-recorded, but requirements.md:343 and criteria:63-64 state it tree-wide without the caveat; a naive recursive fix would fail ~150 files (docs/dev uses a different schema), so this is a schema/wording decision.

### 4.4 Loopback & on-air evidence

- **[confirmed] virtual-loopback.md:66 vs dualcard-loopback.md:99-110 directly contradict** on SCFDMA52 dual-clock status; the stale paragraph was touched *by the commit that landed the contradicting results*; reference-mining-plan C1 is prioritized on the refuted premise; **neither state has a retained artifact** — the only dualcard JSON on disk is a single-case BPSK250 smoke.
- **[confirmed] on-air testplan:** `last_updated: 2026-05-16`, pre-fade-arc; zero `hpx_hf` mentions; §6.2's pass criterion (RateChange events) unreachable from §6.2's own command (adaptive ARQ default-off, no `--profile` flag on openpulse-tnc); §6.4 passes `--mode` to `openpulse-gateway`, which has no such flag, no RF path, and no TNC log, with a mode list matching no rung of `hpx_wideband_hd`.

### 4.5 Docs drift (low)

- **[confirmed]** architecture.md:343-348 **and CLAUDE.md:264** publish the deleted v1 `auth_tag` envelope (wrong minimum-frame size both directions); the normative wire spec is correct.
- **[confirmed]** manual §1.7 "two gates remain, both need radios" vs. backlog items 8, 9, 11 (WS auth + keystore PSK — pure software; the daemon correctly refuses to spawn the WS port under auth, which the manual never mentions at the line telling operators to connect to it), 15 (#917 tail).
- **[confirmed]** `SessionProfile::hpx2300()` in CLAUDE.md:233 doesn't exist (renamed `hpx_wideband`); stale comment at profile.rs:175.
- **[confirmed]** Example config missing `[logbook] [compression] [file_transfer] [discovery] [monitor]` (fail-safe — omission keeps beaconing off; `config init` emits all 20; matrix:270 vs :250 self-contradict on which is the source of truth).
- **[confirmed]** Book/manual/criteria self-identify as v0.15.0 inside v0.16.0 (one factually false sentence: book 3.1 "every crate is 0.15.0"); `check-version-bump-docs.sh` cannot catch the class.
- **[confirmed]** Seven stale doc paths (`docs/architecture.md`, `docs/vara-research.md`, `docs/high-performance-mode.md` + four more research files), ~19 sites, incl. matrix:159 citing a nonexistent path for a ✅ REQ-DOC-03.

---

## 5. What would have to be true for 100% certainty

**Fixable at the desk (no hardware):**
- The ~25 items above closed: real gates for PHY-04 (frequency *ramp*) and PHY-06 (measured latency); the vacuous/fail-open tests deleted or made falsifiable; the matrix re-cited against fresh real runs with pass/fail counts on a clean tree; `snr_scale_boundary` extended to fail on estimator-less profile modes; CSMA at the actual seam with a config key; station-ID emitters extracted and tested; the criteria doc, both loopback docs, and the on-air plan brought to the current tree. A re-run of the D3 sweep after that, finding nothing.
- Retained artifacts (JSON, dated, clean-tree) for one full virtual-loopback ladder run and one full dual-card hardware-loopback ladder run, so both loopback tiers rest on evidence rather than narrative.

**Cannot be established without radio hardware / on-air operation** (this is the irreducible remainder, and it matches the 1.0 criteria's own A-group):
- The `hpx_hf` ladder observed climbing **and** demoting on a real ionospheric fading channel (A2) — sim Watterson is a model, not the sky.
- §97 regulatory validation: station-ID timing audited on air, occupied bandwidth measured on a real transmitter, the compliance report (Phase 5.5-reg).
- PTT drop ≤50 ms measured against a real rig's RF envelope, and TX-drift/acquisition behavior against real oscillators and two independent station clocks (the dual-clock effects that already broke modes no simulator predicted).
- Winlink CMS over an actual RF path; FF-15 Phase H (JS8 rendezvous on air); FF-16 Phase F (file transfer on air).
- Windows/WASAPI: not radio-gated, but requires a Windows build environment that has never been pointed at this tree; until then the honest status is "never compiled," not ✅.

**Bottom line:** the software is close to done and mostly honestly tested; the *bookkeeping that proves it* is the weakest layer, and the on-air campaign — for which the current plan is not executable as written — is the only genuinely hardware-blocked work standing before 1.0.