# Task 22 Part C, run f2b — B sends, `fast` profile, 600 s, interval 8 s, B in the hiss state
- rpi51 commit `14aa9057` (build `e5ba27f`; hashes as in task 22 Part A). AGC FAST on both rigs, FT-991A SQL 0, PREAMP 20, A's drive 0.15. `DAEMON_LOG_LEVEL=debug`.
- Tag `f2b` (`f2` is not used by task 21, but pairs with `f1b`). PTT/ALC watcher (1 s polling): `ptt-alc-watch.txt`; no keying stretch over 70 s. No RSPdx capture.
- Receive audio (8 kHz): rx-audio-2026-11-01/rx-{a,b}-f2b-8k.wav.gz, 1800 s each, started 19:31:58.69 (rpi51) / 19:31:59.05 (station B).
