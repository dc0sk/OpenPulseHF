# A1 run 1 of 6 — A -> B, BPSK250|none|64 (2026-10-31)
- Direction: IC-9700 transmits (~1.2 W), FT-991A receives (IRS). Case `BPSK250|none|64`. Result: PASS.
- Commit 91cf0922 (rpi51). Binaries, identical on both stations:
  - openpulse     52ce8717b53ffb5b93f4738554e9e71b8a5a1f52f64fe4c877d7b64c6072c1a3
  - openpulse-tnc 94abc4b64cb1bcc8734780e492523357490a8d1b6582c006e2e40adc30afbd2e
- FT-991A read-back (Part A): EX065+1500; MD0C; SH016; FA144641564; RFPOWER 0.05; PTT 0. IC-9700: RFPOWER 0.0117647, AGC 3 (SLOW), 144640000 Hz PKTUSB.
- RSPdx capture (kept on rpi51, not committed): ~/onair-captures/2026-10-31/a1-ab-none.cf32, 230400000 bytes,
  sha256 401caa19315c22934b1c8fcfbeb995fef665b08abb148434e7d761897c9d0272, LO 144.590 MHz, 192 kS/s, RFGR 22, IFGR 40, clip 0.00 %.
- Burst in the capture: t = 75.75-79.75 s, peak 51535 Hz above the LO (expected ~51.5 kHz), 53.5 dB over the median 49-54 kHz floor, -20 dB span 51341-51727 Hz. SDR decode: not decoded (witness only).
