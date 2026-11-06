# Task 22 Part C, run f1b — A sends, `fast` profile, 600 s, interval 8 s, B in the hiss state
- rpi51 commit `6c2e0295` (build `e5ba27f`; hashes as in task 22 Part A). AGC FAST on both rigs, FT-991A SQL 0, PREAMP 20, A's drive 0.15. `DAEMON_LOG_LEVEL=debug` (the task said `info,ofdm_plugin=debug`; I used full debug so that the flush and decode times can be read).
- Tag `f1b` because the tag `f1` is task 21's run. PTT/ALC watcher (1 s polling): `ptt-alc-watch.txt`; no keying stretch over 70 s. No RSPdx capture.
- Receive audio (8 kHz): rx-audio-2026-11-01/rx-{a,b}-f1b-8k.wav.gz, 1800 s each, started 18:40:52.30 (rpi51) / 18:40:52.67 (station B).
