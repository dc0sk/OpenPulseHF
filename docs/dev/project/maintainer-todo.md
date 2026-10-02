---
project: openpulsehf
doc: docs/dev/project/maintainer-todo.md
status: draft
last_updated: 2026-10-02
---

# Maintainer TODO — what only you can do

Things that need your hands, your rigs, your credentials, or your decision. Claude keeps this list current
and ticks items off when you report back; everything else is in [`workplan.md`](workplan.md). Newest
asks first within each section. Report results in chat or as a comment on the linked issue.

## Now — unblocks Release 1 work

- [ ] **Run the receive-cost probe on the station Pis** (rpi51, rpi53). It decides whether the
  onset-ranking fix ([`design/fallback-onset-ranking.md`](../design/fallback-onset-ranking.md)) blocks
  Release 1: each decode plus the ACK's airtime must fit the sender's 9 s ACK window.
  ```bash
  git pull   # on main
  PROBE_ENTRY_RUNGS=1 PROBE_READ=4096 cargo test --release -p openpulse-modem \
    --no-default-features --test receive_cost_scaling -- --ignored --nocapture
  ```
  Paste the five `Sl…` lines it prints. If you know the daemon's typical read size on that Pi, use it
  for `PROBE_READ`; 4096 is a guess.

- [ ] **Create the GitHub milestones and move the stand-in labels onto them** (decision 20; the cloud
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

- [ ] **Decide: move #1456 and #1460 to post-release?** The NACK-decay design was reviewed and
  parked ([`design/nack-streak-decay.md`](../design/nack-streak-decay.md), review linked there). The
  reviewer judges neither Release 1 blocking: A2 on 2 m is scored on clean-decode windows, and the
  exchange cadence keeps real failures inside any gap. Idle-flicker demotion (#1456) bites between
  sessions; the foreign-over residual (#1460) is HF only. Say "move" or "keep in M2", and if they
  stay, I redesign against the review's falsifier.

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

## Questions for you

- **#1367 (200 ms sleep before PTT drop): what is the "dual-card rig"?** The measurement needs the
  PTT edge and the last emitted sample on ONE clock. The off-air SDR captures cannot show the release:
  an SSB rig with no audio radiates nothing visible. The cleanest setup is card A's output into card
  B's left input, with the PTT or SEND line through a divider into card B's right input. Tell me which
  cards and which PTT interface (serial RTS/DTR, CAT, CM108 GPIO), and whether a line can reach card B,
  and I write the script to fit.

## Done

- [x] Triage table approved (2026-10-02, decision 20).
- [x] Station hardware chosen for the 2 m campaign (2026-10-02).
- [x] Release scope, test stages, version `v0.17.0`, profiles `fast`/`robust`, RC UI client (decisions 1–19).
