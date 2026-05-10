---
project: openpulsehf
doc: docs/marketing/flyer.md
status: draft
created: 2026-05-09
---

# OpenPulseHF — HAMRADIO 2026 Flyer

*A4 / Letter single page — front and back*

---

## FRONT

```
┌─────────────────────────────────────────────────────────────────────┐
│                                                                     │
│   ██████  ██████  ███████ ███    ██ ██████  ██    ██ ██      ███████│
│  ██    ██ ██   ██ ██      ████   ██ ██   ██ ██    ██ ██      ██     │
│  ██    ██ ██████  █████   ██ ██  ██ ██████  ██    ██ ██      ███████│
│  ██    ██ ██      ██      ██  ██ ██ ██      ██    ██ ██           ██│
│   ██████  ██      ███████ ██   ████ ██       ██████  ███████ ███████│
│                                                                     │
│                    H  F    —    T h e    N e x t    G e n            │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### The open-source HF data modem that sets new standards

OpenPulseHF is a **free, open-source (GPL v3)** HF digital modem written in Rust — built for
reliable data transfer over real ionospheric channels.  No dongles.  No subscriptions.
No proprietary firmware.  Works with your existing SSB transceiver.

---

### Why OpenPulseHF?

**▶ 30+ modulation modes** — from BPSK31 to 8PSK, OFDM multi-carrier, and SC-FDMA  
**▶ Adaptive rate ladder** — 11 speed levels, adjusts per-direction automatically  
**▶ Post-quantum security** — ML-DSA-44 + ML-KEM-768, future-proof from day one  
**▶ Winlink / Pat compatible** — drop-in replacement for VARA and ARDOP  
**▶ Full FEC stack** — RS, Conv K=7 soft Viterbi, Memory-ARQ, concatenated codes  
**▶ Runs on Raspberry Pi** — cross-compiles to aarch64, tested on RPi 4  

---

### World firsts in amateur radio digital modes

| Feature | OpenPulseHF | VARA | ARDOP |
|---|:---:|:---:|:---:|
| Post-quantum handshake (ML-DSA-44) | **✓** | — | — |
| QSY frequency agility (auto channel-hop) | **✓** | — | — |
| SC-FDMA waveform (lower PAPR than OFDM) | **✓** | — | — |
| Soft K=7 Viterbi FEC | **✓** | — | — |
| Memory-ARQ soft combining | **✓** | — | — |
| Zstd dictionary compression | **✓** | — | — |
| GPU-accelerated DSP (optional) | **✓** | — | — |
| 100% open source | **✓** | — | — |

---

### Compatible with everything you already use

```
Pat  ──→  ARDOP TCP  ──→  OpenPulseHF  ──→  Your SSB radio
APRS ──→  KISS TNC   ──→  OpenPulseHF  ──→  Your SSB radio
Winlink CMS ←──────────── OpenPulseHF  ──→  Your SSB radio
```

---

**github.com/dc0sk/OpenPulseHF** · GPL v3 · Rust · RPi 4 ready

---

## BACK

### Technical specifications

#### Modulation modes

| Plugin | Modes | Baud rates | Bandwidth |
|---|---|---|---|
| BPSK | BPSK31 / 63 / 100 / 250 / -RRC | 31–250 | 31–250 Hz |
| QPSK | QPSK125–9600 / -HF / -RRC | 125–9600 | 125 Hz – 13 kHz |
| 8PSK | 8PSK500–9600 / -HF / -RRC | 500–9600 | 500 Hz – 13 kHz |
| FSK4 | FSK4-ACK | 100 | ~400 Hz |
| OFDM | OFDM16 / OFDM52 | ~889–2889 bps gross | 625 Hz – 2 kHz |
| SC-FDMA | SCFDMA16 / SCFDMA52 | ~889–2889 bps gross | 625 Hz – 2 kHz |

#### Forward error correction

RS(255,223) · RS(255,191) t=32 · Conv K=7 soft Viterbi · Concatenated Conv+RS ·
Short-block RS (ACK frames) · Memory-ARQ soft combining · Block interleaving

#### Session management

- HPX state machine · 11 speed levels (SL1–SL11)
- Independent per-direction rate adaptation
- Segmentation and reassembly (up to 64 KB)
- LZ4 + Zstd dictionary compression (negotiated in-band)
- ChirpFallback at signal floor

#### Security

- Ed25519 classical signatures
- ML-DSA-44 post-quantum signatures (FIPS 204)
- ML-KEM-768 forward-secrecy key encapsulation (FIPS 203)
- Hybrid mode: both classical and post-quantum, simultaneously

#### Hardware support

- Any SSB transceiver with audio interface
- PTT: hamlib/rigctld · RTS/DTR serial · VOX · custom serial CAT
- Optional GPU acceleration (wgpu, any Vulkan/Metal/DX12 adapter)
- Raspberry Pi 4 (aarch64) · tested and cross-compiled in CI

#### Software interfaces

- `openpulse-tnc` — ARDOP-compatible TCP TNC (Pat-ready)
- `openpulse-kisstnc` — KISS/AX.25 TCP TNC (any APRS client)
- `openpulse-gateway` — direct Winlink CMS TCP gateway
- `openpulse-tui` — ratatui terminal dashboard
- egui signal-path testbench and operator panel GUI

---

### System requirements

**Linux / macOS / Windows** · Rust stable · ALSA (Linux) or CoreAudio (macOS)  
No bundled C DSP libraries (no libcodec2, no libfec) — ALSA required on Linux

---

**Get started in 5 minutes:**

```bash
git clone https://github.com/dc0sk/OpenPulseHF
cargo build --release -p openpulse-cli
./target/release/openpulse --help
```

---

*OpenPulseHF is free software licensed under the GNU GPL v3.*  
*Copyright © 2025–2026 OpenPulseHF Contributors.*  
*HAMRADIO 2026 · Friedrichshafen, Germany*
