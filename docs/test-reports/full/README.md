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
- **B2F at 0 dB AWGN** — BPSK250 can't decode, so the driver fast-fails on its socket timeout
  (`os error 11`) instead of hanging.

The only Clean / high-SNR (≥20 dB) failure is the OFDM52 no-FEC large-payload case above; the
RS-family OFDM52 padded-framing cases are excluded at the case-generator (`OFDM_RAW_FRAMING_ONLY`).

Regenerate: `cargo run --release -p openpulse-testmatrix --no-default-features -- --full --output docs/test-reports/full`
(the runner writes into a `latest/` subdir; this snapshot is flattened one level up).
