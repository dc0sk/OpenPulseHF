---
project: openpulsehf
doc: README.md
status: living
last_updated: 2026-05-12
---

# OpenPulseHF

> A plugin-based HF software modem written in Rust — built for reliable data over real ionospheric channels.

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

## Why OpenPulseHF?

HF data links are hostile: ionospheric fading, burst noise, Doppler spread, and narrow bandwidth.
OpenPulseHF was designed from the start to cope — with adaptive rate ladders that respond
per-direction to real channel conditions, a collaborative frequency-agility protocol that moves
the link to a better channel without operator intervention, and a post-quantum-capable handshake
that protects session identity today and in the post-quantum era.

Every feature ships with a deterministic, hardware-free test suite and a parametric
channel-simulation harness validated against published Watterson and Gilbert-Elliott models.

---

## Key features

### Modulation and waveforms — 33 modes across 7 plugins

#### HF narrow-band (≤ 2700 Hz occupied bandwidth)

| Plugin | Modes | Baud rate | Bits/sym | BW (Hz) | Eff. bit rate |
|---|---|---|---|---|---|
| BPSK | BPSK31 / BPSK63 / BPSK100 / BPSK250 / BPSK250-RRC | 31–250 | 1 | 50–340 | 19–150 bps |
| QPSK | QPSK125 / QPSK250 / QPSK500 / QPSK1000-HF / QPSK500-RRC / QPSK1000-RRC | 125–1000 | 2 | 140–1350 | 150–1200 bps |
| 8PSK | 8PSK500 / 8PSK500-RRC / 8PSK1000-HF / 8PSK1000-RRC | 500–1000 | 3 | 540–1350 | 900–1800 bps |
| 64QAM | 64QAM500 / 64QAM1000 / 64QAM2000-RRC | 500–2000 | 6 | 540–2700 | 1800–7200 bps |
| FSK4 | FSK4-ACK | 100 (200 ms/frame) | 2 | ~400 | ACK only |
| OFDM | OFDM16 / OFDM52 | — | 2/SC | 625–2000 | ~530–1730 bps |
| SC-FDMA | SCFDMA16 / SCFDMA52 — same BW as OFDM, 3–4 dB lower PAPR | — | 2/SC | 625–2000 | ~530–1730 bps |

Effective bit rate figures are approximate; overhead includes a per-frame preamble (32 symbols
for BPSK, 16 symbols for QPSK/8PSK/64QAM) plus an 8-symbol tail. Actual throughput also
depends on frame length, FEC mode, and channel conditions.
RRC modes (α = 0.35) occupy ~35% more bandwidth than their non-RRC counterparts.

#### UHF/VHF narrowband (12.5 kHz channel, 8 kHz audio)

| Mode | Baud | Bandwidth | Eff. bit rate | Use case |
|---|---|---|---|---|
| QPSK2000 / QPSK2000-RRC | 2000 | ~2700 Hz | ~2400 bps | 12.5 kHz PMR/LMR |
| 8PSK2000 / 8PSK2000-RRC | 2000 | ~2700 Hz | ~3600 bps | 12.5 kHz PMR/LMR high throughput |

#### UHF/VHF HD (12.5 kHz channel, 48 kHz audio)

| Mode | Baud | Bandwidth | Eff. bit rate | Use case |
|---|---|---|---|---|
| QPSK9600 / QPSK9600-RRC | 9600 | ~13 kHz | ~11.5 kbps | 12.5 kHz PMR/LMR high-speed |
| 8PSK9600 / 8PSK9600-RRC | 9600 | ~13 kHz | ~17.3 kbps | 12.5 kHz PMR/LMR maximum throughput |

Rate adaptation steps across all available modes automatically — independently per direction,
so an asymmetric path (good downlink, noisy uplink) is handled without penalising the
better direction.

---

### Forward error correction — 7 modes

| Mode | Mechanism | Corrects | Overhead |
|---|---|---|---|
| None | — | — | 0% |
| RS | RS(255,223) Reed-Solomon t=16 | 16 byte errors/block | 14% |
| RS-Interleaved | RS(255,223) + stride block interleaver | burst errors dispersed | 14% + IL |
| Concatenated | Conv K=3 Viterbi inner + RS(255,223) outer | burst + random | 2.28× |
| ShortRS | Short-block RS t=4 (ACK frames only) | 4 byte errors/frame | 8 B ECC |
| **RsStrong** | **RS(255,191) t=32** | **32 byte errors/block** | **25%** |
| **SoftConcatenated** | **K=7 soft Viterbi inner + RS(255,223) outer** | **random noise + burst** | **2.28×, +5 dB** |

