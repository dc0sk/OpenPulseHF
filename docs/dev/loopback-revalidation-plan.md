# Loopback re-validation at HEAD — plan

**Why this exists.** The pre-1.x completeness audit (2026-07-18) found that no audio-loopback
evidence in the tree is current, and that the two rungs the fading ladder most depends on have never
been run on real audio at all. This plan closes that, using rigs that already exist. It needs no
radio and no license — it is the largest gain in what we actually know that is available at a desk.

Related: [virtual-loopback.md](virtual-loopback.md), [dualcard-loopback.md](dualcard-loopback.md),
[../dev/project/release-1.0-criteria.md](project/release-1.0-criteria.md).

---

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

### Task A — make the hardware runner registry-driven (no hardware needed)

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

### Task B — virtual rung at HEAD (needs the dev host only)

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

### Task C — hardware rung at HEAD (needs the dual-card rig)

```bash
scripts/run-loopback-dualcard.sh --level-check   # confirm cable + gain before anything
scripts/run-loopback-dualcard.sh --full
FEC=rs scripts/run-loopback-dualcard.sh --single-case "QPSK250-D|64"
FEC=rs scripts/run-loopback-dualcard.sh --single-case "MFSK16|32"
```

`CAPTURE_GAIN=16` is the known-good value for this host; max clips a line→mic cable.

### Task D — retain the artifacts

Both runners already write JSON to `docs/dev/test-reports/`. The 2026-06-19 full-tier result was
recorded only as doc prose and its JSON is gone, which is why the audit could not confirm it.

- Commit the full-tier JSON from both rungs (`git add -f` — `loopback-*.json` is gitignored by
  default; that default is right for debug runs and wrong for a milestone record).
- Update the **Status** sections of `virtual-loopback.md` and `dualcard-loopback.md` with the date,
  the commit, and the per-mode result.

### Task E — resolve the contradiction the audit found

`virtual-loopback.md:64-66` still records the 2026-06-13 diagnostic that SCFDMA52-*/64QAM "fail 0/8
on the hardware rig" (attributing it to sample-rate offset), while `dualcard-loopback.md:99-110`
records SCFDMA52 **passing** on a real dual-clock path six days later. Both cannot be current. The
2026-06-25 run supersedes the diagnostic; mark it as such rather than deleting it (it is a dated
record, and the SRO reasoning it contains is still the right explanation for the modes that *do*
still fail). This matters beyond bookkeeping: `reference-mining` item C1 is prioritized on the
refuted premise.

---

## 3. What this does and does not establish

**Does:** that the current DSP survives a real cpal/ALSA/resampler path (virtual), and two
independent sample clocks plus an analog cable (hardware). For the `-D` and MFSK16 rungs, that is
the first evidence of either kind.

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
