---
project: openpulsehf
doc: docs/dev/reviews/review-twin-ota-ft991a-2026-10-31.md
status: resolved
last_updated: 2026-10-31
---

# Review: the A2 daemon runner drives the FT-991A pair (2026-10-31)

Adversarial review (Fable) of the uncommitted diff to `scripts/run-onair-twin-ota.sh`, later committed
as `9d38400`. The diff adds four things:
- a dial offset per station, with a band check and a read-back;
- a passband setting;
- a `deploy` action for a station without a checkout;
- a refusal to run unless both stations have the same binaries, plus `TRAFFIC_FROM=A|B`.

The review is mandatory because the runner keys two transmitters: the IC-9700 on rpi51 and the remote
FT-991A, on 2 m.

## Prompt

The prompt asked for falsification, not agreement. It asked the reviewer to find any path that keys:
- on a wrong or out-of-band frequency;
- at a higher power;
- with mismatched binaries;
- with the wrong station sending.

It also asked for:
- quoting and expansion bugs (`B_REPO_DIR` holds a literal `${HOME}` that must expand on the remote);
- a pipeline status used as a verdict;
- breakage of the A-sends default or of the FT-818 profile, including Hamlib's VFO A/B toggling;
- whether a run with `TRAFFIC_FROM=B` is symmetric in what the daemons do.

The diff and the fake-station harness went with it, and the prompt asked "Does this block Release 1?".

## Verdict

The review found no path that keys:
- on a wrong or out-of-band dial;
- at raised power;
- with mismatched daemons.

The binary check runs before rigctld starts. Both tunes (band check, set, read-back, lower-only power)
finish before any daemon starts. No verdict comes from a pipeline. The IRS keys too: `transmit_ota_ack`
runs under `keyed_transmit` in `server.rs`, so a run sending from B uses the same daemon paths. **It does
not block Release 1.**

## Findings

| # | Severity | Finding |
|---|---|---|
| 1 | should-fix | `TRAFFIC_FROM=B` sends nothing when B was only deployed to: `deploy` copied the two binaries but not `scripts/twin-traffic.sh`. The remote `&` hides the failure and the run polls for 120 s with no traffic. The harness hid it, because its fake B had the script. |
| 2 | should-fix | `deploy` ignored a failed build. It copied the previous binary, the hashes matched, and the evidence would be credited to the wrong commit. `setup` had the same hole (pre-existing). |
| 3 | should-fix | A daemon that does not start is not fatal (pre-existing). The other side keys for the whole traffic window to nobody. `deploy` also does not compare the stations' CPU architecture. |
| 4 | nit | `setup` builds on both stations, and two builds are rarely byte-identical, so the old refusal message ("run setup or deploy") pointed at the cause. |
| 5 | nit | The twin runner read different variable names from the FT-818 profile (pre-existing). So `M USB 3000` would have been sent to an FT-818 whose mode is set on the rig. The band defaults happen to coincide. |
| 6 | unconfirmed | FT-818 VFO toggling: the new `f` read-back is one more read of the same kind as the existing ones. A toggle makes the run refuse (safe), and nothing new is exposed. |
| 7 | nit | Only `openpulse-server` was hashed. A missing CLI shows up as `<no status>`, not a safety issue. |

## Consumer

`docs/dev/onair-execution-plan.md` §4 and the M3 A2 row in `docs/dev/project/workplan.md`. The A2 task in
`docs/dev/onair-tasks.md` (after A1, task 7) runs it with `docs/config/onair-ic9700-ft991a.example.sh`.
Found with `grep -rn 'run-onair-twin-ota' --include=*.sh --include=*.md .`.

## Prior art

`scripts/run-onair-ic9700-ft991a.sh` already has `A_/B_FREQ_OFFSET_HZ` (lines 47–50) and copies binaries
from A to B (`transfer_binaries_a_to_b`). The new code keeps the same names, meanings and layout.
`grep -n 'FREQ_OFFSET\|transfer_binaries' scripts/run-onair-*.sh`.

## Twins

`run-onair-ic9700-ft991a.sh` has the same deploy gaps as findings 2 and 3: no architecture check, and no
hash check after its transfer. Recorded here and not changed: that runner has worked for this pair
(task 6), and this change does not touch it. UNCHECKED: whether `run-onair-tests.sh` has the same gaps.

## Disposition

- **1, taken.** `deploy` copies `RUN_FILES`: the daemon, the CLI and `scripts/twin-traffic.sh`. `run` hashes
  all three on both stations and refuses if any is missing or differs. The harness's B now starts empty.
- **2, taken.** `deploy` and `setup` stop when a build fails, and `deploy` copies nothing.
- **3, taken.** A daemon that does not start stops both stations and ends the run. `deploy` refuses when
  `uname -m` differs.
- **4, taken.** The message says to copy A's build with `deploy`, and that two builds need not match.
- **5, taken.** The twin runner falls back to `TEST_MODE_RIG_A/B`, `TEST_MODE_EXPECT_A/B` and
  `BAND2M_MIN/MAX_HZ`.
- **6, no change.** Keeping VFO B on the test frequency stays the FT-818 mitigation (setup guide).
- **7, taken** with finding 1.

Re-run against the fake harness after the fixes (B starts with no checkout):
- `run` before `deploy` refuses (rc 1).
- A failed build copies nothing (rc 1).
- `deploy` copies the three files (rc 0).
- `TRAFFIC_FROM=B` polls both daemons (rc 0).
- A daemon that does not start on B stops both (rc 1).
- An `armv7l` B refuses the deploy (rc 1).
- The FT-818 names leave the mode alone and check it (rc 0).

Not run on air.
