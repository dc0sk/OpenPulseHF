---
project: openpulsehf
doc: docs/dev/onair-tasks.md
status: review
last_updated: 2026-10-26
---

# On-air task channel (cloud session ↔ station session on rpi51)

The cloud session cannot reach the session on rpi51, so tasks and results travel on the branch
`claude/affectionate-brahmagupta-sdn8qg`. The cloud session writes the next task here and pushes. The
station session pulls, works the **first task whose status is `open`**, appends its results under that
task, sets the status to `done` (or `blocked`, saying why), commits and pushes. Then the maintainer
tells the cloud session, which pulls, reads the results and writes the next task.

Rules for the station session, every task:
- Work only the open task. Never key a transmitter unless the task says so, and then only after
  asking the maintainer in that session first.
- The station safety rules hold throughout: the FT-818's power is never increased; its FRONT antenna
  stays selected; the mode is checked before keying.
- Paste raw command output into the results (fenced blocks); do not summarise it away. Results that
  contain secrets (keys, passwords) are redacted.
- Commit only this file and session records; push to `claude/affectionate-brahmagupta-sdn8qg`.

---

## Task 1 — station readiness, read-only (status: open)

Nothing in this task transmits. Both rigs should be **on** (CAT and the idle level need them).

1. Report `hostname`, `git rev-parse --short HEAD` and the branch. Bring this checkout to the latest
   `origin/claude/affectionate-brahmagupta-sdn8qg` (fast-forward only), and the same on rpi53
   (`ssh dc0sk@dc0sk-rpi53 'cd ~/git/OpenPulseHF && git fetch && git checkout claude/affectionate-brahmagupta-sdn8qg && git merge --ff-only'`).
   If either has local changes, stop and report them.
2. Report which of the *Station preparation* steps 1–3 in `docs/dev/onair-ic9700-ft818-setup.md` are
   already done (SSH keys from rpi51 to itself and rpi53 with `ssh -o BatchMode=yes`; packages; the
   SDRplay API service and `SoapySDRUtil --find="driver=sdrplay"`). Do not install anything as root or
   build third-party code: list what is missing for the maintainer.
3. Release builds on both Pis per step 4 of *Station preparation* (cargo only, no sudo).
4. A local profile outside the repo (`~/onair-local/ic9700-ft818.sh`) that sources
   `docs/config/onair-ic9700-ft818.example.sh`. If one exists from 2026-10-26, show its contents and
   make it source the current example. Set `B_CAT_PORT` to rpi53's `-if00-port0` by-id path (never
   `-if01-`).
5. `source ~/onair-local/ic9700-ft818.sh && scripts/onair-setup-audio-routing.sh`, then
   `scripts/onair-station-check.sh` and, with nothing transmitting anywhere,
   `scripts/onair-station-check.sh --levels`. Paste both outputs in full.
6. Ask the maintainer for the MANUAL list the check prints and record the answers.

### Results

_(station session: append here)_
