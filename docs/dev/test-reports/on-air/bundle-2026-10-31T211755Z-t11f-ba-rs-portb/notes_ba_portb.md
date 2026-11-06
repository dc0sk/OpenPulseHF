# Task 11 Part F — B -> A, BPSK250|rs|64, RSPdx port B witness (2026-10-31)
- Result: PASS (FT-991A 5 W transmits, IC-9700 receives with --ptt none). Binaries as in task 7 (openpulse 52ce8717...); FT-991A EX065+1500, dial 144641564, RFPOWER 0.05; IC-9700 RFPOWER 0.0117647.
- RSPdx capture on port B (SRH-771, not telescopic, no filter mentioned), kept on rpi51, not committed: ~/onair-captures/2026-10-31/a1-ba-rs-portb.cf32, 75000000 bytes, sha256 9f2633217ccb3cee3f130ab2728d19908306ed3e5f8f367ea7248966157d48d0, LO 144.620 MHz, 62.5 kS/s, RFGR 5, IFGR 40. captured 150.00s peak=0.0539 rms=0.0083 clip=0.00% OK.
- No burst visible: no 0.25 s block in 21.0-22.0 kHz more than 6 dB above its own median ratio (maximum 2.2 dB over the median).
