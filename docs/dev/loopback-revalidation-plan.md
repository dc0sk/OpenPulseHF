# Loopback re-validation at HEAD — plan

**Why this exists.** The pre-1.x completeness audit (2026-07-18) found that no audio-loopback
evidence in the tree is current, and that the two rungs the fading ladder most depends on have never
been run on real audio at all. This plan closes that, using rigs that already exist. It needs no
radio and no license — it is the largest gain in what we actually know that is available at a desk.

Related: [virtual-loopback.md](virtual-loopback.md), [dualcard-loopback.md](dualcard-loopback.md),
[../dev/project/release-1.0-criteria.md](project/release-1.0-criteria.md).

---

> **STATUS: COMPLETE (2026-07-20).** All five tasks done. Both rungs re-run at HEAD with `FEC=rs` —
> virtual 63/73 (#998), dual-card 55/67 (#997) — and **every rung of the `hpx_hf` ladder now decodes on
> real audio**, which was the gap this plan existed to close.
>
> Executing it found five defects the plan did not anticipate, three of them in the tooling the plan
> depends on: a scanning-receive window bug that blocked every long coded frame (#995), a 60 s flush
> clamp that made `BPSK31` untransmittable (#997), cpal enumeration truncating so the virtual rung
> could not run at all (#998), an 8PSK samples/symbol floor, and this plan's own Task B command being
> inert. It also **falsified the dual-card rig's dual-clock premise by measurement** (+0.10 ppm), which
> re-attributed the `SCFDMA52-*`/`64QAM` failures to the analog path and closed `reference-mining` C1.

## 1. The gap, stated precisely

| Fact | Evidence |
|---|---|
| Newest recorded loopback run is **2026-06-25** | `docs/dev/test-reports/loopback-dualcard-quick-2026-06-25T175642Z.json` |
| **131 commits** have touched `plugins/`, modem DSP, `openpulse-dsp`, or `profile.rs` since | `git log --since=2026-06-25 -- plugins/ crates/openpulse-modem/src crates/openpulse-dsp/src crates/openpulse-core/src/profile.rs` |
| `QPSK250-D`, `QPSK500-D`, `MFSK16`, `MFSK16-ACK`, JS8 appear in **no** loopback or on-air record | grep across `docs/dev/*loopback*.md`, `onair-*.md`, `docs/dev/test-reports/*.json` |
| The 2026-06-19 hardware result has **no retained JSON** | only a single-case BPSK250 smoke file is on disk |

The intervening work is not incidental — it is the fade-aware ladder re-seat (#932), differential
QPSK (#928), the SNR-estimator rework (#934/#944), and the MFSK16 ARQ rung. The ladder that a
hardware run would exercise today is not the ladder that was last exercised.

### 1.1 Why "just re-run it" would not have closed the gap

`scripts/run-loopback-dualcard.sh` selects cases from **hardcoded arrays** (`QUICK_CASES`,
`FULL_CASES`, lines 53–77): 14 cases, none of them `-D`, MFSK16, PILOT, 64QAM, or any `-RRC`
variant. Re-running `--full` at HEAD today would report 14/14 and still tell us nothing about the
rungs in question.

`scripts/run-loopback-virtual.sh` does **not** have this defect — it enumerates the mode set from
`openpulse modes` at run time (lines 34–42, "no curated exclusions") and reports physically
impossible modes as SKIP-with-reason. That is the correct design.

**So the first task is not a test run, it is fixing the hardware runner to match the virtual one.**
This is the same class as the audit's `CRATES_TESTED` finding: a hardcoded list standing in for an
enumeration, drifting silently as the registry grows.

---

## 2. Plan

### Task A — make the hardware runner registry-driven (no hardware needed) ✅ DONE (#989)

Port the virtual runner's enumeration into `run-loopback-dualcard.sh`:

- Derive the case list from `openpulse modes`, as `run-loopback-virtual.sh` does.
- Keep `QUICK_CASES` as an explicit **fast subset** (that is a legitimate tier), but make `--full`
  mean *the registry*, not a frozen list.
- Choose payload per mode from the existing convention (32 B for the slow BPSK rungs, 64 B mid,
  128 B for ≥500 baud); default rather than enumerate, so a new mode needs no script edit.
- Emit SKIP-with-reason for modes that cannot run at 8 kHz (the 9600-baud family needs Fs ≥ 38.4 kHz)
  and for `MFSK16` if its ~17 s/frame makes it impractical in a long sweep — **skip with a reason is
  fine; silent omission is what caused this**.

**Acceptance:** running `--full --dry-run` lists every registered mode exactly once, each either
scheduled or skipped-with-reason, and the count matches `openpulse modes`. A new mode added to any
plugin appears without editing the script.

### Task B — virtual rung at HEAD (needs the dev host only) ✅ DONE (2026-07-20, #998) — 63/73

> The commands below were **inert as written**: `run-loopback-virtual.sh` never passed `--fec`, so
> `FEC=rs` did nothing — and an uncoded differential run decodes 0.00 *by design*, exactly the false
> regression the note two paragraphs down warns against. The runner now supports `FEC=`. The rung was
> also unrunnable for an unrelated reason: cpal's ALSA enumeration truncated when devices were
> retained, so every `aloop_*` device reported "device not found" (#998).

```bash
scripts/setup-virtual-loopback.sh
cargo build --release -p openpulse-cli          # cpal is on by default for the CLI
scripts/run-loopback-virtual.sh                 # full registry
```

Then targeted runs for the rungs that have never been on real audio. Note the `-D` modes **require
FEC** — a no-FEC differential run decodes 0.00 by design (see CLAUDE.md → *Known sharp edges*), so a
no-FEC failure here is expected and must not be recorded as a regression:

```bash
FEC=rs MODES="QPSK250-D QPSK500-D" scripts/run-loopback-virtual.sh
FEC=rs MODES="MFSK16" scripts/run-loopback-virtual.sh
```

### Task C — hardware rung at HEAD (needs the dual-card rig) ✅ DONE (2026-07-20, #997) — 55/67

```bash
scripts/run-loopback-dualcard.sh --level-check   # confirm cable + gain before anything
scripts/run-loopback-dualcard.sh --full
FEC=rs scripts/run-loopback-dualcard.sh --single-case "QPSK250-D|64"
FEC=rs scripts/run-loopback-dualcard.sh --single-case "MFSK16|32"
```

`CAPTURE_GAIN=16` is the known-good value for this host; max clips a line→mic cable.

### Task D — retain the artifacts ✅ DONE — full-tier JSON + logs committed for both rungs

Both runners already write JSON to `docs/dev/test-reports/`. The 2026-06-19 full-tier result was
recorded only as doc prose and its JSON is gone, which is why the audit could not confirm it.

- Commit the full-tier JSON from both rungs (`git add -f` — `loopback-*.json` is gitignored by
  default; that default is right for debug runs and wrong for a milestone record).
- Update the **Status** sections of `virtual-loopback.md` and `dualcard-loopback.md` with the date,
  the commit, and the per-mode result.

### Task E — resolve the contradiction the audit found ✅ DONE (2026-07-20)

The contradiction was real, but it resolved in a direction this task did not anticipate — **including
in this task's own instructions**, which are preserved below and corrected here.

**What was asked:** mark the 2026-06-13 diagnostic superseded, "*(it is a dated record, and the SRO
reasoning it contains is still the right explanation for the modes that do still fail)*".

**That parenthetical is false.** The SRO reasoning is not the right explanation for any mode on this
rig, because **the rig has no meaningful sample-rate offset**: `--sro-check` measures **+0.10 ppm**.
Both USB adapters slave to the host's USB frame clock, so "two independent clocks" was an inference
from topology that had never been measured. The modes in question also tolerate 400–800 ppm of
*injected* SRO in-process.

**How it actually resolves.** The 2026-06-13 note offered a disjunction — "two independent soundcard
clocks (sample-rate offset) **and/or** analog group-delay/phase". Three rungs now separate the
variables cleanly:

| Rung | Adds | Result for `SCFDMA52-*` / `64QAM` |
|---|---|---|
| in-process (`ChannelSimHarness`) | nothing | pass |
| virtual (`snd-aloop`) | real cpal/ALSA/resampler | **pass** |
| dual-card | a real analog cable (**not** a second clock) | **fail** |

The clock half is eliminated by measurement; the analog half is confirmed by construction. These modes
are **analog-path limited**. The apparent contradiction between the two documents was two different
mechanisms being filed under one label.

**Downstream, as this task warned:** `reference-mining` item **C1** was prioritized on the refuted
premise. C1 is now closed twice over — the SRO channel model is *already implemented*
(`openpulse_channel::sro`, reachable via `ChannelSimHarness::route_with_sro`), and the failure it was
meant to gate is not an SRO. Corrected in `docs/dev/research/reference-mining-plan.md`.

Documents corrected: `dualcard-loopback.md`, `virtual-loopback.md`, `openpulse-manual.md`,
`openpulse-book.md`, `reference-mining-plan.md`.

**The transferable lesson:** the dual-card rig was described as the dual-clock rung for months on the
strength of its *topology*. One 60-second tone measurement refuted it. A rig's claimed property is a
hypothesis until something measures it — and a remediation plan built on that property inherits the
error.

---

## 3. What this does and does not establish

**Does:** that the current DSP survives a real cpal/ALSA/resampler path (virtual) and a real analog
cable (hardware). For the `-D` and MFSK16 rungs, that is the first evidence of either kind.

> **Correction (2026-07-20):** this originally read "two independent sample clocks plus an analog
> cable". The dual-card rig was **measured at +0.10 ppm** — its two USB adapters share the host's USB
> frame clock, so it does *not* deliver independent clocks and never did. Genuine sample-rate-offset
> coverage needs two hosts (rung 2b) or an injected offset (`ChannelSimHarness::route_with_sro`).

**Does not:** anything about RF, band noise, real multipath, or a real fading channel. Every fading
claim in this repo remains a Watterson-simulator claim after this plan completes. Group A of the 1.0
criteria is untouched by it.

The distinction matters most for exactly the rungs being added. `QPSK250-D` exists *because*
coherent QPSK measured 0.00 on simulated `moderate_f1`; MFSK16 is the sub-floor rung for when
everything else fails. Passing a **non-fading** loopback confirms they are correctly implemented and
survive real hardware — it does not confirm the fade behaviour that motivated them. Recording it as
anything more would repeat the error the fade-aware ladder arc was written to correct: an
AWGN-calibrated ladder is not an HF ladder.

---

## 4. Order

Task A first — it is the reason the gap persisted, it needs no hardware, and doing B or C before it
would produce another 14-case result that looks like coverage and is not.

Then B (dev host, cheap, repeatable), then C (needs the rig), then D and E as the write-up.
