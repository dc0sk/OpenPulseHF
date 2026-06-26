---
project: openpulsehf
doc: README.md
status: living
last_updated: 2026-06-24
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

<img src="docs/OpenPulseHF.png" alt="OpenPulseHF QR code" width="180">

---
## Supporters

Big thanks to:

DD2ZM for providing access to a remote controllable station for testing.

DB1IUA for helping stuffing the budget sink-hole.

---
## Status

**v0.2.1 (24th of June '26)** — headline feature: CE-SSB transmit envelope conditioning (per-mode, default-on for the high-PAPR OFDM/SC-FDMA modes). The average-power gain at fixed PEP is confirmed on-air (+1.18 dB on 2 m via an FT-991A), matching the channel-sim prediction; software ACPR and an on-air SDR spectral-mask check show no added splatter on QPSK OFDM (dense OFDM-HOM stays clean at normal data-mode drive). The operator panel now carries a CE-SSB toggle and a tabbed Messages/Event-Log pane.

9th of June '26, the soundsystem bugs have been fixed and validated, now returning to on-air tests.

As of 6th of June '26, I'm  busy with on-air-testing and fixing a lot of bugs and some misconceptions. Mostly related to Linux' soundsystems.

---

## Why OpenPulseHF?

Several capabilities here are firsts or near-firsts in open-source amateur digital modes:

| Capability | What makes it different |
|---|---|
| **Post-quantum link security** | ML-DSA-44 signing + ML-KEM-768 key encapsulation negotiated in-band. Hybrid mode signs with both Ed25519 and ML-DSA-44 simultaneously. No other open HF modem does this. |
| **SC-FDMA waveform on HF** | Single-Carrier FDMA (the LTE uplink waveform) brought to HF with DFT-CE pilot-aided channel estimation and MMSE equalization. (DFT-spread is the low-PAPR-capable structure; the current frequency-interleaved pilot scheme limits the realized PAPR — see [mode/FEC guide §7](docs/mode-fec-ladder.md).) |
| **64QAM and SCFDMA-64QAM with soft demodulation** | Gray-coded 64QAM with max-log-MAP soft demodulator. Aggressive constellation for VHF/UHF links with proper soft FEC backing. |
| **Pilot-framed carrier recovery** | A single-carrier waveform family (`PILOT-QPSK500`…`PILOT-32APSK500`, up to DVB-S2 32APSK) whose known in-band pilots drive carrier tracking instead of a decision-directed Costas loop — cycle-slip-immune on dense constellations and robust to soundcard sample-rate offset. |
| **LDPC belief propagation** | Real rate-1/2 min-sum belief propagation — not a stub. First open-source HF software modem with working LDPC. |
| **LLR-accumulating ARQ** | Soft LLR values accumulate across retransmissions (PACTOR-style Memory-ARQ), turning each retry into a soft combining gain. |
| **GPU-accelerated DSP** | 6 wgpu compute kernels (BPSK modulate/demodulate, timing search, RRC FIR matched filter, 256-pt FFT, soft LLR demod) accelerating BPSK, QPSK, 8PSK, 64QAM, and SC-FDMA — all with automatic CPU fallback. See [GPU-accelerated features](#gpu-accelerated-features). |
| **QSY frequency agility** | Ed25519-signed QSY_REQ/LIST/VOTE/ACK wire protocol. Initiator and responder roles wired into the daemon; rig CAT control via rigctld. |
| **FreeDV authenticated voice** | Ed25519-signed authentication beacons transmitted via the FreeDV Qt-GUI UDP data port (`openpulse-freedv-auth`); no FreeDV fork required. |
| **CE-SSB conditioning for data modes** | Controlled-Envelope SSB (Hershberger W9GR, QEX 2014) — a voice-SSB technique — applied as an adaptive, per-mode TX conditioner that raises average power at fixed PEP on high-PAPR multicarrier modes (OFDM/SC-FDMA). Confirmed **+1.18 dB on-air**, with software *and* on-air SDR spectral checks showing no added splatter on QPSK OFDM (dense HOM clean at normal data-mode drive). Believed to be the first open-source HF *data* modem to do this. |

On-air regulatory validation (spectral mask) has not been completed. Except for the
CE-SSB average-power confirmation above, all tests use loopback and simulated-channel
paths only.

---

## First-to-market features

Capabilities that are firsts or near-firsts among open-source amateur digital-mode software:

| # | Capability | Evidence / where to look |
|---|---|---|
| 1 | **Post-quantum in-band handshake** | ML-DSA-44 + ML-KEM-768 negotiated inside the ConReq/ConAck wire frames; Hybrid mode dual-signs with Ed25519 + ML-DSA-44 simultaneously (`crates/openpulse-core/src/pq_handshake.rs`) |
| 2 | **SC-FDMA (LTE uplink waveform) on HF** | DFT-spread OFDM with DFT-CE pilot-aided channel estimation and MMSE equalization (`plugins/scfdma`). DFT-spread is the low-PAPR-capable structure; realizing the full PAPR advantage over OFDM would need a non-interleaved pilot scheme. That redesign (old roadmap FF-14) was dropped in favour of the **OFDM higher-order ladder** as the HF high-throughput path — see [mode/FEC guide §7](docs/mode-fec-ladder.md) |
| 3 | **64QAM and SCFDMA-64QAM soft demodulation** | Gray-coded 64QAM max-log-MAP LLR demodulator; SCFDMA52-64QAM reaching 8 667 bps gross over a 2 kHz slice (`plugins/64qam`, `plugins/scfdma`) |
| 4 | **Working LDPC belief propagation** | Rate-1/2 min-sum BP — not a passthrough stub; wired into `transmit_with_ldpc` / `receive_with_ldpc` in the modem engine (`crates/openpulse-core/src/ldpc.rs`) |
| 5 | **LLR-accumulating Memory-ARQ** | Soft LLR values accumulated across retransmissions (PACTOR-style, `SoftCombiner` in `crates/openpulse-core/src/fec.rs`); HARQ retry/mode policy on sustained NACK (`crates/openpulse-modem/src/harq.rs`, `rate_policy.rs`) |
| 6 | **GPU DSP across 5 modulation families** | 6 wgpu WGSL kernels (BPSK modulate/demodulate, timing search, RRC FIR, 256-pt FFT, soft demod) accelerating BPSK, QPSK, 8PSK, 64QAM, and SC-FDMA, each with CPU fallback — see [GPU-accelerated features](#gpu-accelerated-features) |
| 7 | **Ed25519-signed QSY frequency agility** | Full initiator + responder state machines wired into the daemon; SNR-ranked channel-list negotiation; rig CAT via rigctld (`crates/openpulse-qsy`) |
| 8 | **Zstd pre-trained compression dictionary** | Dictionary trained on amateur/Winlink traffic patterns; negotiated at session setup and covered by handshake signature (`crates/openpulse-core/src/compression.rs`) |
| 9 | **Trust-weighted multi-hop relay with query propagation** | `RelayForwarder` enforces hop limits and suppresses duplicates; `score_route` weights paths by trust level (Verified=4 … Reduced=1); `QueryForwarder` propagates route-discovery requests across nodes (`crates/openpulse-core/src/relay.rs`, `query_propagation.rs`) |
| 10 | **Cross-band full-duplex repeater** | `CrossBandRepeater` runs in a daemon-managed thread; `EnableRepeater`/`DisableRepeater` control commands; trust-policy filtering on forwarded frames (`crates/openpulse-repeater`) |
| 11 | **Mesh broadcast daemon with authenticated beacons** | TTL-limited re-broadcast; (session_id, nonce) duplicate suppression; beacon payloads carry signed peer descriptors where the peer ID *is* the Ed25519 verifying key (`crates/openpulse-mesh`) |
| 12 | **FreeDV frame signing via codec2 data channel** | External shim adding Ed25519 per-frame signatures to FreeDV voice transmissions using the codec2 embedded data channel; no FreeDV fork required |
| 13 | **CE-SSB envelope conditioning for digital waveforms** | Hershberger's Controlled-Envelope SSB (QEX 2014) — long used in *voice* SSB (WDSP/Thetis) — applied here as a per-mode, default-on adaptive TX conditioner for high-PAPR multicarrier *data* modes (OFDM/SC-FDMA). Raises average TX power at fixed PEP: +1.6/+2.7/+3.8 dB in channel-sim (OFDM52 at 2.5/2.0/1.5×rms, zero BER cost) and **+1.18 dB confirmed on-air** (FT-991A, 2 m, 20 W via attenuator); software ACPR **and an on-air SDR spectral-mask check** (SDRplay RSP2pro) both confirm CE-SSB raises average power **without added splatter on QPSK OFDM**; on dense OFDM-HOM it stays clean at normal data-mode drive (the bigger average-power boost there only splatters if the PA's ALC is over-driven). Believed to be the first open-source HF data modem to apply CE-SSB (`crates/openpulse-dsp/src/cessb.rs`, `ModemEngine::cessb_benefits`) |

---

## Feature tables

### Modulation types

Sorted by occupied bandwidth (the mode-name number is the **baud** rate for
single-carrier modes; SC-FDMA/OFDM are named by data-subcarrier count and span
`total_SCs × 31.25 Hz`). The single-carrier modes also have `-RRC` (α = 0.35,
~+35 % bandwidth) and `-HF` tuning variants not all listed here. The `-RRC`
variants are the operational, carrier-offset-robust ones at 2000 baud: the plain
rectangular `QPSK2000`/`8PSK2000` are registered but **RRC-superseded** (their
crossfade pulse is ISI-limited at 4 samples/symbol — use `-RRC`).
The `PILOT-*` modes are a pilot-framed single-carrier family: known in-band pilot
symbols drive carrier recovery (cycle-slip-immune, sample-rate-offset-robust)
instead of a decision-directed Costas loop — see the
[pilot-framed waveform](docs/dev/hpx-waveform-design.md#pilot-framed-waveform) note.

| Mode | Plugin | Baud | Bits/sym | Gross&nbsp;bps | Occ.&nbsp;BW&nbsp;(Hz) | Waveform | Notes |
|---|---|---|---|---|---|---|---|
| BPSK31 | `bpsk` | 31.25 | 1 | 31 | ~50 | Single-carrier | Weak-signal narrowband HF |
| BPSK63 | `bpsk` | 62.5 | 1 | 63 | ~70 | Single-carrier | |
| BPSK100 | `bpsk` | 100 | 1 | 100 | ~110 | Single-carrier | |
| QPSK125 | `qpsk` | 125 | 2 | 250 | ~140 | Single-carrier | |
| BPSK250 | `bpsk` | 250 | 1 | 250 | ~275 | Single-carrier (+RRC) | |
| QPSK250 | `qpsk` | 250 | 2 | 500 | ~275 | Single-carrier | |
| FSK4-ACK | `fsk4` | 100 | 2 | 200 | ~400 | 4-FSK | ACK control channel only |
| QPSK500 | `qpsk` | 500 | 2 | 1&nbsp;000 | ~550 | Single-carrier (+RRC) | |
| 8PSK500 | `psk8` | 500 | 3 | 1&nbsp;500 | ~550 | Single-carrier (+RRC) | Gray-coded |
| 64QAM500 | `64qam` | 500 | 6 | 3&nbsp;000 | ~550 | Single-carrier | |
| PILOT-QPSK500 | `pilot` | 500 | 2 | 1&nbsp;000 | ~550 | Single-carrier (pilot-framed) | In-band pilots → cycle-slip-immune carrier tracking; SRO-robust |
| PILOT-8PSK500 | `pilot` | 500 | 3 | 1&nbsp;500 | ~550 | Single-carrier (pilot-framed) | Gray-coded; pilot-aided |
| PILOT-16QAM500 | `pilot` | 500 | 4 | 2&nbsp;000 | ~550 | Single-carrier (pilot-framed) | Pilot-amplitude-referenced demap |
| PILOT-32APSK500 | `pilot` | 500 | 5 | 2&nbsp;500 | ~550 | Single-carrier (pilot-framed) | DVB-S2 32APSK geometry |
| OFDM16 | `ofdm` | — | 2 | ~889 | ~625 | OFDM (16 SCs, QPSK) | LS + ZF; ≡ SCFDMA16 throughput |
| SCFDMA16 | `scfdma` | — | 2 | ~889 | ~625 | SC-FDMA (16 SCs, QPSK) | DFT-CE + MMSE |
| SCFDMA26-8PSK | `scfdma` | — | 3 | ~2&nbsp;167 | ~1&nbsp;000 | SC-FDMA (26 SCs, 8PSK) | Narrowband HOM (+3 dB/SC) |
| SCFDMA26-16QAM | `scfdma` | — | 4 | ~2&nbsp;889 | ~1&nbsp;000 | SC-FDMA (26 SCs, 16QAM) | Narrowband HOM (+3 dB/SC) |
| SCFDMA26-32QAM | `scfdma` | — | 5 | ~3&nbsp;611 | ~1&nbsp;000 | SC-FDMA (26 SCs, cross-32QAM) | Narrowband HOM (+3 dB/SC) |
| QPSK1000 | `qpsk` | 1&nbsp;000 | 2 | 2&nbsp;000 | ~1&nbsp;100 | Single-carrier (+RRC/HF) | |
| 8PSK1000 | `psk8` | 1&nbsp;000 | 3 | 3&nbsp;000 | ~1&nbsp;100 | Single-carrier (+RRC/HF) | |
| 64QAM1000 | `64qam` | 1&nbsp;000 | 6 | 6&nbsp;000 | ~1&nbsp;100 | Single-carrier | |
| OFDM52 | `ofdm` | — | 2 | ~2&nbsp;889 | ~2&nbsp;031 | OFDM (52 SCs, QPSK) | ≡ SCFDMA52 throughput; OFDM trades PAPR for per-SC EQ |
| OFDM52-8PSK | `ofdm` | — | 3 | ~4&nbsp;333 | ~2&nbsp;031 | OFDM (52 SCs, 8PSK) | OFDM higher-order ladder — the HF high-throughput path |
| OFDM52-16QAM | `ofdm` | — | 4 | ~5&nbsp;778 | ~2&nbsp;031 | OFDM (52 SCs, 16QAM) | OFDM higher-order ladder |
| OFDM52-32QAM | `ofdm` | — | 5 | ~7&nbsp;222 | ~2&nbsp;031 | OFDM (52 SCs, cross-32QAM) | OFDM higher-order ladder |
| OFDM52-64QAM | `ofdm` | — | 6 | ~8&nbsp;667 | ~2&nbsp;031 | OFDM (52 SCs, 64QAM) | OFDM higher-order ladder |
| SCFDMA52 | `scfdma` | — | 2 | ~2&nbsp;889 | ~2&nbsp;031 | SC-FDMA (52 SCs, QPSK) | Adaptive pilot density |
| SCFDMA52-8PSK | `scfdma` | — | 3 | ~4&nbsp;333 | ~2&nbsp;031 | SC-FDMA (52 SCs, 8PSK) | |
| SCFDMA52-16QAM | `scfdma` | — | 4 | ~5&nbsp;778 | ~2&nbsp;031 | SC-FDMA (52 SCs, 16QAM) | |
| SCFDMA52-32QAM | `scfdma` | — | 5 | ~7&nbsp;222 | ~2&nbsp;031 | SC-FDMA (52 SCs, cross-32QAM) | |
| SCFDMA52-64QAM | `scfdma` | — | 6 | ~8&nbsp;667 | ~2&nbsp;031 | SC-FDMA (52 SCs, 64QAM) | |
| SCFDMA52-64QAM-P4 | `scfdma` | — | 6 | ~8&nbsp;167 | ~2&nbsp;031 | SC-FDMA (49 SCs, dense pilots) | |
| QPSK2000-RRC | `qpsk` | 2&nbsp;000 | 2 | 4&nbsp;000 | ~2&nbsp;700 | Single-carrier + RRC | |
| 8PSK2000-RRC | `psk8` | 2&nbsp;000 | 3 | 6&nbsp;000 | ~2&nbsp;700 | Single-carrier + RRC | |
| 64QAM2000-RRC | `64qam` | 2&nbsp;000 | 6 | 12&nbsp;000 | ~2&nbsp;700 | Single-carrier + RRC | Requires SNR ≥ 25 dB |
| _QPSK9600-RRC_ | `qpsk` | 9&nbsp;600 | 2 | 19&nbsp;200 | ~13&nbsp;000 | Single-carrier + RRC | **Deferred (post-1.0)** — VHF/UHF, needs ≥38.4 kHz Fs |
| _8PSK9600-RRC_ | `psk8` | 9&nbsp;600 | 3 | 28&nbsp;800 | ~13&nbsp;000 | Single-carrier + RRC | **Deferred (post-1.0)** — VHF/UHF, needs ≥38.4 kHz Fs |

Each `PILOT-*` mode above also has a `-RRC` variant (~half the occupied bandwidth) and
`1000` / `2000-RRC` baud rungs — e.g. `PILOT-16QAM1000-RRC` (16QAM, 1000 baud, RRC) — all
selectable by name and surfaced by the `hpx_pilot{,_rrc,_fast,_fast_rrc}` profiles.

The mode/FEC selection ladder and which combinations are usable on HF is documented in [docs/mode-fec-ladder.md](docs/mode-fec-ladder.md).

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
| **LDPC high-rate (PEG)** | `openpulse-core` (ldpc.rs) | 8/9 (k=1024, n=1152) | Throughput on strong channels | Progressive Edge-Growth graph; soft-decision; auto-selected on dense high-SNR rungs (`FecMode::LdpcHighRate`) |
| **Turbo (rate-1/3 PCCC)** | `openpulse-core` (turbo.rs) | 1/3 | Near-capacity on AWGN | RSC K=3, 3GPP QPP interleaver K=40–6144, Max-Log-MAP BCJR, 8 iterations, CRC-16 early exit |
| **Concatenated RS + Conv** | `openpulse-core` | ~0.44 | Strong burst + AWGN | RS outer, Conv inner |
| **Short-block FEC** | `openpulse-core` | varies | ACK/control frames | For FSK4-ACK and small control payloads |

### GPU-accelerated features

All GPU functions return `Option<T>` — `None` triggers automatic CPU fallback. Gated by `#[cfg(feature = "gpu")]`; `--no-default-features` builds use CPU paths throughout.

| Feature | Kernel | Crate / file | Description |
|---|---|---|---|
| **BPSK modulation** | `bpsk_modulate.wgsl` | `openpulse-gpu` / `bpsk-plugin` | GPU symbol mapping and carrier generation (`bpsk_modulate_gpu`) |
| **BPSK IQ demodulation** | `bpsk_demodulate.wgsl` | `openpulse-gpu` / `bpsk-plugin` | Parallel IQ correlation across all sample offsets (`bpsk_iq_demod_gpu`) |
| **Timing-offset search** | `timing_search.wgsl` | `openpulse-gpu` / `bpsk-plugin` | Symbol-timing offset search via parallel energy integration |
| **RRC FIR matched filter** | `rrc_fir.wgsl` | `openpulse-gpu` | Causal RRC pulse-shaping/matched filter (`gpu_rrc_fir`); wired into the RRC paths of BPSK, QPSK, 8PSK, and 64QAM |
| **256-pt FFT / IFFT** | `fft256.wgsl` | `openpulse-gpu` | Cooley-Tukey radix-2 DIT; one workgroup per symbol; shared-memory in-place butterfly |
| **Batch FFT (SC-FDMA hard demod)** | `fft256.wgsl` (batched) | `scfdma-plugin` | All per-symbol FFTs in one `gpu_fft256_batch` call; CPU DFT-CE + MMSE + demap; covers the QPSK/8PSK/16QAM/32QAM/64QAM SC-FDMA variants |
| **Batch FFT (SC-FDMA soft demod)** | `fft256.wgsl` (batched) | `scfdma-plugin` | Same batch dispatch; CPU DFT-CE + MMSE + LLR; all SC-FDMA constellations |
| **64QAM soft demodulation** | `rrc_fir.wgsl` + `soft_demod.wgsl` | `64qam-plugin` | GPU RRC matched filter + batched max-log-MAP LLR (`gpu_soft_demod`) |
| **8PSK soft demodulation** | `rrc_fir.wgsl` + `soft_demod.wgsl` | `psk8-plugin` | GPU RRC matched filter + batched Gray-coded LLR (`gpu_soft_demod`) |

### Adaptive rate profiles

Eleven `SessionProfile` mappings from speed levels to modes, driven by ACK/NACK feedback
and per-level SNR floor/ceiling gates:

| Profile | SL range | Initial | Top mode | Target link |
|---|---|---|---|---|
| `hpx500` | SL2–SL6 | SL2 | QPSK500 | Robust narrowband (≤600 Hz) |
| `hpx_hf` | SL2–SL11 | SL2 | SCFDMA52-64QAM | Primary HF (full ≤2700 Hz span) |
| `hpx_ofdm_hf` | SL5–SL10 | SL5 | OFDM52-64QAM | HF OFDM higher-order ladder |
| `hpx_pilot` | SL2–SL5 | SL2 | PILOT-32APSK500 | HF pilot-aided (cycle-slip-immune, SRO-robust) |
| `hpx_pilot_rrc` | SL2–SL5 | SL2 | PILOT-32APSK500-RRC | Pilot, narrowband (RRC, ~half band) |
| `hpx_pilot_fast` | SL2–SL5 | SL2 | PILOT-32APSK1000 | Pilot, high-throughput (1000 baud) |
| `hpx_pilot_fast_rrc` | SL2–SL5 | SL2 | PILOT-32APSK1000-RRC | Pilot, fast + narrowband |
| `hpx_wideband` | SL8–SL11 | SL8 | 8PSK1000 | Wideband HF |
| `hpx_narrowband` | SL8–SL11 | SL8 | 8PSK2000-RRC | Narrowband HF / VHF |
| `hpx_narrowband_hd` | SL8–SL9 | SL8 | 8PSK9600-RRC | VHF/UHF narrowband |
| `hpx_wideband_hd` | SL9–SL15 | SL12 | 64QAM2000-RRC | VHF/UHF FM / satellite |

`hpx_wideband_hd` requires SNR ≥ 16 dB and is not suitable for HF ionospheric paths. The
four `hpx_pilot*` profiles share one carrier architecture and per-symbol SNR floors,
trading bandwidth (rect vs `-RRC`) against throughput (500 vs 1000 baud).

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

### Filtering and signal enhancement

Options that reduce out-of-band emissions, suppress spectral sidelobes, improve
receiver sensitivity, or raise transmit power efficiency.  Each can be selected
independently per mode.

| Technique | Where | Sidelobe / benefit | Notes |
|---|---|---|---|
| **CE-SSB envelope conditioning** (`openpulse_dsp::cessb`) | TX; high-PAPR multicarrier (OFDM/SC-FDMA), default-on | +1.6/+2.7/+3.8 dB avg power at fixed PEP (2.5/2.0/1.5×rms); negligible OOB regrowth | Look-ahead peak-stretcher; gated by `cessb_benefits` (no-op on single-carrier/BPSK); +1.18 dB confirmed on-air; panel "CE-SSB" toggle + `SetCessb` control |
| **Half-Hann overlapping crossfade** (`PulseShape::Hann`) | All single-carrier modes (default) | ~−32 dB first sidelobe | 50 % symbol overlap; no ISI at SNR > 3 dB; CPU path only |
| **Cosine overlap** (`PulseShape::CosineOverlap`) | Single-carrier alternative | ~−32 dB; null-to-null BW ≈ 2×Rs | Lower spectral leakage than rectangular; GPU-compatible |
| **Root Raised Cosine (SRRC) FIR** (`PulseShape::Rrc`) | `-RRC` mode suffix (QPSK, 8PSK, 64QAM) | ~−35 dB OOB; excess BW = 35 % | α = 0.35 rolloff; taps configurable; ISI-free by matched-filter design |
| **Barker-11 preamble** | Preamble / timing | PSL = −13 dB | 11-chip Barker; used for frame timing acquisition |
| **Barker-13 preamble** | Preamble / timing | PSL = −17 dB | 13-chip Barker; better sidelobe suppression than Barker-11 |
| **DFT-CE pilot-aided channel estimation** | SC-FDMA (all SCFDMA modes) | Removes multipath phase rotation | DFT-domain CE on pilot subcarriers; combines with MMSE |
| **MMSE equalization** | SC-FDMA | Suppresses inter-subcarrier interference | Per-subcarrier minimum mean-square error; requires DFT-CE |
| **LS channel estimation** | OFDM (OFDM16 / OFDM52) | Least-squares pilot tap estimation | Per-symbol LS CE → ZF equalization |
| **ZF equalization** | OFDM | Removes pilot-estimated channel distortion | Per-subcarrier zero-forcing; follows LS-CE |
| **LMS/DFE adaptive equalizer** | BPSK-RRC demod path | Residual ISI suppression | Supervised preamble training → decision-directed; `crates/openpulse-dsp` |
| **Gardner timing error detector** | All single-carrier modes | Symbol clock recovery | Symbol-rate TED; feeds symbol timing interpolator |
| **PLL carrier phase tracking** | BPSK / QPSK / 8PSK | Phase noise rejection | Phase-locked loop updated per symbol; `crates/openpulse-dsp` |
| **AFC IQ-squaring estimator** | BPSK (all rates) | Frequency offset correction ±baud/4 | Tracking range: ±7.8 Hz (BPSK31) … ±62.5 Hz (BPSK250) |
| **Soft-input FEC (LDPC / Turbo)** | Any mode with `supports_soft_demod()` | Coding gain vs. hard-decision FEC | Requires a plugin that returns genuine LLRs; engine warns if paired with hard-only plugin |

**Rectangular pulse spectrum** for reference: first sidelobes −13 dB; classical full-Hann windowing
improves this to −32 dB at the cost of ISI.  See [`docs/features.md`](docs/features.md) for the
detailed crossfade and ISI analysis.

### Operator interfaces

| Interface | Description |
|---|---|
| **Operator panel** (`openpulse-panel`) | Full egui/eframe GUI connecting to the daemon via TCP control port; mode selection, PTT, QSY management, CE-SSB toggle, tabbed Messages / Event-Log pane, live status |
| **Twin-station view** (`openpulse-twinview`) | egui both-directions viewer — one window over two daemons; per-station spectrum/waterfall + rate/OTA/HPX readouts, so both link directions show at once |
| **TUI** (`openpulse-tui`) | ratatui terminal UI — HPX state (colour-coded), AFC/rate meters, DCD energy bar, scrollable transitions log |
| **CLI** (`openpulse-cli`) | Full-featured command-line interface: transmit, receive, benchmark, monitor, config init, calibrate (audio/PTT/AFC) |
| **Signal testbench** (`openpulse-testbench`) | egui 4-column live view: TX / channel / mixed / RX; waterfall, spectrum, scatter; 7 channel models; SNR slider |

---

## Quick start

```bash
# Toolchain preflight (required: rustc >= 1.94.0)
./scripts/check-toolchain.sh

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

If you are temporarily pinned to an older Rust toolchain, run the fallback core gates to keep CI-relevant coverage for all non-PKI crates:

```bash
cargo clippy --workspace --exclude pki-tooling --no-default-features -- -D warnings
cargo test --workspace --exclude pki-tooling --no-default-features
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
| `crates/openpulse-dsp` | RRC filter, PLL, Gardner timing recovery, LMS/DFE adaptive equalizer, CE-SSB envelope conditioner |
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
| `apps/openpulse-twinview` | egui both-directions viewer over two daemons (twin-station rig) |
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

## Non-GPL interfacing

OpenPulseHF is GPL v3, but several interfaces let non-GPL software interoperate without
GPL obligations.  All of the following sit behind a **process boundary** or a **network
protocol boundary**, which the FSF and courts have consistently held does not trigger
copyleft:

| Interface | Transport | Description |
|---|---|---|
| **ARDOP TNC** | TCP 8515 (cmd) / 8516 (data) | ARDOP ASCII command protocol; Pat, Winlink Express, JS8Call–compatible |
| **KISS/AX.25 TNC** | TCP 8100 | Standard KISS framing; compatible with Xastir, YAAC, APRX, Linux AX.25 tools |
| **Daemon control port** | NDJSON over TCP 9000 | Full event stream + control commands; operator panel and scripting |
| **Daemon WebSocket** | JSON over WS 9001 | Same protocol as TCP port; browser and Electron clients |
| **PKI REST API** | HTTP/JSON 8080 | Trust-bundle and key-management endpoints; read-only routes unauthenticated |
| **CLI subprocess** | stdin / stdout | NDJSON output; invocation crosses the process boundary |
| **Winlink CMS gateway** | B2F over TCP 8772 | Outbound-only client to `cms.winlink.org`; CMS is proprietary |

Note: **plugins that statically link against `openpulse-core`** are derivative works
and must be GPL-compatible.  The only supported path for proprietary DSP backends is to
run them as a separate process and bridge data through one of the TCP interfaces above.

See [`docs/non-gpl-interfacing.md`](docs/non-gpl-interfacing.md) for the full interface
specifications including wire formats, authentication, and schema references.

---

## License

GNU General Public License v3.0 or later — see [LICENSE](LICENSE).
