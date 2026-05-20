---
project: openpulsehf
doc: README.md
status: living
last_updated: 2026-05-19
---

# OpenPulseHF

A plugin-based HF/VHF/UHF software modem and protocol stack written in Rust.

OpenPulseHF is a multi-crate Rust workspace providing a full digital communications stack —
from DSP primitives and adaptive rate profiles through ARDOP/KISS TNC interfaces,
B2F/Winlink protocol support, QSY frequency agility, mesh networking, and post-quantum
key exchange. The plugin architecture lets modulation modes be added without touching
the core modem engine. All tests run against a deterministic loopback backend; no audio
hardware is required to build or test.

[![CI](https://github.com/dc0sk/OpenPulseHF/actions/workflows/ci.yml/badge.svg)](https://github.com/dc0sk/OpenPulseHF/actions/workflows/ci.yml)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

**Author:** Simon Keimer · [DC0SK](https://github.com/dc0sk)

---

## Why OpenPulseHF?

Several capabilities here are firsts or near-firsts in open-source amateur digital modes:

| Capability | What makes it different |
|---|---|
| **Post-quantum link security** | ML-DSA-44 signing + ML-KEM-768 key encapsulation negotiated in-band. Hybrid mode signs with both Ed25519 and ML-DSA-44 simultaneously. No other open HF modem does this. |
| **SC-FDMA waveform on HF** | Single-Carrier FDMA (the LTE uplink waveform) brought to HF with DFT-CE pilot-aided channel estimation and MMSE equalization — not OFDM, so PAPR stays low. |
| **64QAM and SCFDMA-64QAM with soft demodulation** | Gray-coded 64QAM with max-log-MAP soft demodulator. Aggressive constellation for VHF/UHF links with proper soft FEC backing. |
| **LDPC belief propagation** | Real rate-1/2 min-sum belief propagation — not a stub. First open-source HF software modem with working LDPC. |
| **LLR-accumulating ARQ** | Soft LLR values accumulate across retransmissions (PACTOR-style Memory-ARQ), turning each retry into a soft combining gain. |
| **GPU-accelerated DSP** | wgpu-backed BPSK DSP kernels with automatic CPU fallback — rare in open-source HF modems. |
| **QSY frequency agility** | Ed25519-signed QSY_REQ/LIST/VOTE/ACK wire protocol. Initiator and responder roles wired into the daemon; rig CAT control via rigctld. |

On-air regulatory validation has not been completed. All tests use loopback and
simulated-channel paths only.

---

## Feature highlights

### Modulation and waveforms

- **20+ registered modes** across 7 plugins: BPSK31–BPSK250, QPSK125–QPSK9600,
  QPSK/8PSK -2000-RRC and -9600-RRC, 8PSK500–8PSK1000, 64QAM500–64QAM2000-RRC,
  SCFDMA52-8PSK/16QAM/32QAM/64QAM, OFDM16/OFDM52, FSK4-ACK
- RRC pulse shaping on all carrier modes; Gardner TED timing recovery
- LMS/DFE adaptive equalizer on BPSK-RRC demodulation path
- DFT-CE pilot-aided channel estimation in SC-FDMA (MMSE equalization)
- AFC offset estimation in BPSK demodulator; offset tracked per session
- Soft-decision (max-log-MAP) demodulators for 8PSK, SC-FDMA QAM, and 64QAM
- Optional GPU-accelerated BPSK DSP kernels via wgpu (automatic CPU fallback)

### Error correction and channel coding

| Layer | Algorithm |
|---|---|
| Reed-Solomon | RS(255, 223) + block interleaver — default for HF burst-error profiles |
| Convolutional | Rate-1/2, K=3, G={7,5}, hard-decision Viterbi — better for AWGN-dominant paths |
| LDPC | Rate-1/2 min-sum belief propagation — highest coding gain |
| ARQ | Soft LLR accumulation across retransmissions; adaptive mode switching per retry |

### Adaptive rate profiles

Six `SessionProfile` mappings from speed levels to modes, driven by ACK/NACK feedback
and per-level SNR floor/ceiling gates:

| Profile | SL range | Initial | Top mode |
|---|---|---|---|
| `hpx_hf` | SL2–SL8 | SL2 | SCFDMA52-8PSK |
| `hpx_narrowband` | SL8–SL11 | SL8 | 8PSK2000-RRC |
| `hpx_wideband` | SL8–SL11 | SL8 | 8PSK1000 |
| `hpx_ofdm_hf` | SL5–SL6 | SL5 | OFDM52 |
| `hpx_narrowband_hd` | SL8–SL9 | SL8 | 8PSK9600-RRC |
| `hpx_wideband_hd` | SL12–SL15 | SL12 | 64QAM2000-RRC |

`hpx_wideband_hd` targets VHF/UHF FM, microwave, and satellite links (SNR 16–40 dB).
It is not suitable for HF ionospheric paths.

### Compression

Three algorithms negotiated at session setup, transparent to higher layers:

| Algorithm | Use case |
|---|---|
| LZ4 | Low-latency; good for structured text and log payloads |
| Zstd + pre-trained HPX dictionary | Best ratio on amateur/Winlink message traffic |
| None | Binary payloads that don't compress |

B2F/Winlink wire layer additionally supports **Gzip** (Type D) and **LZHUF/LH5** (Type C,
Winlink-wire-compatible LE-prefix format).

### Protocol and interfaces

- **ARDOP-compatible TCP TNC** (`openpulse-tnc`) — Pat-compatible command set;
  GRIDSQUARE, ARQBW, ARQTIMEOUT, CWID, SENDID, PING
- **KISS/AX.25 TNC** (`openpulse-kisstnc`) — full byte stuffing, AX.25 UI frames
- **B2F/Winlink** — ISS and IRS roles, FC/FS/Ff/Fq frames, Gzip and LZHUF compression
- **Direct TCP Winlink CMS gateway** (`openpulse-gateway`) — no TNC bridge needed
- **QSY frequency agility** — Ed25519-signed wire codec; initiator and responder session
  state machines; SNR-ranked channel selection; rig CAT via rigctld
- **Mesh broadcast daemon** — TTL-limited re-broadcast with duplicate suppression
- **Cross-band repeater** — configurable digipeater with trust-policy filtering
- **Multi-hop relay** — trust-weighted path scoring; hop-limit enforcement; duplicate
  suppression; `RelayForwarder` and `QueryForwarder` for query propagation

### Security and identity

- **Ed25519** handshake signing + transfer manifest signing/verification
- **ML-DSA-44 + ML-KEM-768** post-quantum handshake — Hybrid (Ed25519 + ML-DSA-44) and PQ-only modes
- **Three trust profiles**: OpenTrust, Balanced, Strict — configurable per deployment
- **PKI service** — Ed25519 trust-bundle signing with PostgreSQL persistence
- **Signed peer descriptors** — self-authenticating identity; peer ID is the verifying key bytes

### Channel simulation

- **Watterson HF fading** — Good F1/F2, Moderate, Poor profiles; Doppler-shaping filter
- **Gilbert-Elliott burst error** — configurable state machine; AWGN and burst modes
- **QRN/QRM/QSB/Chirp** — broadband noise, interference, slow fading, frequency drift
- DCD energy threshold and 0.3-persistence CSMA channel access

### Operator interfaces

| Interface | Description |
|---|---|
| **Operator panel** (`openpulse-panel`) | Full egui/eframe GUI connecting to the daemon via TCP control port; mode selection, PTT, QSY management, message store, live status |
| **TUI** (`openpulse-tui`) | ratatui terminal UI — HPX state (colour-coded), AFC/rate meters, DCD energy bar, scrollable transitions log |
| **CLI** (`openpulse-cli`) | Full-featured command-line interface: transmit, receive, benchmark, monitor, config init |
| **Signal testbench** (`openpulse-testbench`) | egui 4-column live view: TX / channel / mixed / RX; waterfall, spectrum, scatter; 7 channel models; SNR slider |

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

# Automated mode × channel test matrix (outputs to docs/test-reports/)
cargo run -p openpulse-testmatrix --no-default-features
```

The `--no-default-features` flag disables the CPAL audio backend. All tests must
pass with this flag. Never add tests that require real audio hardware.

---

## Repository layout

### Core layer

| Crate | Role |
|---|---|
| `crates/openpulse-core` | Frame format, CRC-16, FEC (RS+Conv+LDPC+interleaver), HPX session state machine, plugin registry, trust/signing, SAR, ACK, rate adaptation, relay, query propagation, peer cache, LZ4/Zstd compression, PQ handshake |
| `crates/openpulse-audio` | `LoopbackBackend` (testing) and `CpalBackend` (hardware, feature-gated) |
| `crates/openpulse-modem` | `ModemEngine`, `PipelineScheduler`, `ArqSession` (LLR-accumulating retry), benchmark harness, CSMA/DCD, channel sim harness |
| `crates/openpulse-channel` | Channel simulation: Watterson, Gilbert-Elliott, QRN/QRM/QSB/Chirp |
| `crates/openpulse-radio` | `PttController` trait: NoOp, SerialRtsDtr, Vox, Rigctld; `RigctldController` for CAT |
| `crates/openpulse-dsp` | RRC filter, PLL, Gardner timing recovery, LMS/DFE adaptive equalizer |
| `crates/openpulse-config` | Typed TOML configuration with CLI-override precedence |
| `crates/openpulse-gpu` | wgpu-backed BPSK DSP kernels; CPU fallback; `gpu` feature flag |

### Protocol layer

| Crate | Role |
|---|---|
| `crates/openpulse-ardop` | ARDOP-compatible TCP TNC; `openpulse-tnc` binary |
| `crates/openpulse-kiss` | KISS/AX.25 TNC; `openpulse-kisstnc` binary |
| `crates/openpulse-b2f` | B2F/Winlink state machine: FC/FS/Ff/Fq frames, Gzip (Type D), LZHUF (Type C) |
| `crates/openpulse-b2f-driver` | High-level ISS/IRS session driver over ARDOP TCP; e2e loopback tests |
| `crates/openpulse-gateway` | Direct TCP Winlink CMS gateway; `openpulse-gateway` binary |
| `crates/openpulse-qsy` | QSY frequency agility: Ed25519-signed wire codec, `QsySession` (initiator + responder), `QsyScanner` |
| `crates/openpulse-mesh` | Mesh broadcast daemon with TTL-limited re-broadcast |
| `crates/openpulse-repeater` | Cross-band repeater / digipeater with trust-policy filtering |
| `crates/openpulse-daemon` | Unified background daemon: modem engine, PTT, QSY, repeater, NDJSON+WebSocket control port |

### UI and tooling

| Crate | Role |
|---|---|
| `crates/openpulse-cli` | CLI binary: transmit, receive, benchmark, monitor NDJSON events, config init |
| `crates/openpulse-tui` | ratatui TUI: HPX state, AFC/rate meters, DCD energy bar, transitions log |
| `apps/openpulse-panel` | egui operator panel GUI connecting to daemon control port |
| `apps/openpulse-testbench` | egui signal-path testbench: 4-column waterfall/spectrum/scatter, 7 channel models |
| `apps/openpulse-testmatrix` | Automated mode × channel test matrix runner |
| `pki-tooling` | Ed25519 trust-bundle signing service with PostgreSQL persistence |

### Plugins

| Crate | Registered modes |
|---|---|
| `plugins/bpsk` | BPSK31, BPSK63, BPSK100, BPSK250; GPU path; LMS/DFE equalizer on RRC path |
| `plugins/qpsk` | QPSK125, QPSK250, QPSK500, QPSK1000, QPSK2000-RRC, QPSK9600-RRC |
| `plugins/psk8` | 8PSK500, 8PSK1000, 8PSK2000-RRC, 8PSK9600-RRC; max-log-MAP soft demodulator |
| `plugins/64qam` | 64QAM500, 64QAM1000, 64QAM2000-RRC; Gray-coded 8×8 PAM-8; soft demodulator |
| `plugins/scfdma` | SCFDMA52-8PSK, SCFDMA52-16QAM, SCFDMA52-32QAM, SCFDMA52-64QAM; DFT-CE + MMSE |
| `plugins/ofdm` | OFDM16, OFDM52; LS channel estimation + ZF equalization |
| `plugins/fsk4` | FSK4-ACK (100 baud ACK channel; Goertzel demodulator) |

---

## License

GNU General Public License v3.0 or later — see [LICENSE](LICENSE).
