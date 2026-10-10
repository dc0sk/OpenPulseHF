---
project: openpulsehf
doc: docs/dev/project/maintainer-todo.md
status: draft
last_updated: 2026-10-10
---

# Maintainer TODO — what only you can do

Things that need your hands, your rigs, your credentials, or your decision. Claude keeps this list current
and ticks items off when you report back; everything else is in [`workplan.md`](workplan.md). Newest
asks first within each section. Report results in chat or as a comment on the linked issue.

## Now — unblocks Release 1 work

- [ ] **Run the key-to-audio probe on both station Pis** (work plan M2, key-to-audio gap). The IC-9700
  SDR captures suggest ~1.3 s of dead air between PTT and the first sample on every keyed turn; the
  modulator is ruled out (≤ 26 ms), so this times the audio device path. It writes **silence**, so
  nothing is radiated even with VOX on; PTT is not touched. It uses the output device the on-air daemon is configured with
  (`[audio] device` in the config `run-onair-twin-ota.sh` writes; empty would mean the system
  default, and then `PROBE_DEVICE` is left unset). Both Pis, because this times each station's own
  audio device, and the two drive different rigs. A named device matters: production enumerates every
  output device on each open only when a name is configured, and the on-air config always names one.
  rpi51 on the system default (no config at the old path; `10c9ec9f`): enumerate 1066 / 530 / 483 /
  485 / 582 ms, `open_output` 30–118 ms, write → drained − audio 132–144 ms. Enumeration would be
  most of the ~1.3 s gap, if the station's named device enumerates the same way:
  ```bash
  git checkout main && git pull && git log --oneline -1
  DEV=$(sed -n '/^\[audio\]/,/^\[/s/^device *= *"\(.*\)"/\1/p' ~/.config/openpulse-twin-ota/openpulse/config.toml)
  env ${DEV:+PROBE_DEVICE="$DEV"} cargo test --release -p openpulse-audio \
    --features cpal-backend --lib key_to_audio -- --ignored --nocapture
  ```
  If that file is missing, set `DEV` by hand to the station's `A_AUDIO_DEVICE` / `B_AUDIO_DEVICE` from
  your on-air profile. Paste the five `run …` lines and the device name from each Pi. If enumeration or `open_output` is most of it, the fix
  is opening the stream before keying (a PTT-timing change, so a design review first).

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
