# Task 19 Part C, run a2d — A sends, robust, 900 s, stepped drive schedule on A_TX_SINK_VOLUME
- rpi51 commit `bbe97add` (build `4fd5a13`, same binaries and hashes as a2c, identical on both stations).
- Traffic started 05:53:20.15 UTC (t0 = the runner's "Driving OTA traffic" line). SL6 was not decoding by t0+480 s, so the schedule started at t0+480: 0.15 until 06:01:20.71, 0.08 from 06:01:20.71, 0.05 from 06:02:22.998, 0.035 from 06:03:55.97, 0.15 from 06:05:28.73 to the end (drive-schedule.txt).
- RSPdx witness, port A, LO 144.620 MHz, 62.5 kS/s, RFGR 22 / IFGR 40, 960 s from 05:53:20.15: `~/onair-captures/2026-10-31/a2d.cf32` (480000000 bytes, not committed), sha256 `649eb5c36eed723606effbe2bb1fbbf475a3d8dfb0a0eeed8aa20111ea3a125c`, `peak=0.0139 rms=0.0086 clip=0.00%`. Not analysed.
- Receive audio (8 kHz): rx-audio-2026-11-01/rx-{a,b}-a2d-8k.wav.gz, 1250 s each, started 05:52:32.03 (rpi51) / 05:52:32.41 (station B).
