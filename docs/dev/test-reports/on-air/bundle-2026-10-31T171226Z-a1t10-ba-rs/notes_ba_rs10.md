# Task 10 Part A — B -> A, BPSK250|rs|64, RSPdx at full gain (2026-10-31)
- Result: PASS (FT-991A transmits 5 W, IC-9700 receives). Commit 91cf0922 binaries as in task 7 (sha256 52ce8717..., 94abc4b6...).
- FT-991A: EX065+1500; dial 144641564; RFPOWER 0.05. IC-9700: RFPOWER 0.0117647, PTT 0, receiver --ptt none.
- RSPdx capture (kept on rpi51, not committed): ~/onair-captures/2026-10-31/a1-ba-rs10.cf32, 230400000 bytes, sha256 93a4a7c608c77fd2d3231b43ad68c9cd1fc6628cc8a4cbae7e917e3e8c6d3b61, LO 144.590 MHz, 192 kS/s, RFGR 0, IFGR 30. captured 150.00s peak=1.0818 rms=0.6129 clip=0.96% OVERLOAD.
- No burst visible: max 2.9 dB over the median 49-54 kHz floor. Strongest spectral line at 0 Hz (+26 dB over the median; DC), broadband energy saturates the front end.
