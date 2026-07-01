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
- `results.csv` / `raw.json` — every case. The CSV `result` column is a self-documenting
  `pass` / `fail` / `skip` string (it used to be a pair of `1`/`0` `passed`,`skipped` columns, which
  invited misreading in ad-hoc analysis).

## Where the 2542 failures are (this run: 3480/6022)

The failure count is dominated by **fading and burst** channels — the whole point of the sweep:

| channel family | failures | note |
|---|---|---|
| Watterson (ionospheric fading) | 1427 | good_f1 → extreme; most modes fail past their Doppler/delay margin |
| Gilbert-Elliott (burst) | 806 | light → severe burst errors |
| AWGN (low SNR) | 163 | concentrated at ≤ 5 dB (0 dB: 67/121, 5 dB: 34/121); ≥ 10 dB ≈ all pass |
| QRN / QRM / QSB / chirp | 146 | impulse/tone/fade/sweeper impairment sweeps |

At **benign conditions (clean + AWGN ≥ 20 dB) the pass rate is 956/957 (99.9 %)** — the lone exception
is OFDM52 no-FEC at AWGN 20 dB / 223 B (a marginal no-FEC-at-large-payload case; passes at ≥ 25 dB).
So the ~2542 failures are the expected map of *where each waveform stops working*, not defects.

## Expected-failure classes (not regressions)

- **HF fading / burst** (Watterson, Gilbert-Elliott) and **low-SNR AWGN (≤8 dB)** — the bulk; modes
  legitimately fail below their viable SNR.
- **QRN / QRM / QSB / chirp** — impairment sweeps past the viable margin.
- **OFDM52 no-FEC at the SNR floor / large payload** — a padded wideband mode with no FEC has
  nonzero residual BER; it runs FEC-protected in practice.
- **Dense multicarrier on HF fading** — the SCFDMA52-QAM rungs (16/32/64QAM) and OFDM52-HOM pass at
  benign conditions (clean / high-SNR AWGN) but fail on Watterson/Gilbert-Elliott fading and marginal
  SNR: high-order SC-FDMA QAM is HF-fading-unsuitable *by design* (they are good-conditions top rungs;
  see `plugins/scfdma/tests/pilot_channel_estimation.rs::scfdma_qam_modes_unsuitable_for_hf_watterson_profiles`).
  Pass rates this run: OFDM52 7/21, SCFDMA52-8PSK 26/54, -16QAM 17/51, -32QAM 19/48, -64QAM 10/45 —
  all decode cleanly at good conditions and drop off as the channel degrades, which is the point of
  the sweep. (The SCFDMA52-32QAM 2D-Gray remap, PR #616, dropped its AWGN floor 17→9 dB.)
- **B2F at 0 dB AWGN** — BPSK250 can't decode, so the driver fast-fails on its socket timeout
  (`os error 11`) instead of hanging.

**Suspect zone (Clean channel + AWGN ≥ 20 dB): 0 failures** in this run (commit `82cfbc5`, 3480/6022
passed) — no real bugs, no regression. Every failure is an expected sub-viable-condition sweep (deep
fades, burst errors, low SNR). The RS-family OFDM52 padded-framing cases are excluded at the
case-generator (`OFDM_RAW_FRAMING_ONLY`).

Regenerate: `cargo run --release -p openpulse-testmatrix --no-default-features -- --full --output docs/test-reports/full`
(the runner writes into a `latest/` subdir; this snapshot is flattened one level up).
