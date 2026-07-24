---
project: openpulsehf
doc: docs/dev/onair-execution-plan.md
status: living
last_updated: 2026-07-23
---

# On-air execution plan

The sequenced plan to obtain the on-air evidence 1.0 needs (release-1.0-criteria.md group **A**:
A1 two-station HF QSO on the `hpx_hf` ladder, A2 rate ladder observed adapting on a *real* fading
channel, A3 one Winlink message over RF). It is written to be executed against the tooling as it
stands **today** — the on-air scripts were brought current on 2026-07-23; the currency audit and the
fixes are in the "Tooling readiness" section at the end.

**Read this first, because it changes the order of everything below:** the modem, every shipping
waveform, the decoder, and two independent transmitters are already **proven on real 2 m RF** — a
station's engine-TX waveform was captured off-air by an SDR and decoded to the exact payload,
repeatably, with a clean 298 Hz BPSK250 lobe and no splatter. What has *never* completed is a
two-station **rig→rig** link, and the reason is documented and is **not a modem defect**: computer-
borne RFI conducted into each rig's USB-audio capture, sitting 30–40 dB over the wanted signal right
in the modem passband. Ferrites and USB-port swaps did not touch it (it is conducted, not radiated).
The recorded fix is **galvanic USB isolation** (an ADuM-class USB isolator) or audio-isolation
transformers on a rear DATA/ACC jack instead of the USB CODEC.

So the plan is not "debug the modem on the air". It is: **(1) remove the receive-side RFI so a rig can
actually hear, then (2) collect the A1–A3 evidence, using the SDR as the trusted reference monitor
throughout.** The SDR is the instrument that already works; every rig-RX result is checked against it.

---

## 0. Ground truth (what is already proven, so we do not re-litigate it)

| Claim | Status | Evidence |
|---|---|---|
| Engine TX waveform + PA chain is clean on RF | **PROVEN** | SDR measured 298 Hz −26 dB BPSK250 lobe, no splatter, at 5 W into 2 m |
| The decoder works off-air | **PROVEN** | SDR capture of a rig's TX decoded the exact payload, repeatably |
| Both a FT-991A (008924A1) and an IC-705 transmit clean, decodable signals | **PROVEN** | SDR decodes both |
| CE-SSB gives ~+1.2 dB avg power on a real PA | **PROVEN** | on-air A/B, RM2 OFF 35.1 → ON 46.1 = +1.18 dB at constant ALC |
| A rig→rig link completes | **BLOCKED** | conducted computer RFI into RX USB audio; not a modem bug |
| Rate ladder adapts on a real fade | **NOT YET DONE** | simulator only; this is A2 |
| Winlink message over RF | **NOT YET DONE** | loopback + mock-CMS only; this is A3 |

The one faulty rig from the earlier campaign (FT-991A 007174ED — a hardware TX distortion that
followed the rig across hosts) has been retired; do not reuse it. Healthy hardware: FT-991A 008924A1,
IC-705 (hamlib 3085), and the SDRplay RSP2pro on the dev host.

---

## 1. Phase G0 — Fix the receive path (the actual blocker)

**Goal:** a rig that can hear a signal at the USB-audio output, verified before any modem run.
**Owner action (hardware):** this phase is physical and cannot be done in software.

1. **Install galvanic USB isolation on each rig's CODEC link.** An ADuM3160/4160-class USB isolator
   between the host and each rig's USB-audio interface. Ferrites alone are documented insufficient.
   Alternative: drive/capture audio through a rear DATA/ACC jack with 1:1 isolation transformers
   instead of the rig's USB CODEC.
