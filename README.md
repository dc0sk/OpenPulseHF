---
project: openpulsehf
doc: README.md
status: living
last_updated: 2026-05-17
---

# OpenPulseHF

> Open-source HF digital communications stack for operators who need reliability, adaptability, and modern security.

[![CI](https://github.com/dc0sk/OpenPulseHF/actions/workflows/ci.yml/badge.svg)](https://github.com/dc0sk/OpenPulseHF/actions/workflows/ci.yml)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
[![Donate via PayPal](https://img.shields.io/badge/Donate-PayPal-blue.svg?logo=paypal)](https://www.paypal.com/donate/?hosted_button_id=WY9U4MQ3ZAQWC)

**Author:** Simon Keimer · [DC0SK](https://github.com/dc0sk)

OpenPulseHF is a full-stack HF digital radio modem: modulation plugins, ARQ session management,
Winlink/B2F compatibility, AX.25/KISS bridging, a channel-simulation test harness, and a live
signal-path testbench GUI — all in a single Rust workspace, no external C DSP or codec
dependencies (system audio libraries such as ALSA on Linux or CoreAudio on macOS are required
when building with the `cpal` audio backend).

---

## Why Operators Choose OpenPulseHF

OpenPulseHF focuses on one outcome: keep digital links usable in real HF conditions.

- Adaptive ARQ sessions that react to changing channels in both directions
- Broad mode coverage from robust narrowband up to high-throughput wideband profiles
- Built-in security and trust controls, including post-quantum-capable handshake modes
- Interoperability surfaces for common station workflows (ARDOP, KISS, B2F/Winlink)
- Deterministic lab validation with channel models, test matrix automation, and CI gates

## Core Capabilities

### Modem and Session Stack

- 38 modulation modes across BPSK, QPSK, 8PSK, 64QAM, OFDM, SC-FDMA, and FSK4 ACK
- 20-speed adaptive ladder with HPX session profiles
- Multiple FEC paths (RS, interleaving, concatenated, stronger RS, soft-decision options)
- Compression negotiation (None, LZ4, Zstd dictionary path)
- Multi-hop relay and mesh-ready behavior

### Security and Trust

- Signed handshake and signed manifest workflow
- Ed25519, hybrid, and PQ-capable signing paths (ML-DSA-44)
- Forward-secrecy KEM path (ML-KEM-768)
- Trust policy controls, diagnostics, and trust-store operations

### Operator Tooling

- `openpulse` CLI for operation, diagnostics, trust, and benchmark workflows
- NDJSON monitoring for automation and observability
- TUI and panel frontends
- Loopback-first testing plus optional hardware backend path

## Notable Open-Source Differentiators

To the best of our knowledge, these represent uncommon or first-available capabilities in open-source amateur digital-mode software:

- Post-quantum-capable in-band handshake modes (ML-DSA-44 / ML-KEM-768)
- Collaborative QSY frequency agility with signed negotiation frames
- SC-FDMA waveform family in an open HF stack
- Memory-ARQ sample combining in a production-integrated modem workflow
- Zstd dictionary-compression workflow tailored for short radio payloads

## Compliance and Bandplan Awareness

OpenPulseHF is engineered for compliance-aware operation, not blind transmission.

- Explicit compliance documentation and checklists
- On-air validation plan and deployment checklist
- Bandplan-awareness controls for QSY and transmit workflows
- Guardrails for channel width and segment conventions where configured
- Operator-first model: station license, local regulation, and band rules remain authoritative

Start with:

- [docs/regulatory.md](docs/regulatory.md)
- [docs/regulatory-compliance-checklist.md](docs/regulatory-compliance-checklist.md)
- [docs/on-air_testplan.md](docs/on-air_testplan.md)

## Quick Start

### Prerequisites

```bash
# Linux (Debian/Ubuntu)
sudo apt install libasound2-dev
```

### Build

```bash
cargo build --workspace
```

### Run (loopback, no RF hardware)

```bash
./target/debug/openpulse modes
./target/debug/openpulse devices
./target/debug/openpulse --backend loopback transmit "Hello HF" --mode BPSK100
./target/debug/openpulse --backend loopback receive --mode BPSK100
```

### Validate

```bash
cargo fmt --all -- --check
cargo clippy --workspace --no-default-features -- -D warnings
cargo test --workspace --no-default-features
```

## Documentation

User-facing docs index:

- [docs/README.md](docs/README.md)

Developer and planning docs index:

- [docs/dev/README.md](docs/dev/README.md)

Operator manual:

- [docs/openpulse-manual.md](docs/openpulse-manual.md)

## Contributing

Contributions are welcome. For plugin and engineering workflows, start here:

- [docs/dev/contributing-plugins.md](docs/dev/contributing-plugins.md)

## Support

- Operational feedback, logs, and benchmark reports are valuable contributions.
- Financial support helps sustain development and maintenance:
    - https://www.paypal.com/donate/?hosted_button_id=WY9U4MQ3ZAQWC

## License

GNU General Public License v3.0 or later — see [LICENSE](LICENSE).

For proprietary integration approaches, see:

- [docs/dev/plugin-commercial-interface.md](docs/dev/plugin-commercial-interface.md)
