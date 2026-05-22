---
project: openpulsehf
doc: README.md
status: living
last_updated: 2026-05-20
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
[![Donate via PayPal](https://img.shields.io/badge/Donate-PayPal-blue.svg?logo=paypal)](https://www.paypal.com/donate/?hosted_button_id=WY9U4MQ3ZAQWC)

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
| **GPU-accelerated DSP** | 11 wgpu compute kernels covering BPSK, RRC FIR, 256-pt FFT/IFFT, SC-FDMA (hard + soft, all constellations), OFDM16/52 (hard + soft), 64QAM, and 8PSK — all with automatic CPU fallback. See [GPU-accelerated features](#gpu-accelerated-features). |
| **QSY frequency agility** | Ed25519-signed QSY_REQ/LIST/VOTE/ACK wire protocol. Initiator and responder roles wired into the daemon; rig CAT control via rigctld. |
| **FreeDV authenticated voice** | Ed25519-signed authentication beacons transmitted via the FreeDV Qt-GUI UDP data port (`openpulse-freedv-auth`); no FreeDV fork required. |

On-air regulatory validation has not been completed. All tests use loopback and
simulated-channel paths only.

---

## First-to-market features

Capabilities that are firsts or near-firsts among open-source amateur digital-mode software:

| # | Capability | Evidence / where to look |
|---|---|---|
| 1 | **Post-quantum in-band handshake** | ML-DSA-44 + ML-KEM-768 negotiated inside the ConReq/ConAck wire frames; Hybrid mode dual-signs with Ed25519 + ML-DSA-44 simultaneously (`crates/openpulse-core/src/pq_handshake.rs`) |
| 2 | **SC-FDMA (LTE uplink waveform) on HF** | DFT-spread OFDM with DFT-CE pilot-aided channel estimation and MMSE equalization; 3–4 dB lower PAPR than equivalent OFDM (`plugins/scfdma`) |
| 3 | **64QAM and SCFDMA-64QAM soft demodulation** | Gray-coded 64QAM max-log-MAP LLR demodulator; SCFDMA52-64QAM reaching 8 667 bps gross over a 2 kHz slice (`plugins/64qam`, `plugins/scfdma`) |
| 4 | **Working LDPC belief propagation** | Rate-1/2 min-sum BP — not a passthrough stub; wired into `transmit_with_ldpc` / `receive_with_ldpc` in the modem engine (`crates/openpulse-core/src/ldpc.rs`) |
| 5 | **LLR-accumulating Memory-ARQ** | Soft LLR values accumulated across retransmissions (PACTOR-style); mode switching on sustained NACK (`crates/openpulse-modem/src/arq_session.rs`) |
| 6 | **GPU DSP across 6 modulation families** | 11 wgpu WGSL kernels (BPSK, RRC FIR, 256-pt FFT, SC-FDMA hard/soft covering QPSK–64QAM, OFDM16/52 hard/soft, 64QAM, 8PSK) with CPU fallback — see [GPU-accelerated features](#gpu-accelerated-features) |
| 7 | **Ed25519-signed QSY frequency agility** | Full initiator + responder state machines wired into the daemon; SNR-ranked channel-list negotiation; rig CAT via rigctld (`crates/openpulse-qsy`) |
| 8 | **Zstd pre-trained compression dictionary** | Dictionary trained on amateur/Winlink traffic patterns; negotiated at session setup and covered by handshake signature (`crates/openpulse-core/src/compression.rs`) |
| 9 | **Trust-weighted multi-hop relay with query propagation** | `RelayForwarder` enforces hop limits and suppresses duplicates; `score_route` weights paths by trust level (Verified=4 … Reduced=1); `QueryForwarder` propagates route-discovery requests across nodes (`crates/openpulse-core/src/relay.rs`, `query_propagation.rs`) |
| 10 | **Cross-band full-duplex repeater** | `CrossBandRepeater` runs in a daemon-managed thread; `EnableRepeater`/`DisableRepeater` control commands; trust-policy filtering on forwarded frames (`crates/openpulse-repeater`) |
| 11 | **Mesh broadcast daemon with authenticated beacons** | TTL-limited re-broadcast; (session_id, nonce) duplicate suppression; beacon payloads carry signed peer descriptors where the peer ID *is* the Ed25519 verifying key (`crates/openpulse-mesh`) |
| 12 | **FreeDV frame signing via codec2 data channel** | External shim adding Ed25519 per-frame signatures to FreeDV voice transmissions using the codec2 embedded data channel; no FreeDV fork required |

---

## Feature tables

### Modulation types

| Mode | Plugin | Baud | Bits/sym | Gross bps | Waveform | Notes |
|---|---|---|---|---|---|---|
| BPSK31 | `bpsk` | 31.25 | 1 | 31 | Single-carrier | Narrowband HF |
| BPSK63 | `bpsk` | 62.5 | 1 | 63 | Single-carrier | |
| BPSK100 | `bpsk` | 100 | 1 | 100 | Single-carrier | |
| BPSK250 | `bpsk` | 250 | 1 | 250 | Single-carrier + RRC | |
| QPSK125 | `qpsk` | 62.5 | 2 | 125 | Single-carrier | |
| QPSK250 | `qpsk` | 125 | 2 | 250 | Single-carrier | |
| QPSK500 | `qpsk` | 250 | 2 | 500 | Single-carrier | |
| QPSK1000 | `qpsk` | 500 | 2 | 1 000 | Single-carrier | |
| QPSK2000-RRC | `qpsk` | 1 000 | 2 | 2 000 | Single-carrier + RRC | |
| QPSK9600-RRC | `qpsk` | 4 800 | 2 | 9 600 | Single-carrier + RRC | VHF/UHF |
| 8PSK500 | `psk8` | 167 | 3 | 500 | Single-carrier | Gray-coded |
| 8PSK1000 | `psk8` | 333 | 3 | 1 000 | Single-carrier | |
| 8PSK2000-RRC | `psk8` | 667 | 3 | 2 000 | Single-carrier + RRC | |
| 8PSK9600-RRC | `psk8` | 3 200 | 3 | 9 600 | Single-carrier + RRC | VHF/UHF |
| 64QAM500 | `64qam` | 83 | 6 | 500 | Single-carrier | |
| 64QAM1000 | `64qam` | 167 | 6 | 1 000 | Single-carrier | |
| 64QAM2000-RRC | `64qam` | 333 | 6 | 2 000 | Single-carrier + RRC | Requires SNR ≥ 25 dB |
| SCFDMA16 | `scfdma` | — | 2 | ~889 | SC-FDMA (16 SCs, QPSK) | DFT-CE + MMSE |
| SCFDMA52 | `scfdma` | — | 2 | ~2 889 | SC-FDMA (52 SCs, QPSK) | Adaptive pilot density |
| SCFDMA52-8PSK | `scfdma` | — | 3 | ~4 333 | SC-FDMA (52 SCs, 8PSK) | |
| SCFDMA52-16QAM | `scfdma` | — | 4 | ~5 778 | SC-FDMA (52 SCs, 16QAM) | |
| SCFDMA52-32QAM | `scfdma` | — | 5 | ~7 222 | SC-FDMA (52 SCs, cross-32QAM) | |
| SCFDMA52-64QAM | `scfdma` | — | 6 | ~8 667 | SC-FDMA (52 SCs, 64QAM) | |
| SCFDMA52-64QAM-P4 | `scfdma` | — | 6 | ~8 167 | SC-FDMA (49 SCs, dense pilots) | |
| OFDM16 | `ofdm` | — | 2 | ~444 | OFDM (16 SCs, QPSK) | LS + ZF |
| OFDM52 | `ofdm` | — | 2 | ~1 444 | OFDM (52 SCs, QPSK) | |
| FSK4-ACK | `fsk4` | 100 | 2 | 200 | 4-FSK | ACK control channel only |

### MAC / channel access types
| Mechanism | Where used | Description |
|---|---|---|
| **0.3-persistence CSMA** | `openpulse-modem` | DCD energy check; transmit deferred when channel busy; configurable per `ModemEngine` |
| **DCD energy threshold** | `openpulse-core` (dcd.rs) | RMS energy gate with configurable hold window (default 100 ms); forced-busy override for testing |
| **HPX adaptive session** | `openpulse-core` (hpx.rs) | ACK/NACK-driven speed-ladder state machine; `RateAdapter` with per-level SNR gates and NACK-decrement hysteresis |
| **ARQ retry loop** | `openpulse-modem` (arq_session.rs) | LLR-accumulating retransmission loop; mode switching on sustained NACK; configurable retry limit |
| **QSY frequency agility** | `openpulse-qsy` | SNR-ranked channel-list negotiation; initiator transmits QSY_REQ → LIST → VOTE/ACK; responder role wired into daemon receive path |
| **Cross-band repeater** | `openpulse-repeater` | Full-duplex digipeater; configurable trust-policy filter; `EnableRepeater`/`DisableRepeater` daemon commands |
| **Mesh re-broadcast** | `openpulse-mesh` | TTL-limited beacon re-broadcast with (session_id, nonce) duplicate suppression |
| **Multi-hop relay** | `openpulse-core` (relay.rs) | `RelayForwarder` with trust-weighted path scoring; hop-limit enforcement; `score_route`/`select_best_scored_route` |

### Compression types

| Algorithm | Layer | Direction | Notes |
|---|---|---|---|
| **LZ4** | Session (in-band) | Both | `lz4_flex`; transparent negotiation in ConReq/ConAck; fast, good for structured text |
| **Zstd + HPX dictionary** | Session (in-band) | Both | Pre-trained dictionary on amateur/Winlink traffic; best compression ratio |
| **None** | Session (in-band) | Both | Binary payloads that are already compressed |
| **Gzip** | B2F wire (Type D) | Both | `flate2`; Winlink Type D proposal |
| **LZHUF / LH5** | B2F wire (Type C) | Both | `oxiarc-lzhuf`; 4-byte LE prefix; Winlink Type C wire-compatible |

Compression algorithm negotiated at session setup via `supported_compression` / `selected_compression` fields in ConReq/ConAck, covered by Ed25519 signature — post-signing injection is detectable.

### ARQ types

| Type | Crate / module | Description |
|---|---|---|
| **Stop-and-wait ARQ** | `openpulse-modem` (engine.rs) | Basic per-frame ACK; NACK triggers retransmit of last frame |
| **LLR-accumulating ARQ (Memory-ARQ)** | `openpulse-modem` (arq_session.rs) | Soft LLR values accumulated across retransmissions (PACTOR-style); each retry adds soft-combining gain; mode switch on sustained NACK |
| **SAR (Segmentation and Reassembly)** | `openpulse-core` (sar.rs) | 4-byte header (segment_id, fragment_index, fragment_total); max 64 005 bytes per segment; configurable reassembly timeout; duplicate-idempotent |
| **QSY ACK** | `openpulse-qsy` | Ed25519-signed ACK/REJECT frames completing the QSY_REQ → LIST → VOTE → ACK negotiation loop |

### Error correction types

| Algorithm | Crate / module | Code rate | Strength | Notes |
|---|---|---|---|---|
| **Reed-Solomon RS(255,223)** | `openpulse-core` (fec.rs) | 223/255 ≈ 0.87 | 16-byte burst correction per block | Default for HF burst-error profiles; always paired with block interleaver |
| **Reed-Solomon RS(255,191)** | `openpulse-core` (fec.rs) | 191/255 ≈ 0.75 | 32-byte burst correction per block | Higher erasure tolerance |
| **Block interleaver** | `openpulse-core` (fec.rs) | 1.0 | Disperses burst errors | Configurable depth; used with RS by default |
| **Convolutional K=3** | `openpulse-core` (conv.rs) | 1/2 | AWGN-dominant paths | G={7,5}; hard-decision Viterbi; better than RS at random-error BER 1% |
| **LDPC rate-1/2** | `openpulse-core` (ldpc.rs) | 1/2 | Highest coding gain | Min-sum belief propagation; configurable iterations; first open-source HF modem with working LDPC |
| **Turbo (rate-1/3 PCCC)** | `openpulse-core` (turbo.rs) | 1/3 | Near-capacity on AWGN | RSC K=3, 3GPP QPP interleaver K=40–6144, Max-Log-MAP BCJR, 8 iterations, CRC-16 early exit |
| **Concatenated RS + Conv** | `openpulse-core` | ~0.44 | Strong burst + AWGN | RS outer, Conv inner |
| **Short-block FEC** | `openpulse-core` | varies | ACK/control frames | For FSK4-ACK and small control payloads |

### GPU-accelerated features

All GPU functions return `Option<T>` — `None` triggers automatic CPU fallback. Gated by `#[cfg(feature = "gpu")]`; `--no-default-features` builds use CPU paths throughout.

| Feature | Kernel | Crate / file | Description |
|---|---|---|---|
| **BPSK modulation** | `bpsk_modulate.wgsl` | `openpulse-gpu` / `bpsk-plugin` | GPU symbol mapping and carrier generation |
| **BPSK IQ demodulation** | `bpsk_iq_demod.wgsl` | `openpulse-gpu` / `bpsk-plugin` | Parallel IQ correlation across all sample offsets |
| **BPSK timing search** | `bpsk_timing.wgsl` | `openpulse-gpu` / `bpsk-plugin` | Symbol-timing offset search via parallel energy integration |
| **RRC FIR convolution** | `rrc_fir.wgsl` | `openpulse-gpu` | Causal RRC pulse-shaping filter; workgroup 256; wired into BPSK, QPSK, 8PSK, SC-FDMA |
| **256-pt FFT / IFFT** | `fft256.wgsl` | `openpulse-gpu` | Cooley-Tukey radix-2 DIT; one workgroup per symbol; shared-memory in-place butterfly |
| **Batch FFT (SC-FDMA hard demod)** | `fft256.wgsl` (batched) | `scfdma-plugin` | All per-symbol FFTs dispatched in one `gpu_fft256_batch` call; CPU DFT-CE + MMSE + demap; covers QPSK, 8PSK, 16QAM, 32QAM, 64QAM SC-FDMA variants |
| **Batch FFT (SC-FDMA soft demod)** | `fft256.wgsl` (batched) | `scfdma-plugin` | Same batch dispatch; CPU DFT-CE + MMSE + LLR computation; all SC-FDMA constellations including 16QAM and 32QAM |
| **64QAM soft demodulation** | `fft256.wgsl` (batched) | `64qam-plugin` | GPU batch FFT for symbol timing; CPU max-log-MAP LLR |
| **8PSK soft demodulation** | `fft256.wgsl` (batched) | `psk8-plugin` | GPU batch FFT; CPU Gray-coded LLR |
| **OFDM16/52 hard demodulation** | `fft256.wgsl` (batched) | `ofdm-plugin` | Batch FFT across all OFDM symbols; CPU LS channel estimation + ZF equalization + BPSK demap |
| **OFDM16/52 soft demodulation** | `fft256.wgsl` (batched) | `ofdm-plugin` | Same batch FFT; CPU ZF equalization + BPSK LLR output |

### Adaptive rate profiles

Six `SessionProfile` mappings from speed levels to modes, driven by ACK/NACK feedback
and per-level SNR floor/ceiling gates:

| Profile | SL range | Initial | Top mode | Target link |
|---|---|---|---|---|
| `hpx_hf` | SL2–SL8 | SL2 | SCFDMA52-8PSK | HF ionospheric |
| `hpx_narrowband` | SL8–SL11 | SL8 | 8PSK2000-RRC | Narrowband HF / VHF |
| `hpx_wideband` | SL8–SL11 | SL8 | 8PSK1000 | Wideband HF |
| `hpx_ofdm_hf` | SL5–SL6 | SL5 | OFDM52 | HF OFDM ladder |
| `hpx_narrowband_hd` | SL8–SL9 | SL8 | 8PSK9600-RRC | VHF/UHF narrowband |
| `hpx_wideband_hd` | SL12–SL15 | SL12 | 64QAM2000-RRC | VHF/UHF FM / satellite |

`hpx_wideband_hd` requires SNR ≥ 16 dB and is not suitable for HF ionospheric paths.

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
- **FreeDV frame signing** (`crates/openpulse-freedv-auth`) — Ed25519 signatures over the codec2 embedded data channel; authenticates voice transmissions without modifying FreeDV itself

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
| **CLI** (`openpulse-cli`) | Full-featured command-line interface: transmit, receive, benchmark, monitor, config init, calibrate (audio/PTT/AFC) |
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