**Memory-ARQ soft combining** is also available: sample buffers from N retransmissions are
element-wise averaged before demodulation, giving ~3 dB SNR gain per doubling of retransmissions
with no wire-protocol changes.

---

### Compression — negotiated in-band

| Algorithm | Savings on typical HF messages | Notes |
|---|---|---|
| None | — | Always available |
| LZ4 | 20–40% | Fast, general-purpose |
| **Zstd + HPX dictionary** | **40–60%** | Pre-trained on amateur radio traffic |

`compress_if_smaller()` tries both algorithms and keeps the smaller result.
Selection is negotiated in the signed handshake.

---

### ARQ session layer

- HPX state machine with 8 ACK types and 20 speed levels (SL1–SL20)
- ChirpFallback: after three consecutive NACKs at SL2, falls back to a narrowband chirp
- Segmentation and reassembly (SAR) handles payloads up to 64 KB per session
- Seven adaptive session profiles: `hpx500`, `hpx_hf`, `hpx_wideband`, `hpx_narrowband`,
  `hpx_narrowband_hd`, `hpx_ofdm_hf`, `hpx_wideband_hd` (SL12–SL14 mapping to 64QAM modes; SL15–SL20 reserved)

---

### First-to-market features ★

The following features are, to the best of our knowledge, first implementations
in any open-source amateur radio digital mode software:

| Feature | Notes |
|---|---|
| **ML-DSA-44 post-quantum signatures** (FIPS 204) | Hybrid + PQ-only modes |
| **ML-KEM-768 forward-secrecy KEM** (FIPS 203) | Session key encapsulated in handshake |
| **QSY frequency agility** | Stations collaboratively hop to a better frequency; Ed25519-signed |
| **SC-FDMA waveform** | DFT-spread OFDM, 3–4 dB lower PAPR than plain OFDM |
| **K=7 soft-decision Viterbi FEC** | True LLR inputs from BPSK/QPSK demodulators |
| **Memory-ARQ soft sample combining** | Element-wise averaging across retransmissions |
| **Zstd dictionary compression** | Pre-trained HPX dictionary for short payloads |
| **GPU DSP offload** (wgpu, optional) | BPSK modulation, demodulation, timing search |
| **Per-band TX attenuation persistence** | Remembers TX gain per frequency band via rigctld |
| **IQ complex baseband output** | Direct SDR upconversion from baseband samples |
| **FreeDV authenticated voice** (FF-11) | Ed25519-signed beacon via codec2 data channel |

---

### QSY frequency agility — first to market

Two stations collaboratively negotiate a move to a less congested frequency without operator
input.  Each side scans candidate frequencies via rigctld, exchanges SNR readings over the
existing data channel, votes for the best common frequency, and switches on a coordinated timer.

- Five-frame ASCII protocol (QSY_REQ / QSY_LIST / QSY_VOTE / QSY_ACK / QSY_REJECT)
- Every frame is Ed25519-signed; tampering returns an explicit `InvalidSignature` error
- Disabled by default; enabled per-session via `[qsy]` config section

---

### Post-quantum in-band handshake — first to market

Three signing modes, negotiated in the connection handshake:

| Mode | Ed25519 | ML-DSA-44 | Use case |
|---|:---:|:---:|---|
| Classical | ✓ | — | Backward compatible |
| **Hybrid** | **✓** | **✓** | **Transition period (recommended)** |
| PQ-only | — | ✓ | Fully post-quantum |

Key encapsulation for forward secrecy uses **ML-KEM-768**.  In hybrid mode both classical and
post-quantum signatures are required — sessions are authenticated against today's trust stores
while carrying a PQ signature that will matter when classical keys are threatened.

---

### Compatibility with existing HF software

| Application | Protocol | Status |
|---|---|---|
| Pat (Winlink client) | ARDOP TCP command + data ports | Shipped — `openpulse-tnc` |
| Winlink CMS | B2F / Winlink over TCP | Shipped — `openpulse-gateway` |
| Any APRS or AX.25 application | KISS TNC over TCP | Shipped — `openpulse-kisstnc` |
| direwolf / soundmodem | KISS framing | Shipped |
| hamlib / flrig | rigctld CAT (PTT, frequency, S-meter) | Shipped |
| Custom rig without hamlib | Generic serial CAT (TOML-scripted) | Shipped — FF-13 |

