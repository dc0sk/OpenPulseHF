---
project: openpulsehf
doc: docs/dev/project/maintainer-todo.md
status: draft
last_updated: 2026-10-04
---

# Maintainer TODO — what only you can do

Things that need your hands, your rigs, your credentials, or your decision. Claude keeps this list current
and ticks items off when you report back; everything else is in [`workplan.md`](workplan.md). Newest
asks first within each section. Report results in chat or as a comment on the linked issue.

## Now — unblocks Release 1 work

- [ ] **Re-run the receive-cost probe on one Pi after the onset-ranking fix merges.** The first run
  (2026-10-04, below) showed the decode leaves no margin in the 9 s ACK window at SL2, and none fits
  with the MFSK16 ACK. The fix
  ([`design/fallback-onset-ranking.md`](../design/fallback-onset-ranking.md)) brought x86 SL2 from
  4.6 s to 1.8 s; this confirms it on the station:
  ```bash
  git pull   # on main
  PROBE_ENTRY_RUNGS=1 PROBE_READ=4096 cargo test --release -p openpulse-modem \
    --no-default-features --test receive_cost_scaling -- --ignored --nocapture
  ```
  Paste the five `Sl…` lines.

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