2. **Measure the idle noise floor after isolation, on each rig, with the runnable gate:**
   ```bash
   # locally, or over SSH for a remote rig — NO TX anywhere, SDR stopped:
   scripts/onair-rx-idle-floor.sh plughw:CARD=CODEC,DEV=0
   ssh dc0sk@dc0sk-rpi53 'cd ~/git/OpenPulseHF && scripts/onair-rx-idle-floor.sh plughw:CARD=CODEC,DEV=0'
   ```
   It captures ~5 s of the rig's RX USB audio and runs `scripts/onair-rx-idle-floor.py`, which fails
   on any narrow line standing ≥15 dB above the broadband floor (or above −40 dBFS) in 300–2600 Hz —
   the birdies the campaign recorded (600–1400 Hz on the FT-991A; 1286/1394/1745 Hz on the IC-705).
   The prominence criterion is gain-independent (a real birdie is ~40 dB up; ADC noise is flat), so
   it works regardless of the operator's capture level. Exit 0 = clean, 1 = birdies (with the
   offending frequencies listed), 2 = capture error.
   - The analyzer was validated against synthetic captures before use: pure noise → PASS; three
     injected lines → exactly those three; a single −30 dBFS line → exactly one. Adjust the band /
     prominence / absolute thresholds via the `OPHF_*` env knobs if a rig needs different limits.
3. **Gate:** do not proceed to Phase G1 until **both** rigs return exit 0 from this script. A live
   birdie here is the whole reason the June campaign read 0/3 — it is cheaper to kill it now than to
   misattribute it again later.

**Exit criterion G0:** both healthy rigs present a clean RX USB-audio idle floor (no in-passband
birdies above −40 dBFS).

---

## 2. Phase G1 — Signal-chain gates (per rig pair, every session)

The reproducible RF-domain preflight already exists as
[onair-signal-chain-verification.md](onair-signal-chain-verification.md). It is the on-air analogue of
the dual-card rig's AGC guard: run **every gate in order, stop on the first failure**. Do not skip it
because "it worked last time" — USB re-enumeration and mixer resets between sessions are exactly what
made prior runs non-reproducible.

| Gate | What it proves | Pass criterion |
|---|---|---|
| G0 (theirs) | known clean state | USB CODEC enumerated both sides; no stale openpulse/rigctld |
| G1 | CAT connectivity | rigctld answers on both; frequency/mode read back |
| G2 | TX audio path | a 1500 Hz tone deflects ALC into 0.15–0.35 and shows on the far S-meter |
| G3 | **RX audio path** | a synchronized capture during a real BPSK250 TX shows peak mean-sq ≥ 0.005 |
| G4 | capture integrity | cpal chunks < 1024 samples @ 8 kHz |
| G5 | frequency alignment | carrier lands at 1500 ± a few Hz (Goertzel per-50-Hz-bin) |
| G6 | end-to-end decode | one BPSK250 frame decodes; result JSON `afc_correction` < 100 Hz |

**Gate 3 is the one Phase G0 exists to satisfy.** If G0's isolation worked, G3 passes; if G3 still
fails, the RFI is not fully killed — return to G0, do not proceed.

Two known code/config items from the June campaign to carry into G5/G6:
- a persistent **~1286 Hz interferer** was mitigated by moving off 144.651 MHz and capping
  `AFC_MAX_CORRECTION_HZ=100`. Pick an operating frequency with a clean passband (confirm on the SDR
  waterfall first).
- the one-shot receive's **retry window misalignment** (onair-status.md, Issue B): the retry scans
  `fep-step..fep+1024` but the real signal can arrive ~47 000 samples later. If G6 intermittently
  misses a frame that the SDR confirms was on the air, this is a live *code* item to fix in the
  engine's scanning receive, not a rig problem — file it and use the daemon streaming path (which
  holds the capture stream open) rather than the one-shot `receive` for the matrix.

**Exit criterion G1:** all seven gates pass for the chosen rig pair on the chosen frequency.

---

## 3. Phase A1 — Two-station QSO on the `hpx_hf` ladder

**Goal:** the A1 evidence — a two-station HF (or 2 m as a stand-in for the plumbing) QSO using the
ladder, logs retained.

**Runner:** `scripts/run-onair-ic9700-ft991a.sh` (the mature rigctld two-station runner; now applies
`--fec` on both ends and uses corrected timings) *or* `scripts/run-onair-tests.sh` (the generic
SSH-pair runner, now on the real `transmit`/`receive` CLI). Both build/require a **cpal** binary on
each Pi — do **not** deploy via `deploy-rpi-pair.sh` (no-audio; it now refuses by default).

