# Full-tier test matrix — characterization snapshot

This directory is the **full-tier** (`--full`) run of `openpulse-testmatrix`: every mode × every
propagation channel (Watterson, Gilbert-Elliott, QRN/QRM/QSB/chirp) × every FEC mode × multiple
payload sizes. It is **characterization data, not a pass/fail gate.**

The committed gate is [`../latest`](../latest) — the **quick tier** (555/555, 0 failed). The full
tier *deliberately sweeps sub-viable conditions* (deep fades, low-SNR AWGN, no-FEC at the SNR floor),
so a large failure count is expected and is the point: it maps where each waveform stops working.

## How to read it

- `summary.md` — totals and metadata.
- `by-channel.md`, `by-mode.md`, `by-usecase.md` — pass/fail breakdowns.
- `results.csv` / `raw.json` — every case.

## Expected-failure classes (not regressions)

- **HF fading / burst** (Watterson, Gilbert-Elliott) and **low-SNR AWGN (≤8 dB)** — the bulk; modes
  legitimately fail below their viable SNR.
- **QRN / QRM / QSB / chirp** — impairment sweeps past the viable margin.
- **OFDM52 no-FEC at the SNR floor / large payload** — a padded wideband mode with no FEC has
  nonzero residual BER; it runs FEC-protected in practice.
- **Dense multicarrier padded-framing limitation** — SCFDMA52-16/32/64QAM and OFDM52-HOM decode fail
  in the `raw_modem` runner at *every* SNR (incl. clean) with any FEC, because the padded demod byte
  count doesn't round-trip the runner's length-prefix/255-byte-RS framing (the demod slices a padded
  block → `length prefix … exceeds available bits`). This is a **matrix-harness framing limitation,
  not a decode-capability failure**: the modes' real performance (e.g. the SCFDMA52-32QAM 2D-Gray
  remap that dropped its floor 17→9 dB) is validated in `crates/openpulse-modem/tests/snr_floor_calibration.rs`,
  which uses the mode's own framing. SCFDMA52-32QAM/64QAM are 0/48 and 0/45 here — unchanged run over run.
- **B2F at 0 dB AWGN** — BPSK250 can't decode, so the driver fast-fails on its socket timeout
  (`os error 11`) instead of hanging.

**Suspect zone (Clean channel + AWGN ≥ 20 dB): 0 failures** in this run (commit `82cfbc5`, 3480/6022
passed) — no real bugs, no regression. Every failure is an expected sub-viable sweep or the
dense-multicarrier framing limitation above (whose cases carry no SNR, so they're outside the
suspect zone). The RS-family OFDM52 padded-framing cases are excluded at the case-generator
(`OFDM_RAW_FRAMING_ONLY`).

Regenerate: `cargo run --release -p openpulse-testmatrix --no-default-features -- --full --output docs/test-reports/full`
(the runner writes into a `latest/` subdir; this snapshot is flattened one level up).
