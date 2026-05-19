---
project: openpulsehf
doc: README.md
status: living
last_updated: 2026-05-19
---

# OpenPulseHF

A plugin-based HF/VHF/UHF software modem and protocol stack written in Rust.

OpenPulseHF is a multi-crate Rust workspace providing a full digital communications
stack — from DSP primitives and adaptive rate profiles through ARDOP/KISS TNC
interfaces, B2F/Winlink protocol support, QSY frequency agility, mesh networking,
and post-quantum key exchange. The plugin architecture lets modulation modes be
added without touching the core modem engine. All tests run against a deterministic
loopback backend; no audio hardware is required to build or test.

[![CI](https://github.com/dc0sk/OpenPulseHF/actions/workflows/ci.yml/badge.svg)](https://github.com/dc0sk/OpenPulseHF/actions/workflows/ci.yml)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

**Author:** Simon Keimer · [DC0SK](https://github.com/dc0sk)

---

## Feature highlights

- Six adaptive rate profiles spanning BPSK31 through 64QAM2000-RRC and OFDM52
- ARQ retry loop with soft LLR accumulation across retransmissions
- Rate adaptation driven by ACK/NACK feedback and per-level SNR floor/ceiling gates
- Watterson HF fading and Gilbert-Elliott burst-error channel simulation
- DCD energy threshold and 0.3-persistence CSMA channel access
- AFC offset estimation wired into BPSK demodulator
- Soft-decision (max-log-MAP) demodulators for 8PSK, SC-FDMA QAM, and 64QAM
- DFT-CE pilot-aided channel estimation in SC-FDMA (MMSE equalization)
- LMS/DFE adaptive equalizer on the BPSK-RRC demodulation path
- Reed-Solomon + convolutional block interleaving FEC
- LDPC rate-1/2 min-sum belief propagation
- ARDOP-compatible TCP TNC (`openpulse-tnc`) with Pat-compatible command set
- KISS/AX.25 TNC (`openpulse-kisstnc`)
- B2F/Winlink state machine: ISS and IRS roles, Gzip (Type D) and LZHUF (Type C)
  compression; `queue_message_type_c` for Winlink-wire-compatible ISS sends
- Direct TCP Winlink CMS gateway (`openpulse-gateway`)
- QSY frequency-agility wire codec and session state machine with Ed25519 signing
- Mesh broadcast daemon with TTL-limited re-broadcast
- Configurable relay/digipeater node with trust-policy filtering
- Post-quantum in-band handshake: ML-DSA-44 signing, ML-KEM-768 key encapsulation,
  Hybrid (Ed25519 + ML-DSA-44) mode
- Ed25519 transfer manifest signing and verification
- Multi-hop relay forwarding with trust-weighted path scoring
- Peer descriptor cache with signed peer identity and query propagation
- Typed TOML configuration with CLI-override precedence
- Optional GPU-accelerated BPSK DSP kernels via wgpu (CPU fallback)
- egui/eframe signal-path testbench: waterfall, spectrum, scatter, 7 channel models
- ratatui TUI with HPX state, AFC/rate meters, DCD energy bar, transitions log
- PKI tooling: Ed25519 trust-bundle signing service with PostgreSQL persistence

On-air regulatory validation has not been completed. All tests use loopback and
simulated-channel paths only.

---

## Profiles table

| Profile | SL range | Initial | Top mode |
|---|---|---|---|
| `hpx_hf` | SL2–SL8 | SL2 | SCFDMA52-8PSK |
| `hpx_narrowband` | SL8–SL11 | SL8 | 8PSK2000-RRC |
| `hpx_wideband` | SL8–SL11 | SL8 | 8PSK1000 |
| `hpx_ofdm_hf` | SL5–SL6 | SL5 | OFDM52 |
| `hpx_narrowband_hd` | SL8–SL9 | SL8 | 8PSK9600-RRC |
| `hpx_wideband_hd` | SL12–SL15 | SL12 | 64QAM2000-RRC |

`hpx_wideband_hd` is intended for VHF/UHF FM, microwave, and satellite links where
SNR margins of 16–40 dB are achievable. It is not suitable for HF ionospheric paths.

---

## Quick start

```bash
# Build (requires libasound2-dev on Linux for the CPAL audio feature)
cargo build --workspace

# Test suite — no audio hardware required
cargo test --workspace --no-default-features

# Lint
cargo clippy --workspace --no-default-features -- -D warnings
cargo fmt --all -- --check

# Benchmark regression gate
cargo run -p openpulse-cli --no-default-features -- --backend loopback --log error benchmark run

# Automated mode × channel test matrix
cargo run -p openpulse-testmatrix --no-default-features
```

The `--no-default-features` flag disables the CPAL audio backend. All tests must
pass with this flag. Never add tests that require real audio hardware.

---

## Repository layout

### Core layer

| Crate | Role |
|---|---|
| `crates/openpulse-core` | Frame format, CRC-16, FEC (RS+Conv+LDPC), HPX session state machine, plugin registry, trust/signing, SAR, ACK, rate adaptation, relay, query propagation, peer cache, compression, PQ handshake |
| `crates/openpulse-audio` | LoopbackBackend (testing) and CpalBackend (hardware, feature-gated) |
| `crates/openpulse-modem` | ModemEngine, PipelineScheduler, ArqSession, benchmark harness, CSMA/DCD, channel sim harness |
| `crates/openpulse-channel` | Channel simulation: Watterson, Gilbert-Elliott, QRN/QRM/QSB/Chirp |
| `crates/openpulse-radio` | PttController trait: NoOp, SerialRtsDtr, Vox, Rigctld |
| `crates/openpulse-dsp` | RRC filter, PLL, Gardner timing recovery, LMS/DFE adaptive equalizer |
| `crates/openpulse-config` | Typed TOML configuration with CLI-override pattern |
| `crates/openpulse-gpu` | wgpu-backed BPSK DSP kernels; CPU fallback; `gpu` feature |

### Protocol layer

| Crate | Role |
|---|---|
| `crates/openpulse-ardop` | ARDOP-compatible TCP TNC; `openpulse-tnc` binary |
| `crates/openpulse-kiss` | KISS/AX.25 TNC; `openpulse-kisstnc` binary |
| `crates/openpulse-b2f` | B2F/Winlink state machine: FC/FS/Ff/Fq frames, Gzip (Type D), LZHUF (Type C) |
| `crates/openpulse-b2f-driver` | High-level ISS/IRS session driver over ARDOP TCP |
| `crates/openpulse-gateway` | Direct TCP Winlink CMS gateway; `openpulse-gateway` binary |
| `crates/openpulse-qsy` | QSY frequency-agility: wire codec, Ed25519 signing, QsySession, QsyScanner |
| `crates/openpulse-mesh` | Mesh broadcast daemon with TTL-limited re-broadcast |
| `crates/openpulse-repeater` | Digipeater / relay node with trust-policy filtering |
| `crates/openpulse-daemon` | Unified background daemon: modem, PTT, control-protocol services |

### UI and tooling

| Crate | Role |
|---|---|
| `crates/openpulse-cli` | CLI binary |
| `crates/openpulse-tui` | ratatui TUI frontend |
| `apps/openpulse-testbench` | egui signal-path testbench: waterfall, spectrum, scatter, 7 channel models |
| `apps/openpulse-panel` | Operator panel GUI connecting to openpulse-daemon |
| `apps/openpulse-testmatrix` | Automated mode × channel test matrix runner |
| `pki-tooling` | Ed25519 trust-bundle signing service with PostgreSQL persistence |

### Plugins

| Crate | Modes |
|---|---|
| `plugins/bpsk` | BPSK31, BPSK63, BPSK100, BPSK250; optional GPU path; LMS equalizer on RRC path |
| `plugins/qpsk` | QPSK125–QPSK9600, QPSK2000-RRC, QPSK9600-RRC |
| `plugins/psk8` | 8PSK500–8PSK9600, 8PSK2000-RRC, 8PSK9600-RRC; max-log-MAP soft demodulator |
| `plugins/64qam` | 64QAM500, 64QAM1000, 64QAM2000-RRC; Gray-coded; soft demodulator |
| `plugins/scfdma` | SCFDMA52-8PSK, SCFDMA52-16QAM, SCFDMA52-32QAM, SCFDMA52-64QAM; DFT-CE, MMSE |
| `plugins/ofdm` | OFDM16, OFDM52; LS estimate + ZF channel equalization |
| `plugins/fsk4` | FSK4-ACK (ACK channel, 100 baud, Goertzel demodulator) |

---

## License

GNU General Public License v3.0 or later — see [LICENSE](LICENSE).
