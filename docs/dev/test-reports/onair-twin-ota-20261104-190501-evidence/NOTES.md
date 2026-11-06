# Task 22 Part B, run a2h — A sends, robust, 900 s, interval 8 s; B's preamp 20 → 0 → 20, then A's drive 0.01 / 0.005 / 0.002 / 0.15
- rpi51 commit `22ca4365` (build `e5ba27f`; hashes as in task 22 Part A). AGC FAST on both rigs, FT-991A SQL 0 (hiss state), PREAMP 20.
- Traffic started 18:05:02.12 UTC (t0 = the runner's "Driving OTA traffic" line); window end 18:20:02. First SL6 decode 18:09:53.16 (t0+291 s). Schedule in `drive-schedule.txt`: PREAMP 0 from 18:13:02.53 (t0+480 s), 20 from 18:14:35.79 (read back 0 and 20); A_TX_SINK_VOLUME 0.01 from 18:16:09.02, 0.005 from 18:17:25.11, 0.002 from 18:18:44.19, 0.15 from 18:20:00.17.
- ATT was not used: the FT-991A rejects it on 144.64 MHz.
- PTT/ALC watcher (1 s polling, both rigctlds): `ptt-alc-watch.txt`. No keying stretch over 70 s.
- RSPdx witness, port A, LO 144.620 MHz, 62.5 kS/s, RFGR 22 / IFGR 40, 960 s from 18:05:02.1: `~/onair-captures/2026-10-31/a2h.cf32` (480000000 bytes, not committed), sha256 `3a39dc159aa9503382fb1ff5460774cb357ed8fb2f37af7da7617d163e02f0a5`, `peak=0.0170 rms=0.0100 clip=0.00%`.
- Receive audio (8 kHz): rx-audio-2026-11-01/rx-{a,b}-a2h-8k.wav.gz, 1950 s each, started 18:04:12.99 (rpi51) / 18:04:13.36 (station B).
