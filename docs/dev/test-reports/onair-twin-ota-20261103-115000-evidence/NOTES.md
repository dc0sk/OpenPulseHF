# Task 20 Part C, run a2f — A sends, robust, 900 s, stepped drive schedule on A_TX_SINK_VOLUME
- rpi51 commit `13029c11` (build `f18c8a6`, same binaries and hashes as a2e, identical on both stations).
- Traffic started 10:50:00.83 UTC (t0 = the runner's "Driving OTA traffic" line); window end 11:05:00.8. First SL6 decode 10:54:52.10 (t0+292 s). Drive schedule (UTC): 0.15 until 10:58:01.87 (t0+481 s), 0.08 from 10:58:01.87, 0.05 from 10:59:04.50, 0.035 from 11:00:37.54, 0.15 from 11:02:10.60 to the end (drive-schedule.txt).
- RSPdx witness, port A, LO 144.620 MHz, 62.5 kS/s, RFGR 22 / IFGR 40, 960 s from 10:50:00.8: `~/onair-captures/2026-10-31/a2f.cf32` (480000000 bytes, not committed), sha256 `824154105bcde28a0764725b78ea2a4110f530bf6c2b75e26e6b725f28eeda4b`, `peak=0.0139 rms=0.0084 clip=0.00%`. Not analysed.
- Receive audio (8 kHz): rx-audio-2026-11-01/rx-{a,b}-a2f-8k.wav.gz, 1950 s each, started 10:49:12.72 (rpi51) / 10:49:13.09 (station B).
