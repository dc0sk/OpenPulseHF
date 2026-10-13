---
project: openpulsehf
doc: docs/dev/project/maintainer-todo.md
status: draft
last_updated: 2026-10-13
---

# Maintainer TODO — what only you can do

Things that need your hands, your rigs, your credentials, or your decision. Claude keeps this list current
and ticks items off when you report back; everything else is in [`workplan.md`](workplan.md). Newest
asks first within each section. Report results in chat or as a comment on the linked issue.

## Now — unblocks Release 1 work

- [ ] **After the device-name cache merges, re-run the key-to-audio probe on rpi51** (work plan M2,
  key-to-audio gap; the rpi51 agent session can do it). It writes **silence** and does not touch PTT.
  ```bash
  git checkout main && git pull && git log --oneline -1
  env PROBE_DEVICE='plughw:CARD=CODEC,DEV=0' PROBE_INPUT_DEVICE='plughw:CARD=CODEC,DEV=0' \
    cargo test --release -p openpulse-audio --features cpal-backend --lib key_to_audio -- --ignored --nocapture
  ```
  Expected: `open_output` near 45 ms from run 2 on (run 1 resolves in full). Paste the five `run …`
  lines; they also give key-to-first-sample, the number the G1 leader check is set against.

- [ ] **rpi53: one device listing, no probe.** rpi53 is the same Pi 5 on the same build, so the probe
  would repeat rpi51's numbers. What differs per station is where the FT-818's sound card sits in
  ALSA's device order, since a cached open still scans every device before it. No rig needed:
  ```bash
  aplay -L | grep -v '^ ' | nl
  ```
  Paste the output and say which line is the FT-818's interface.

- [x] **Key-to-audio probe on rpi51, named device** (done 2026-10-12, `main` after #1508, by the rpi51
  agent session). The station's device is **`plughw:CARD=CODEC,DEV=0`** (the IC-9700's USB codec);
  use it as `A_AUDIO_DEVICE`. Capture held open on the same device. Five runs:
  - pass 1 (all 16 devices, named) 525–540 ms;
  - pass 2 (`find` the name) 42–45 ms;
  - `open_output` by name 567–684 ms;
  - drain overhead 138–144 ms;
  - **key-to-audio 707–828 ms**.

  No device vanished with the capture held. Design round 2 follows.

- [x] **Key-to-audio probe on rpi51** (done 2026-10-11, `09762a3a`, system default device, because
  the station config does not exist yet). Five runs:
  - enumerate 1 084 / 532 / 494 / 464 / 595 ms (16, then 9 devices);
  - `open_output` 95 / 32 / 32 / 52 / 47 ms;
  - write → drained − audio 133–144 ms.

  A named device pays the enumeration on every open, whatever its name, so the gap per keyed turn is
  about 0.65–1.3 s. That matches the ~1.3 s on the IC-9700 captures. The fix is designed next (work
  plan M2).

- [x] **Run the receive-cost probe with the PN-63 candidate on rpi51** (#1062 F1c; done 2026-10-10,
  `main`). Decode per frame, shipped → PN: SL5 1.28 → 1.55 s, SL4 1.54 → 2.17 s, SL3 1.98 → 3.14 s,
  SL2 3.63 → 6.29 s. PASS: SL2 + 0.52 s + 1 s = 7.81 s ≤ 9 s; on SL3–SL5, where PN-63 goes
  (decision 25), the worst is SL3 at 4.66 s.

- [x] **Re-run the receive-cost probe on one Pi after the onset-ranking fix merges** (done
  2026-10-10 on rpi51, `PROBE_READ=4096`; commit not recorded, since the checkout of the deleted branch failed
  and the run used the tree already there). Decode per frame, 0 Hz / +50 Hz: SL6 0.31 / 0.33 s, SL5
  1.28 / 1.33 s, SL4 1.54 / 1.60 s, SL3 1.98 / 2.09 s, SL2 3.63 / 3.75 s (2026-10-04: SL2 7.3 s).
  SL2 + 0.52 s FSK4 ACK + 1 s = 5.2 s, inside the 9 s window; with the ≈5 s MFSK16 ACK, 8.6 s, inside
  but without the full 1 s margin.

- [x] **Run the receive-cost probe on the station Pis** (done 2026-10-04, `PROBE_READ=4096`). Decode
  per frame, rpi53 / rpi51: SL6 3.21 / 3.32 s, SL5 5.19 / 5.45 s, SL4 5.56 / 5.76 s, SL3 5.90 /
  6.12 s, SL2 7.04 / 7.26 s. All five decoded.

- [x] **Create the GitHub milestones and move the stand-in labels onto them** (done 2026-10-04) (decision 20; the cloud
  session cannot create milestones). From a checkout with `gh` logged in:
  ```bash
  R=dc0sk/OpenPulseHF
  for t in "M1 v0.17 wire format" "M2 release path" "M3 on air (2 m)" "M4 release 1 (v0.17.0)" "after v0.17, before 1.0"; do
    gh api repos/$R/milestones -f title="$t" >/dev/null
  done
  move() { # label milestone-title
    gh issue list -R $R --label "$1" --state all --json number -q '.[].number' |
      xargs -r -I{} gh issue edit {} -R $R --milestone "$2" --remove-label "$1"
  }
  move milestone:M2 "M2 release path"
  move milestone:after-v0.17 "after v0.17, before 1.0"
  gh label delete milestone:M2 -R $R --yes; gh label delete milestone:after-v0.17 -R $R --yes
  ```

## Rig work — M3 on air (2 m)

Station pair (decided 2026-10-02): **rpi51 + IC-9700** and **rpi53 + FT-818**, local, 144.640 MHz.
FT-991A on dd2zm-landline later (remote; its RX path fails offline).

- [ ] **G0–G3 on the 2 m pair**, per the re-baselined [`onair-execution-plan.md`](../onair-execution-plan.md)
  (isolators fitted; G3 decides whether the RFI is gone). Build both ends from the same commit.
- [ ] **Leader delay**, once G1 passes: if the first frames fail to decode while later ones work,
  set `[modem] ptt_leader_ms` on the transmitting station and note the value. **150 ms** is a
  reasonable start for the IC-9700: on the recorded captures its key-up clicks sit 130 ms apart.
  That is a reading, not a measurement (`crates/openpulse-modem/tests/captures/README.md`). Today the
  daemon keys before it modulates, so every frame already carries its synthesis time as dead air. Note
  that gap too if you can: the `PttChanged` event time against the first audio on the other station.

## Done

- [x] #1456/#1460 stay in M2 (2026-10-02, decision 21).
- [x] #1367 question withdrawn (2026-10-02): it asked for a PTT line on the dual-card loopback rig, which has none. Claude handles #1367 in software, with tail and release checks on the dual-card rung and at G1.
- [x] Triage table approved (2026-10-02, decision 20).
- [x] Station hardware chosen for the 2 m campaign (2026-10-02).
- [x] Release scope, test stages, version `v0.17.0`, profiles `fast`/`robust`, RC UI client (decisions 1–19).