`openpulse-tnc` speaks ARDOP TCP natively — Pat connects to port 8515 without any
configuration changes.

---

### Channel simulation and test harness

Deterministic, hardware-free testing against published propagation models:

| Model | Profiles |
|---|---|
| AWGN | Systematic SNR sweep 0–30 dB, seeded RNG |
| Watterson (ITU-R F.1487) | Good F1, Good F2, Moderate F1, Poor F1, Extreme |
| Gilbert-Elliott | Light, Moderate, Heavy, Severe burst profiles |
| QRN | Middleton Class-A impulsive atmospheric noise |
| QRM | Co-channel tonal interference (configurable frequencies/amplitudes) |
| QSB | Slow multiplicative fading |
| Chirp | Swept-frequency chirp interference |

The `openpulse-testmatrix` binary runs the mode × FEC × compression × channel matrix
and produces Markdown + CSV reports including per-test BER and effective throughput.
Run without flags for the quick tier (core modes and channels); add `--full` for the
complete matrix across all propagation profiles and payload sizes.

---

### Hardware support

- Any SSB transceiver with a sound-card audio interface
- **PTT**: hamlib/rigctld · RTS/DTR serial · VOX · TOML-scripted serial CAT (FF-13)
- Optional GPU acceleration: any Vulkan / Metal / Direct3D 12 adapter via `wgpu`
- **Raspberry Pi 4/5** (aarch64): cross-compiled and CI-tested

---

## What is already delivered

All items below are merged, tested, and in `main`:

**Core modem (Phases 1–9)**
Modulation plugins (BPSK, QPSK, 8PSK, FSK4, OFDM, SC-FDMA), rate adaptation, 8 ACK types,
signed handshake, SAR, DCD/CSMA, peer cache, multi-hop relay, post-quantum handshake,
GPU acceleration (optional), B2F/Winlink, ARDOP TNC, KISS TNC, direct CMS gateway,
TOML config, structured JSON event stream, ratatui TUI, egui testbench + operator panel GUI

**Far-future features (FF series)**

| Item | Feature |
|---|---|
| FF-1 | QSY frequency agility with rigctld |
| FF-2 | I/Q complex baseband output for SDR upconversion |
| FF-3 | RRC matched filtering + Gardner timing recovery + Costas PLL |
| FF-4 | OFDM multi-carrier plugin (OFDM16, OFDM52) with LS+ZF equalization |
| FF-5 | UHF/VHF narrowband/HD modes (2000 and 9600 baud QPSK/8PSK) |
| FF-6 | Binary spectrum channel (20 Hz waterfall, operator panel) |
| FF-7 | Tanh TX limiter (soft-clip for PA back-off on 8PSK/RRC) |
| FF-8 | Per-band TX attenuation persistence via rigctld |
| FF-9 | HPX reactor pattern (event-driven session state machine) |
| FF-10 | Zstd dictionary compression |
| FF-11 | FreeDV authenticated voice shim (Ed25519 via codec2 data channel) |
| FF-12 | SC-FDMA waveform plugin (SCFDMA16, SCFDMA52) |
| FF-13 | Generic serial CAT (TOML-scripted, for rigs not in hamlib) |

**FEC backlog (BL-FEC series)**

