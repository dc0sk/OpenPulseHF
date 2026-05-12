---
project: openpulsehf
doc: docs/marketing/flyer.md
status: draft
last_updated: 2026-05-12
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

**▶ 33 modulation modes** — from BPSK31 to 64QAM (12 kbps raw), OFDM multi-carrier, and SC-FDMA  
**▶ RRC matched filtering** — Root Raised Cosine pulse shaping on all RRC modes for clean spectrum and precise symbol recovery  
**▶ Adaptive rate ladder** — 20 speed levels (SL1–SL20), adjusts per-direction automatically, no operator input  
**▶ Multi-block RS FEC** — full Reed-Solomon protection at any payload size; no artificial per-frame byte limit  
**▶ Post-quantum security** — ML-DSA-44 + ML-KEM-768, future-proof from day one  
**▶ Winlink / Pat compatible** — drop-in replacement for VARA and ARDOP  
**▶ Full FEC stack** — RS t=16/t=32, Conv K=7 soft Viterbi, Memory-ARQ, concatenated codes  
**▶ Built-in signal-path testbench** — live 4-column waterfall + IQ scatter + BER meter, 7 channel models  
**▶ Automatic frequency correction (AFC)** — tracks ±62.5 Hz drift; tolerates imperfect radio calibration  
**▶ Up to 111 kbps effective throughput** — SCFDMA52 + LZ4 compression, measured in built-in testbench  
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
| Built-in signal-path testbench GUI | **✓** | — | — |
| Multi-block RS FEC (unlimited payload size) | **✓** | — | — |
| RRC matched filtering (clean adjacent channel) | **✓** | — | — |
| 322-case automated channel test matrix | **✓** | — | — |
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

| Plugin | Modes | Baud rates | Bandwidth | Raw data rate | Peak with LZ4† | Pulse shaping |
|---|---|---|---|---|---|---|
| BPSK | BPSK31 / 63 / 100 / 250 | 31–250 | 50–260 Hz | 31–250 bps | 62–500 bps | Hann overlap |
| BPSK | BPSK250-RRC | 250 | ~340 Hz | 250 bps | ~500 bps | RRC α=0.35 |
| QPSK | QPSK125–1000 / -HF | 125–1000 | 140 Hz – 1.1 kHz | 250–2000 bps | 500–4000 bps | Hann / Cosine |
| QPSK | QPSK500/1000-RRC | 500–1000 | 675 Hz – 1.35 kHz | 1000–2000 bps | 2.0–4.0 kbps | RRC α=0.35 |
| 8PSK | 8PSK500–1000 / -HF | 500–1000 | 540 Hz – 1.1 kHz | 1500–3000 bps | 3.0–6.0 kbps | Hann / Cosine |
| 8PSK | 8PSK500/1000-RRC | 500–1000 | 675 Hz – 1.35 kHz | 1500–3000 bps | 3.0–6.0 kbps | RRC α=0.35 |
| **64QAM** | **64QAM500 / 1000 / 2000-RRC** | **500–2000** | **540–2700 Hz** | **3000–12000 bps** | **6.0–24.0 kbps** | **Rectangular / RRC** |
| FSK4 | FSK4-ACK | 100 | ~400 Hz | ACK only | ACK only | Hann |
| OFDM | OFDM16 / OFDM52 | — | 625 Hz – 2 kHz | 889–2889 bps | 1.8–5.8 kbps | OFDM CP |
| SC-FDMA | SCFDMA16 / SCFDMA52 | — | 625 Hz – 2 kHz | 889–2889 bps | **1.8–5.8 kbps (peak: 111 kbps†)** | DFT-spread |

† Raw data rate = symbol rate × bits/symbol. "Peak with LZ4" uses ≈ 2× typical for text payloads
(Winlink messages, emails). The built-in testbench measures **111 kbps on SCFDMA52 + LZ4** with a
2048-byte compressible test frame (≈ 38× ratio on highly repetitive data).

RRC modes use a Gardner timing error detector (TED) with a Costas PLL for carrier
recovery — the same professional-grade loops found in LTE and DVB receivers.

#### Forward error correction

| Mode | Mechanism | Errors corrected | Overhead | Payload limit |
|---|---|---|---|---|
| RS(255,223) | Reed-Solomon t=16 | 16 bytes/block | 14% | **Unlimited** (multi-block) |
| RS(255,191) Strong | Reed-Solomon t=32 | 32 bytes/block | 25% | **Unlimited** (multi-block) |
| Concatenated | K=3 hard Conv + RS | burst + random | ~2.3× | Unlimited |
| Soft Viterbi | K=7 soft Conv + RS | random noise | ~2.3×, +5 dB | Unlimited |
| Memory-ARQ | Sample combining over N retx | noise floor | none | — |

All RS modes split large payloads into 255-byte blocks automatically.  No per-frame
payload cap — a 2048-byte payload receives the same full RS protection as a 128-byte one.

#### Session management

- HPX state machine · 20 speed levels (SL1–SL20)
- Independent per-direction rate adaptation
- Segmentation and reassembly (up to 64 KB)
- LZ4 + Zstd dictionary compression (negotiated in-band, signed in handshake)
- Automatic frequency correction (AFC) — tracks ±B/4 Hz per mode
- 0.3-persistence CSMA with 100 ms DCD hold window
- ChirpFallback at signal floor

#### Security

- Ed25519 classical signatures
- ML-DSA-44 post-quantum signatures (FIPS 204)
- ML-KEM-768 forward-secrecy key encapsulation (FIPS 203)
- Hybrid mode: both classical and post-quantum simultaneously
- Transfer manifest (SHA-256 + signature) verifies every data transfer

#### Signal-path testbench

The built-in `openpulse-testbench` GUI (egui/eframe) lets you explore any mode in real
time without a radio:

- **4 live taps**: TX waveform → noise → mixed → decoded signal
- **Per-tap view**: FFT spectrum (dBFS) + plasma-colourmap waterfall texture
- **IQ scatter plot**: real-time constellation diagram from the post-channel signal
- **7 channel models**: AWGN, Watterson F1/F2/Poor, Gilbert-Elliott light/burst, QRN, QRM, QSB, Chirp
- **Live stats bar**: BER, ECC correction rate, SNR estimate, effective data rate
- **Live audio capture**: connect a real radio receiver as input (CPAL backend)

#### Hardware support

- Any SSB transceiver with audio interface
- PTT: hamlib/rigctld · RTS/DTR serial · VOX · custom serial CAT
- Optional GPU acceleration (wgpu, any Vulkan/Metal/DX12 adapter)
- Raspberry Pi 4 (aarch64) · tested and cross-compiled in CI

#### Software interfaces

- `openpulse-tnc` — ARDOP-compatible TCP TNC (Pat-ready, port 8515/8516)
- `openpulse-kisstnc` — KISS/AX.25 TCP TNC (any APRS client, port 8100)
- `openpulse-gateway` — direct Winlink CMS TCP gateway
- `openpulse-tui` — ratatui terminal dashboard with AFC meter and DCD bar
- `openpulse-testbench` — egui signal-path testbench with live channel simulation
- `openpulse-panel` — operator panel GUI (connects to daemon control port)

---

### Verified against published channel models

The `openpulse-testmatrix` binary runs **322 test cases** covering every mode × FEC ×
compression × channel combination — and all 322 pass.  Channel models are calibrated
against ITU-R F.1487 (Watterson) and the Gilbert-Elliott burst model.  CI blocks any
merge that regresses a case.

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
