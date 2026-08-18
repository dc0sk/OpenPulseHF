---
project: openpulsehf
doc: docs/dev/design/streaming-frequency-acquisition.md
status: resolved
last_updated: 2026-08-18
---

# Frequency acquisition on the daemon's streaming receive path (#1118)

Design for giving the shipping daemon the one acquisition capability it lacks. Reviewed
adversarially before implementation; the eight changes that review required are folded in, and the
two premises it falsified are marked as such rather than quietly corrected.

## The defect, and why it is a defect

**`REQ-PHY-03`: "The demodulator must track station-to-station frequency offsets of up to ±50 Hz
without operator intervention."** The shipping daemon fails at exactly 50 Hz.

Measured by `daemon_vs_cli_on_real_captures::m2_carrier_offset_sweep_cli_vs_daemon` — one real
engine-transmitted BPSK250+Rs frame per row, shifted by the shipped `CfoChannel`, embedded in real
recorded idle at the measured 4032-sample lead-in so the receiver must locate *and* acquire, both
arms fed bit-identical audio:

| offset | CLI | daemon | daemon + perfect frequency estimate | CLI's settled correction |
|---|---|---|---|---|
| 0 Hz | decoded | decoded | decoded | +0.0 |
| 20 Hz | decoded | decoded | decoded | +19.8 |
| **50 Hz** | decoded | **—** | **decoded** | +49.9 |
| 100 Hz | decoded | — | decoded | +100.2 |
| 200 Hz | decoded | — | decoded | +200.1 |
| 400 Hz | decoded | — | decoded | +399.8 |

The third column is the control: hand the daemon the correct centre frequency and **nothing else** —
no energy gate, no veto, no condemnation recovery — and it decodes every row. Verified clean in
review: on this path `center_frequency` and `afc_correction_hz` are consumed everywhere as their
sum, the BPSK plugin mixes at `center_frequency` alone, and the DCD, noise-floor tracker, AGC and
onset-scan geometry are frequency-agnostic. The one frequency-sensitive front-end component, the
notch, was **measured** not to change the result under the shipping daemon's own notch config.

So the missing capability is **frequency acquisition specifically**, not "the acquisition chain" —
consistent with the earlier ablation that falsified "the chain earns its keep" on the corpus.