| Item | Feature |
|---|---|
| BL-FEC-1 | Concatenated Conv+RS session mode (PR #169) |
| BL-FEC-2 | Strong RS(255,191) t=32 codec (PR #171) |
| BL-FEC-3 | Short-block RS for ACK/control frames (PR #170) |
| BL-FEC-4 | Memory-ARQ soft combining (PR #171) |
| BL-FEC-5 | K=7 soft-decision Viterbi + `demodulate_soft()` plugin API (PR #177) |
| BL-FEC-6 | `IterativeDecoder` trait + `LdpcCodec` stub — GPU path reserved (PR #176) |

---

## Getting started

### Prerequisites

```bash
# Linux (Debian/Ubuntu)
sudo apt install libasound2-dev

# macOS — no extra packages needed

# Raspberry Pi 4/5 cross-compilation
cargo install cross
```

### Build and run

```bash
# Full workspace build
cargo build --workspace

# Run the CLI in loopback mode (no radio hardware needed)
cargo run -p openpulse-cli --no-default-features -- --backend loopback --log info transmit "Hello HF"

# Start an ARDOP-compatible TNC (Pat-ready) — loopback mode, no hardware
cargo run -p openpulse-ardop -- --mode BPSK250 --cmd-port 8515 --data-port 8516

# Start an ARDOP-compatible TNC with real radio hardware (requires cpal feature)
cargo run -p openpulse-ardop --features cpal -- --mode BPSK250 --backend cpal --cmd-port 8515 --data-port 8516

# Start a KISS TNC (APRS-ready) — loopback mode, no hardware
cargo run -p openpulse-kiss -- --mode BPSK250 --port 8100

# Start a KISS TNC with real radio hardware (requires cpal feature)
cargo run -p openpulse-kiss --features cpal -- --mode BPSK250 --backend cpal --port 8100

# Run the signal-path benchmark
cargo run -p openpulse-cli --no-default-features -- --backend loopback --log error benchmark run

# Run the quick-tier test matrix (virtual channels, no hardware)
cargo run -p openpulse-testmatrix --no-default-features

# Run the full test matrix (all propagation channels and payload sizes)
cargo run -p openpulse-testmatrix --no-default-features -- --full --output docs/test-reports
```

### Tests

```bash
# Full test suite (no audio hardware required)
cargo test --workspace --no-default-features

# Clippy (CI gate)
cargo clippy --workspace --no-default-features -- -D warnings
```

### Raspberry Pi cross-compilation

```bash
cross build --release --workspace --target aarch64-unknown-linux-gnu --no-default-features
```

---

## Architecture overview

```
┌─────────────────────────────────────────────────────────────────────┐
│  Applications                                                       │
│  openpulse-cli  openpulse-tui  openpulse-testbench  openpulse-panel│
└───────────────────┬─────────────────────────────────────────────────┘
                    │
┌───────────────────▼─────────────────────────────────────────────────┐
│  Protocol layer                                                     │
│  openpulse-ardop  openpulse-kiss  openpulse-b2f  openpulse-gateway │
│  openpulse-qsy    openpulse-mesh  openpulse-repeater               │
└───────────────────┬─────────────────────────────────────────────────┘
                    │
┌───────────────────▼─────────────────────────────────────────────────┐
│  Modem engine (openpulse-modem)                                     │
│  ModemEngine · PipelineScheduler · ChannelSimHarness               │
└────────┬──────────────────────────────────┬────────────────────────┘
         │                                  │
┌────────▼────────┐               ┌─────────▼──────────────────────┐
│  Core           │               │  Plugins                       │
│  openpulse-core │               │  bpsk · qpsk · psk8 · 64qam   │
│  FecCodec · SAR │               │  fsk4 · ofdm · scfdma          │
│  HpxReactor     │               └────────────────────────────────┘
│  PQ handshake   │
│  Trust / PKI    │
└─────────────────┘
```

See `docs/architecture.md` for the full crate map and design decisions.

---

## Documentation

| Topic | Document |
|---|---|
| Architecture and crate map | `docs/architecture.md` |
| CLI usage | `docs/cli-guide.md` |
| HPX waveform design | `docs/hpx-waveform-design.md` |
| HPX state machine | `docs/hpx-session-state-machine.md` |
| Channel simulation harness | `docs/benchmark-harness.md` |
| Feature roadmap | `docs/roadmap.md` |
| FEC backlog | `docs/backlog-fec-improvements.md` |
| Plugin contribution guide | `docs/contributing-plugins.md` |
| Commercial plugin interface | `docs/plugin-commercial-interface.md` |
| Regulatory compliance | `docs/regulatory.md` |
| VARA research and comparison | `docs/vara-research.md` |
| On-air test plan | `docs/on-air_testplan.md` |
| On-air automation scripts | `scripts/run-onair-tests.sh` |
| Code review request | `docs/requests/code-review.md` |
| HAMRADIO 2026 marketing | `docs/marketing/` |
| Agent / contributor safety rules | `AGENTS.md`, `docs/AGENTS.md` |

---

## Contributing

PRs are welcome.  Read `docs/contributing-plugins.md` before writing a new modulation plugin.
All PRs must pass `cargo test --workspace --no-default-features` and
`cargo clippy --workspace --no-default-features -- -D warnings`.

---

## License

GNU General Public License v3.0 or later — see [LICENSE](LICENSE).

For commercial or proprietary plugin integration, see
[`docs/plugin-commercial-interface.md`](docs/plugin-commercial-interface.md).
