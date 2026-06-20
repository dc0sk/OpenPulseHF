# openpulse-linksim

Two-station bidirectional ARQ **link simulator**. It proves the *effective two-way
transfer rate* a station actually achieves under simulated noise / fading — not the raw
modem rate — by running a realistic half-duplex exchange between two stations and
accounting for everything that eats into goodput.

## What it models

- **Station A → B (forward):** real data frames at the current speed level, modulated by a
  real `ModemEngine` and routed through a forward `ChannelModel` realization.
- **Station B → A (reverse):** B decodes, estimates the per-frame SNR, and returns a real
  FSK4 **ACK frame** (`AckOk` / `AckUp` / `AckDown` / `Nack`) through an independent reverse
  channel. A lost/garbled ACK is treated as an implicit NACK.
- **Over-the-air rate adaptation:** A steps the speed level up/down the chosen
  `SessionProfile` ladder from the ACKs (mirroring `RateAdapter` policy, bounded to the
  profile's defined levels).
- **Goodput accounting:** forward air time + ACK air time + turnaround × 2, summed over
  every attempt (including retransmissions). `effective_bps = delivered_bits / total_air`.

This captures the real costs — ACK overhead, half-duplex turnaround, retransmits, and the
cold-start climb up the ladder — that make the effective two-way rate far lower than the
forward mode's gross rate.

## CLI

```bash
# Single run
cargo run -p openpulse-linksim -- --profile hpx_hf --channel awgn --snr 15 --fec rs

# SNR sweep (start:stop:step dB) → effective-rate table
cargo run -p openpulse-linksim -- --profile hpx500 --channel awgn --sweep 0:24:4 --frames 30

# Watterson fading, JSON output
cargo run -p openpulse-linksim -- --profile hpx_hf --channel watterson-moderate --snr 18 --json
```

Channels: `clean`, `awgn`, `watterson-good`, `watterson-moderate`, `watterson-poor`,
`gilbert-elliott`. FEC: `none`, `rs`, `rs-strong`, `soft`.

## Library

`run_link(&LinkParams) -> LinkResult` is the headless core (no audio hardware). See
`LinkParams` / `LinkResult` / `ChannelSpec`. A live egui visualization is planned on top of
this same engine.