**Station pairings** (each has a config profile + a setup doc):
- IC-9700 (rpi51) ↔ FT-991A (dd2zm) — [signal-chain-verification](onair-signal-chain-verification.md).
- IC-9700 (rpi51, stationary) ↔ FT-818 + SCU-17 (this laptop, portable) —
  [onair-ic9700-ft818-setup.md](onair-ic9700-ft818-setup.md), profile
  `docs/config/onair-ic9700-ft818.example.sh`. 2 m 144.640 MHz; SDR co-located with the portable end.

Sequence:
1. **SDR up first, always.** Start `scripts/onair-sdr/sdr_capture.py` on the dev host so every TX is
   independently monitored. The SDR is the arbiter: if a rig-RX fails but the SDR decoded the same
   burst, the fault is receive-side (RFI/level), not the waveform.
2. **`calibrate drive`** on the transmitting rig (`openpulse calibrate drive`, needs rigctld + a real
   radio) to land ALC in the moderate band. On-air-validated behaviour; converges in ~4 iterations.
3. **Quick tier first:** `--quick` (BPSK250 × {none, rs, soft-concatenated}). A single clean BPSK250
   decode rig→rig is the moment A1's plumbing is real. Retain the JSON + the SDR capture.
4. **Ladder walk for A2 setup:** run the fade rungs the plan now includes — MFSK16 (SL1), QPSK250-D
   (SL6) — FEC-protected. These are the rungs that carry the ladder across a fade; A2 needs them to
   work individually before the adapter can step through them.

**Exit criterion A1:** ≥1 mode decodes rig→rig (not just SDR), with the JSON and SDR capture retained
via `scripts/onair-bundle-evidence.sh`.

---

## 4. Phase A2 — Rate ladder adapting on a real fading channel

**Goal:** the load-bearing 1.0 claim — the ladder observed *climbing and demoting on a real fade*,
≥3 rung transitions driven by channel conditions, not by the simulator.

This needs a genuinely fading path. Options, in order of availability:
1. **HF NVIS on a band with real Doppler/QSB** (40 m at the right time of day) between two stations
   far enough apart to fade. This is the real test; 2 m line-of-sight will not fade.
2. If two HF stations are not simultaneously available, an **SDR-monitored single HF path** with one
   TX and the SDR as RX still captures ladder *demotion* under a real fade (climb needs the ACK
   return, so this only gets half of A2).

**Runner:** the daemon OTA path — `scripts/run-onair-twin-ota.sh` drives two `openpulse-server`
daemons with the receiver-led rate stepping (`ota-status` polling). It is the only runner that
exercises the *adaptive* ladder rather than fixed-mode cases. Confirm it uses the cpal-built
`openpulse-server` (it does; the control-client CLI it also builds is deliberately no-audio and only
talks to the daemon).

Watch for the **two-scale SNR boundary** (CLAUDE.md): SL2–SL6 report true channel SNR, SL7+ report a
saturation-bounded plugin SNR. The evidence climb is what carries the ladder across that boundary — a
real fade is exactly where you would see it either work or stall, so this is also a validation of the
#934 evidence-based climb on real conditions.

**Exit criterion A2:** a retained session log showing ≥3 ladder transitions attributable to measured
channel change (SNR log + FER per rung), with the fade independently visible on the SDR.

---

## 5. Phase A3 — Winlink message over RF

**Goal:** one end-to-end Winlink message across RF to a real CMS/RMS gateway.

**Runner:** `openpulse-ardop` TNC + `pat`, per on-air_testplan.md §6.3. The gateway path
(`openpulse-gateway`) is direct-TCP to CMS and is **not** the RF path — do not use `--mode` on it (it
has none). The RF path is: `pat` → `openpulse-ardop` TNC → modem → RF → a real RMS station → CMS.

Prerequisite: A1 must pass first (a working rig→rig link is the substrate). A3 is then a protocol
exercise on top of it, not a new signal-path unknown.

**Exit criterion A3:** a retained session log plus the delivered message, round-tripped through a real
RMS/CMS.

---

## 6. Phase A4/A5 — Regulatory + safety (checks, run alongside)