**Scope of the number.** `≲20–50 Hz` is a BPSK250 figure at n = 1 per cell. BPSK31 — `hpx_hf`'s entry
rung — has a ±7.8 Hz estimator range and is plausibly out of spec even at the corpus's +12 Hz. Do not
generalise the range across modes. `CfoChannel` models CFO only: sampling-clock offset (#391/#397)
and REQ-PHY-04's 1 Hz/s drift are not exercised here.

**Corrected on review:** an earlier draft justified this with "~400 Hz inter-rig offsets". That
figure traces to the two-station OTA notes whose CFO readings are marked unreliable in the same
paragraph — the spectral peak-picker was measuring dev-host birdies. The trusted measurement is
**−64 Hz**, which already exceeds REQ-PHY-03. Corrected in the harness header and in
`openpulse-book.md`.

## Requirements

| ID | Statement | Status |
|---|---|---|
| `REQ-PHY-03` | The demodulator must track station-to-station frequency offsets of up to ±50 Hz without operator intervention. | ratified (existing) |

No new requirement: this is a shipping surface failing one that already exists.

## Design — two-phase acquisition per burst

1. **Phase 1 is today's path, unchanged.** Scan onsets across `burst_onset_scan_bounds`, attempt each
   candidate at the current `afc_correction_hz`. Every frame the daemon decodes today decodes here,
   bit-identically.
2. **Phase 2 runs only when phase 1 failed every candidate at every onset.** For each scanned onset:
   `afc_mini_settle` over the preamble-sized window, apply the two existing guards (converged:
   `last_delta < 5` and `|fine − anchor| ≤ 20`; plausible: `|fine| ≤ AFC_MAX_CORRECTION_HZ`), then
   retry the candidates at the settled correction.
3. **Rollback discipline, stated rather than implied:** phase 2 restores `afc_correction_hz` on every
   failure and commits only on success — the same rule every other arm follows, and the one #1143
   closed on the path that lacked it.
4. **Skip the retry** where the settle fails a guard, lands inside `AFC_SETTLE_DEADBAND_HZ` (phase 1
   already proved correction ≈ 0 fails), or is rejected by the #1049 veto where the mode publishes a
   template. Honest limit: the documented noise-settle failure mode is *confident*-but-bogus
   (measured 257 Hz, 81.2 Hz), so guard-failures will not be the majority on noise.
5. **Feed the #1157 calibration at the new veto call site.** `rho_calibration.push_at` currently lives
   in the CLI path's veto branch only; without this the `DORMANT(#1118)` getters stay CLI-fed and the
   promise recorded on #1118 goes unmet.

### Why C rather than settling always, or once per burst

**The cost argument does not carry it — the no-regression property does.** Review demolished the
rarity claim and it is not repeated here: on a real band every burst that crosses the DCD and is not
our own on-frequency traffic fails phase 1 by definition, so phase 2 is effectively always-on for
everything except what already works. The honest bound is **≈2× today's per-failed-burst decode work
plus O(100) settles**, and C is *more* expensive than settling-always on a failed burst. What C buys
is that the working case cannot regress — worth 2× in a codebase whose acquisition path has been
repaired five times.

**Steady state is better than the band-level bound suggests, and this is the premise review
corrected.** An earlier draft claimed nothing on this path ever sets `afc_correction_hz`. False:
`decode_burst_inner` commits the AFC estimate on a successful decode, deliberately (#1143). So the
first phase-2 success caches the peer's offset and every later burst of that QSO decodes in **phase
1**, at today's cost. A cross-burst estimate is therefore not an alternative to this design — it is
its steady state, already half-built.

Two stations at different offsets ping-pong the cache; the loser pays a phase-2 pass. That
self-heals, because `afc_mini_settle` zeroes the correction before settling, so a stale cache cannot
poison the settle itself. Per-station correction caches are **not** proposed: the ping-pong is an
optimisation target, not a correctness hole.

### Path inventory (cross-cutting concern)

| entry | acquisition today | after this change |
|---|---|---|
| daemon `accumulate_capture` → `decode_burst` / `ota_decode_burst` | onset scan only | phase 1 + phase 2 |
| daemon `receive_ota_ack_within` (ISS ACK listen) | **none** — its own accumulate loop | **transitively**, via the cached correction, *if* the ISS decoded anything from the peer first |
| CLI `receive --listen-ms` | full chain | unchanged |
| ARDOP / KISS / TUI / monitor | none (single-window `receive`) | unchanged — out of scope here |

**The ACK path is the one review found and this design must not hide.** FSK4-ACK tone spacing is
100 Hz, so a ±50 Hz offset is catastrophic there; `decode_fsk4_ack` goes through
`stage_demodulate_payload` and so inherits the cached correction, which means C covers it *only if*
the session already decoded something from that peer. Nothing proves that chain today, which is why
the acceptance gate below is a round trip rather than a decode. Noted in passing:
`decode_mfsk16_k3_ack` mixes at bare `center_frequency` without the correction — probably survivable
because MFSK16 self-acquires, but it is an inconsistency worth its own look.

## What is deliberately excluded

* **`receive_with_timeout_fec`'s listen deadline and retry regime.** Only `afc_mini_settle` and its
  guards move. The daemon must not inherit a wall clock (#1066).
* **The energy gate.** The daemon has its own noise-floor-relative carrier detect (REQ-DCD-ADAPT,
  #1055); M1 verified it located every corpus frame, and the M2 control decodes without the gate.
* **The condemnation recovery** — *and this exclusion is pinned to phase 2 settling at **every**
  scanned onset*. The recovery exists to walk a scanning receiver past a bad anchor inside one long
  listen; a per-onset walk subsumes it. If phase 2 is ever optimised to settle once per burst or at a
  "best" onset, this premise dies, and the design has silently become the option that needs an onset
  criterion #1139 could not find.

## Acceptance

| Objective | Gate |
|---|---|
| The daemon acquires a carrier offset it cannot acquire today (REQ-PHY-03) | `m2_carrier_offset_sweep_cli_vs_daemon`, promoted from measurement to assertion at ±50 Hz |
| The on-frequency case does not regress | the same sweep's 0 Hz and 20 Hz rows, plus `daemon_vs_cli_on_real_captures::m1` |
| A **two-daemon ARQ round trip** survives an inter-rig offset, ACK path included | twin-daemon harness with a `CfoChannel` at the bridge tap, ±64 Hz minimum |
| Phase 2 does not run on bursts phase 1 decoded | a counter, asserted zero on the on-frequency rows |

## Findings ledger

| ID | Finding | State |
|---|---|---|
| F-1118-04 | `decode_mfsk16_k3_ack` mixes at bare `center_frequency`, omitting `afc_correction_hz`, where every other decode path uses the sum. | deferred — probably survivable (MFSK16 self-acquires); tracked on #1118 |
| F-1118-03 | The ISS ACK listen (`receive_ota_ack_within`) has no acquisition of its own and is covered only transitively by the cached correction. | deferred — the acceptance round trip is what would expose it; a direct fix is out of scope here |
| F-1118-02 | "~400 Hz inter-rig offsets" is unsupported; provenance is a measurement marked unreliable in its own source. | fixed — corrected here, in the harness header and in `openpulse-book.md` |
| F-1118-01 | "Nothing on the daemon path sets `afc_correction_hz`" was false; the success branch commits it (#1143). | fixed — the design now treats cross-burst caching as its steady state |
