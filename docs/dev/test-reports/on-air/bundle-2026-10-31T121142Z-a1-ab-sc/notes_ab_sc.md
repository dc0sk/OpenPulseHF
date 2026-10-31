# A1 run 3 of 6 — A -> B (IC-9700 transmits ~1.2 W, FT-991A receives), BPSK250|soft_concatenated|64 (2026-10-31)
- Result: FAIL. Commit 91cf0922 (rpi51). Binaries, identical on both stations:
  - openpulse     52ce8717b53ffb5b93f4738554e9e71b8a5a1f52f64fe4c877d7b64c6072c1a3
  - openpulse-tnc 94abc4b64cb1bcc8734780e492523357490a8d1b6582c006e2e40adc30afbd2e
- FT-991A read-back (Part A): EX065+1500; MD0C; SH016; FA144641564; RFPOWER 0.05; PTT 0. IC-9700: RFPOWER 0.0117647, AGC 3 (SLOW), 144640000 Hz PKTUSB.
- RSPdx capture (kept on rpi51, not committed): ~/onair-captures/2026-10-31/a1-ab-sc.cf32, 230400000 bytes, sha256 1557195897f752a3679896f844db145363d9cc4ef2933fd33163eb7b3be3e56d, LO 144.590 MHz, 192 kS/s, RFGR 22, IFGR 40. captured 150.00s peak=0.0494 rms=0.0112 clip=0.00% OK
- Burst: t = 75.75-94.0 s, peak 51542.7 Hz above the LO, 54.1 dB over the floor, -20 dB span 51343 51721 Hz. SDR decode: not decoded (witness only).
