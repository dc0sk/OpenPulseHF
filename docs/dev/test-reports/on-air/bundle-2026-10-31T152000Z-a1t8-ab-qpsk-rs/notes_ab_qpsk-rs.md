# A1 run 1 of 4 — A -> B (IC-9700 transmits ~1.2 W, FT-991A receives), QPSK250-D|rs|64 (2026-10-31)
- Result: FAIL. Commit 91cf0922 (rpi51). Binaries, identical on both stations:
  - openpulse     52ce8717b53ffb5b93f4738554e9e71b8a5a1f52f64fe4c877d7b64c6072c1a3
  - openpulse-tnc 94abc4b64cb1bcc8734780e492523357490a8d1b6582c006e2e40adc30afbd2e
- FT-991A read-back (Part A): EX065+1500; MD0C; SH016; FA144641564; RFPOWER 0.05; PTT 0. IC-9700: RFPOWER 0.0117647, AGC 3 (SLOW), 144640000 Hz PKTUSB.
- RSPdx capture (kept on rpi51, not committed): ~/onair-captures/2026-10-31/a1-ab-qpsk-rs.cf32, 230400000 bytes, sha256 4cf469f816eb21145c648ca07d74c1cb229a9405d18428ac495c7d057c58bcf4, LO 144.590 MHz, 192 kS/s, RFGR 22, IFGR 40. captured 150.00s peak=0.1955 rms=0.0054 clip=0.00% OK
- Burst: t = 75.25-80.75 s, peak 51499.9 Hz above the LO, 53.1 dB over the floor, -20 dB span 51344 51735 Hz. SDR decode: not decoded (witness only).
