# OpenPulse

> Transmit data via HF – a plugin-based software modem written in Rust.

[![CI](https://github.com/dc0sk/OpenPulse/actions/workflows/ci.yml/badge.svg)](https://github.com/dc0sk/OpenPulse/actions/workflows/ci.yml)

## Overview

OpenPulse is a cross-platform software modem for sending and receiving data
over amateur radio (HF / VHF) via a soundcard.  It is inspired by established
HF digital modes such as
[VARA](https://rosmodem.wordpress.com/),
[PACTOR](https://en.wikipedia.org/wiki/PACTOR), and
[ARDOP](https://de.wikipedia.org/wiki/ARDOP).

### Architecture

The project is a **Cargo workspace** with separate crates for each concern:

| Crate | Role |
|-------|------|
| `crates/openpulse-core`  | Core traits (`ModulationPlugin`, `AudioBackend`), frame format, CRC-16, plugin registry |
| `crates/openpulse-audio` | Audio backend implementations: in-process loopback (testing) and `cpal` (ALSA / PipeWire / CoreAudio / WASAPI) |
| `crates/openpulse-modem` | Modem engine – wires plugins and audio together |
| `crates/openpulse-cli`   | `openpulse` binary (CLI via `clap`) |
| `plugins/bpsk`           | BPSK modulation plugin (NRZI, raised-cosine pulse shaping) |

Future plugins (QPSK, ARDOP-compatible modes, …) implement the
`ModulationPlugin` trait and are registered at startup – no recompilation of
the core is required.

### Supported modulation modes

| Mode      | Baud rate | Notes |
|-----------|-----------|-------|
| `BPSK31`  |  31.25    | Narrow-band HF, inspired by PSK31 |
| `BPSK63`  |  62.5     | Twice the throughput of BPSK31 |
| `BPSK100` | 100       | Convenient for loopback / testing |
| `BPSK250` | 250       | Wide-band / VHF |

## Platform support

| Platform | Audio backend |
|----------|---------------|
| Linux    | ALSA, PipeWire (via ALSA compat layer) – **primary target** |
| macOS    | CoreAudio |
| Windows  | WASAPI |
| Any      | In-process loopback (no hardware required, used for tests) |

## Build

```sh
# Linux – install ALSA development headers first
sudo apt-get install libasound2-dev

cargo build --release
```

## Usage

```sh
# Transmit a string (loopback – no hardware needed)
openpulse --backend loopback transmit "CQ CQ DE DC0SK" --mode BPSK100

# Receive from the default soundcard
openpulse receive --mode BPSK31

# List audio devices
openpulse devices

# List registered modulation modes
openpulse modes
```

## Testing

```sh
# Run all tests (loopback backend – no audio hardware required)
cargo test --workspace --no-default-features

# Run with full audio support (requires ALSA headers on Linux)
cargo test --workspace
```

## Frame format

```
┌────────┬─────────┬──────────────────┬─────────────┬─────────┬───────────┐
│ magic  │ version │ sequence (16-bit) │ length (8b) │ payload │ CRC-16    │
│ "OPLS" │  0x01   │     big-endian    │  0–255 B    │         │ CRC-CCITT │
└────────┴─────────┴──────────────────┴─────────────┴─────────┴───────────┘
```

## Roadmap

- [ ] QPSK modes (2× spectral efficiency)
- [ ] Reed–Solomon / convolutional FEC
- [ ] Bandwidth-adaptive rate control
- [ ] ARDOP-compatible mode plugin
- [ ] TUI frontend (ratatui)
- [ ] GUI frontend (iced)

## License

GNU General Public License v3.0 or later – see [LICENSE](LICENSE).
