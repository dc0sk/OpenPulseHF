# Task 21 Part D, run f1 — A sends, `fast` profile, 600 s, TRAFFIC_INTERVAL=8, A's drive at the profile's value, no steps
- rpi51 commit `772429a0` (build `23278b6`; hashes as in task 21 Part A). `DAEMON_LOG_LEVEL=info,ofdm_plugin=debug`: the daemon logs have the OFDM lines at debug, everything else at info (no `fec attempt`, `burst flushed` or `logged TX metadata` lines).
- First keying seen by the PTT/ALC watcher 09:55:33 UTC; watcher polled both rigctlds once a second (`ptt-alc-watch.txt`: time, PTT, ALC while keyed). No keying stretch over 70 s.
- No RSPdx capture for this run (not asked for).
- Receive audio (8 kHz): rx-audio-2026-11-01/rx-{a,b}-f1-8k.wav.gz, 1800 s each, started 09:54:44.18 (rpi51) / 09:54:44.54 (station B).
