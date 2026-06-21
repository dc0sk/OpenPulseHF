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

## GUI (live visualizer)

```bash
cargo run -p openpulse-linksim --features gui --bin openpulse-linksim-gui
```

Side-by-side **Station A | Channel | Station B**: A's clean data TX (left), the noisy
on-air signal (middle), and B's FSK4 ACK response (right) — each with a live spectrum +
waterfall. The toolbar selects the profile / channel / FEC and an **SNR slider that adjusts
the channel live**, so you can watch the ladder adapt and the bottom plot track the
effective two-way transfer rate over time. Runs continuously on a background thread over the
same `LinkSim` engine.

## Serve the operator panel (`openpulse-panel`)

The simulator can speak the **`openpulse-daemon` control protocol**, so an *unmodified*
`openpulse-panel` connects to it exactly as it would to a real station — no daemon, modem,
or audio hardware required. It emits the same NDJSON `ControlEvent` stream interleaved with
binary `OPSP` spectrum frames a real daemon does (speed-level ladder, HPX session state,
effective-bps / compression / signal metrics, and the on-air waterfall). The panel's
`Effective` reading uses the same definition as the linksim GUI (Net × compression × frame
success), so the two windows agree.

The panel is a **monitor** here: operator-only controls (messages, QSY, rig CAT, PTT,
RF-connect) are inert (no real peer), and its mode selector has no effect because the link is
adaptive — the **profile / ladder is driven from the linksim GUI** (or the `--profile` flag in
headless mode), and the sim steps the mode up and down on its own. The panel has no profile
concept because the daemon control protocol exposes modes, not profiles.

There are two ways to serve it:

**A. Both windows from one simulation (the demo).** Launch the linksim GUI with `--serve`:
the GUI window *and* the panel are driven by the **same** live `LinkSim`, so the GUI's SNR
slider / profile / FEC controls drive the panel too, in lock-step. A `● panel ×N` indicator
in the GUI toolbar shows connected panels.

```bash
# Terminal 1 — the GUI window, also serving the panel
cargo run -p openpulse-linksim --features "gui serve" --bin openpulse-linksim-gui -- --serve 127.0.0.1:9000

# Terminal 2 — the operator panel, fed by the same sim
cargo run -p openpulse-panel        # Server: 127.0.0.1:9000 → Connect
```

**B. Headless server (no GUI).** Run the CLI as a fake daemon; `--serve-fps` paces the
waterfall scroll. Useful when you only want the panel.

```bash
cargo run -p openpulse-linksim --features serve -- \
    --serve 127.0.0.1:9000 --profile hpx_hf --channel awgn --snr 12
```

## Library

`LinkSim` is a step-able simulation (`new` / `step` → `FrameStep` / `set_conditions` /
`result`); `run_link(&LinkParams) -> LinkResult` runs one to completion. Headless, no audio
hardware. See `LinkParams` / `LinkResult` / `FrameStep` / `ChannelSpec`.