These are checks, not discoveries, and run during the A1–A3 windows:
- **A4 station ID cadence:** `openpulse beacon` / the daemon's periodic ID; verify ≤10-minute cadence
  on the SDR waterfall against the operator's national rule. Run the compliance checklist in
  on-air_testplan.md §7.
- **A5 PTT fail-safe:** deliberate fault injection during an on-air window — kill the controlling
  process mid-TX and confirm the rig un-keys (watchdog + `finally: TX0;` path). The dev-host campaign
  already burned a "stuck carrier on USB drop" incident; verify the time-out timer (TOT) is enabled on
  each rig as a hardware backstop before any keyed run.

---

## 7. Evidence and bundling

Every phase writes its artifacts through the existing pipeline:
- per-run JSON from the matrix runners → `docs/dev/test-reports/on-air/`
- `scripts/onair-bundle-evidence.sh` packages the JSON + logs + `git-status.short.txt` + the SDR
  captures into a dated bundle
- `scripts/onair-generate-report.sh` renders it

Two integrity rules, both learned the hard way in this repo:
1. **Never bundle a run where FEC was decorative.** The runners now apply `--fec`; a bundle from a
   pre-2026-07-23 runner recorded FEC labels that were never applied — treat those bundles as void.
2. **A rig-RX pass is only trusted if the SDR corroborates it,** or if G1–G6 passed clean in the same
   session. A decode over a birdie-laden capture is not evidence; the SDR is the arbiter.

---

## 8. Tooling readiness (currency audit, 2026-07-23)

Audited against `openpulse modes` and the live `--help`. **Fixed this session** (commit in this PR):

| Script | Was | Now |
|---|---|---|
| `deploy-rpi-pair.sh` | cross-built `--no-default-features` → deployed **no-audio** binaries that key nothing; nothing warned | refuses by default (`ALLOW_NO_AUDIO_DEPLOY=1` to override), documents the on-Pi cpal build |
| `run-onair-tests.sh` | called `openpulse send --callsign --to --hex` and `openpulse-tnc --listen` — **none exist** | real `transmit`/`receive` + `--backend cpal` + `--fec` + `--device` + rigctld `--ptt`; checks the decoded payload; adds MFSK16 + QPSK250-D fade rungs |
| `run-onair-ic9700-ft991a.sh` | never passed `--fec` (evidence claimed a FEC never applied); `IRS_STARTUP_WAIT=5`; bare `sleep 2` | `--fec` on both ends; startup 10 s; `KILL_WAIT=12`; RX-invalid `concatenated` case dropped |
| `run-onair-tx500-kx3.sh` | same missing-`--fec`; a false "CLI does not expose FEC" note | `--fec` on both ends; note corrected; timings fixed |
| `onair-preflight.sh` | checked binary *presence* only — a loopback-only build passed | adds a `--backend cpal devices` probe that catches the no-audio footgun |

**Verified without a rig:** every changed CLI invocation parses and runs on the loopback backend;
every FEC token in the case lists transmits and receives; all five scripts pass `bash -n`. **Not
verifiable without two rigs:** that any runner completes an actual on-air QSO — that is Phase A1, and
it is what this plan exists to reach.

**Still hardcoded, deliberately deferred:** the matrix runners' mode lists are static (not
`enumerate_registry_modes` like the loopback runner). For a two-station on-air matrix this is lower
risk than for a loopback sweep — each case names a valid mode with an explicit FEC — and the fade
rungs the test plan requires are now present. Full registry-driven enumeration with per-mode airtime
scaling is a follow-up, noted so it does not read as done.

**One live code item** (not tooling): the one-shot `receive` retry-window misalignment
(onair-status.md Issue B). If G6/A1 intermittently miss an SDR-confirmed frame, fix it in the engine
scanning receive or route the matrix through the daemon streaming path. Filed here rather than lost.

---

## 9. Critical path, one line

**G0 (kill the RX RFI) → G1 (seven gates pass) → A1 (one rig→rig decode) → A2 (ladder on a real fade)
→ A3 (Winlink over RF).** A4/A5 run alongside. The SDR monitors every step and is the arbiter of any
rig-RX result. Everything upstream of G0 — the modem, the waveforms, the transmitters — is already
proven; the campaign is a receive-path and propagation exercise, not a modem debug.
