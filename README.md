---
project: openpulsehf
doc: README.md
status: living
last_updated: 2026-10-15
---

# OpenPulseHF

A plugin-based HF/VHF software modem and protocol stack written in Rust.

OpenPulseHF turns an SSB transceiver and a sound interface into an adaptive data link. The first
release (Release 1, `v0.17.0`) is deliberately narrow: one adaptive rate ladder (`hpx_hf`, SL1–SL14)
with forward error correction on every rung, session compression, the background daemon and CLI,
and an ARDOP-compatible TNC for Pat (Winlink). The workspace also contains a much larger stack —
KISS/AX.25, Winlink B2F and a CMS gateway, QSY, mesh, relay, repeater, JS8-based discovery, file
transfer, a post-quantum handshake, GPU kernels and GUIs — which is **not part of Release 1** and
ships disabled or experimental. The plugin architecture lets modulation modes be added without
touching the core modem engine. All tests run against a deterministic loopback backend; no audio
hardware is required to build or test.

[![CI](https://github.com/dc0sk/OpenPulseHF/actions/workflows/ci.yml/badge.svg)](https://github.com/dc0sk/OpenPulseHF/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/dc0sk/OpenPulseHF?label=release)](https://github.com/dc0sk/OpenPulseHF/releases/latest)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)
[![Donate via PayPal](https://img.shields.io/badge/Donate-PayPal-blue.svg?logo=paypal)](https://www.paypal.com/donate/?hosted_button_id=WY9U4MQ3ZAQWC)

**Author:** Simon Keimer · [DC0SK](https://github.com/dc0sk)

<img src="docs/OpenPulseHF.png" alt="OpenPulseHF QR code" width="180">

---
## Supporters

Big thanks to:

DD2ZM for providing access to a remote controllable station for testing.

DB1IUA for helping stuffing the budget sink-hole.

Notfunk Deutschland e.V. for providing space on their booth at HAM RADIO 2026.

---
## Status

**Latest release: v0.16.0 (2026-07-18)** — [release notes](https://github.com/dc0sk/OpenPulseHF/releases/tag/v0.16.0).
A documentation and test-integrity release: it shipped [the OpenPulseHF book](docs/openpulse-book.md)
and a rewritten [operator manual](docs/openpulse-manual.md), and replaced acceptance tests that could
not fail with real AWGN round-trips. The modem itself was unchanged from v0.15.0.

**In progress: Release 1 = `v0.17.0`** (not yet tagged; `main` still reports version 0.16.0). The plan,
milestones and decisions are in [`docs/dev/project/workplan.md`](docs/dev/project/workplan.md);
`1.0` stays reserved for the full [1.0 criteria](docs/dev/project/release-1.0-criteria.md).

- **In Release 1:** `openpulse-daemon` (binary `openpulse-server`), `openpulse-cli` (binary
  `openpulse`) and the ARDOP TNC (`openpulse-ardop`, binary `openpulse-tnc`); the `hpx_hf` ladder
  SL1–SL14 and its plugins (MFSK16 with its return channel, BPSK, QPSK250-D, OFDM52, FSK4 ACK);
  each rung's FEC; session compression; PTT backends; station ID; the signed classical (Ed25519)
  handshake.
- **Not in Release 1** (still in the code; ships disabled or experimental, with no release gate):
  mesh, repeater, relay, discovery/JS8, file transfer, QSY, FreeDV auth, Winlink B2F and the CMS
  gateway, KISS, the post-quantum handshake, GPU acceleration, the operator panel and TUI, and the
  plugins that are not on `hpx_hf` (`psk8`, `64qam`, `scfdma`, `pilot`).
- **Where it stands (2026-10-15):** M0 (governance reset) and M1 (v0.17 wire format, 2026-10-01) are
  done. M2 (release path) is done in code as of 2026-10-14; its on-air follow-ups (the PTT leader
  measured at gate G1, a Raspberry Pi receive-cost re-measure) wait for the radios. **Next is M3: on
  air on 2 m** (IC-9700 ↔ FT-818, 144.640 MHz) — gates G0–G3, the A1/A2/A4/A5 runs, a Pat session
  over the ARDOP TNC. HF follows only with the release candidate. M4 cuts `v0.17.0`.
- **No interoperability with v0.16.0.** A v0.16.0 station and a current `main` station do not
  decode each other's frames on any rung, and their handshakes are incompatible. The breaks came in
  the planned pre-1.0 window: every frame is now whitened (added in #1027 after v0.16.0, keystream
  fixed in #1148), the handshake became a binary format (#1147), and since 2026-10-10 BPSK63, BPSK100 and BPSK250 (SL3–SL5) send a
  PN-63 preamble (#1062). Update both ends of a link together.
- **Profiles changed.** There are now exactly two session profiles, `fast` and `robust` (see
  [Adaptive rate profiles](#adaptive-rate-profiles)). The ten earlier profile names were deleted with
  no aliases; an unknown name is rejected with the list of valid ones.

What changed since v0.16.0, for operators: the *Unreleased* section of
[`docs/releasenotes.md`](docs/releasenotes.md).

**Earlier releases** (full notes in [`docs/releasenotes.md`](docs/releasenotes.md)):

- [v0.15.0](https://github.com/dc0sk/OpenPulseHF/releases/tag/v0.15.0) (2026-07-18) — Winlink stack
  hardening: one live DoS fixed, an aggregate decompression cap, per-operation read deadlines.
- [v0.4.0](https://github.com/dc0sk/OpenPulseHF/releases/tag/v0.4.0) (2026-07-12) — JS8-based station
  discovery and rendezvous, signed P2P file transfer, OFDM re-seat and HARQ soft-combining.
- [v0.3.0](https://github.com/dc0sk/OpenPulseHF/releases/tag/v0.3.0) (2026-06-29) — signed handshake
  over the air on connect, automatic ADIF logbook, opt-in ARDOP adaptive ARQ, generic serial CAT.
- [v0.2.1](https://github.com/dc0sk/OpenPulseHF/releases/tag/v0.2.1) (2026-06-24) — CE-SSB transmit
  envelope conditioning for the high-PAPR multicarrier modes.

---

## Notable capabilities

Only the first three rows are part of Release 1. The others are implemented and tested in
simulation, but ship disabled or experimental.

| Capability | Release 1 | What it is / where to look |
|---|---|---|
| **One adaptive ladder, coded on every rung** | yes | `hpx_hf`, SL1–SL14: MFSK16 → coded BPSK → differential QPSK250-D → OFDM52 up to 64QAM with LDPC r≈8/9, ~9 bps to 7.7 kbps. ACK/NACK- and SNR-driven (`crates/openpulse-core/src/profile.rs`, [mode/FEC guide](docs/mode-fec-ladder.md)) |
| **LLR-accumulating ARQ** | yes | Soft LLRs accumulate across retransmissions (PACTOR-style Memory-ARQ): `SoftCombiner` in `crates/openpulse-core/src/fec.rs`, retry/mode policy in `crates/openpulse-modem/src/harq.rs` and `rate_policy.rs` |
| **CE-SSB envelope conditioning** | yes (default on) | Controlled-Envelope SSB (Hershberger W9GR, QEX 2014) as a per-mode TX conditioner on the QPSK-subcarrier OFDM modes (OFDM52 on the ladder; the denser OFDM and all SC-FDMA modes are excluded). +1.18 dB average power measured on air at fixed PEP (FT-991A, 2 m) (`crates/openpulse-dsp/src/cessb.rs`, `ModemEngine::cessb_benefits`) |
| **Post-quantum handshake** | no | ML-DSA-44 + ML-KEM-768 in the ConReq/ConAck frames; Hybrid mode signs with Ed25519 and ML-DSA-44 (`crates/openpulse-core/src/pq_handshake.rs`) |
| **SC-FDMA on HF** | no | DFT-spread OFDM with DFT-CE pilot channel estimation and MMSE equalisation (`plugins/scfdma`). The frequency-interleaved pilots limit the realised PAPR advantage; OFDM, not SC-FDMA, is the high-throughput path — see [mode/FEC guide §7](docs/mode-fec-ladder.md) |
| **Pilot-framed single carrier** | no | `PILOT-*` modes up to DVB-S2 32APSK: in-band pilots drive carrier tracking instead of a decision-directed loop (`plugins/pilot`) |
| **GPU DSP** | no | Six wgpu kernels (BPSK modulate/demodulate, timing search, RRC FIR, 256-pt FFT, soft demod) with CPU fallback — see [GPU-accelerated features](#gpu-accelerated-features) |
| **QSY frequency agility** | no | Ed25519-signed QSY_REQ/LIST/VOTE/ACK protocol, verified against the handshake peer (#1252); initiator and responder in the daemon; rig CAT via rigctld (`crates/openpulse-qsy`) |
| **Relay, mesh, repeater** | no | Trust-weighted multi-hop relay with query propagation (`relay.rs`, `query_propagation.rs`); mesh re-broadcast with authenticated beacons (`crates/openpulse-mesh`); cross-band repeater (`crates/openpulse-repeater`) |
| **JS8-based discovery + rendezvous** | no | A native JS8 waveform (8-GFSK, LDPC(174,87)) to find OpenPulse stations on the JS8 calling frequency and negotiate a working frequency; RX-only by default (`plugins/js8`, `crates/openpulse-discovery`) |
| **Signed P2P file transfer** | no | Offer/accept transfer with an inline signed `TransferManifest` + SHA-256 checked against the peer's handshake key; a failed check quarantines the file (`crates/openpulse-filexfer`) |
| **FreeDV authentication** | no | Ed25519-signed beacons for FreeDV voice via the FreeDV GUI's UDP data port; no FreeDV fork (`crates/openpulse-freedv-auth`) |

On-air status: a first rig-to-rig decode on 2 m passed on 2026-07-28 (phase A1, on the pre-v0.17
format), and the CE-SSB power and spectrum checks above were made on real RF. The Release 1 on-air
campaign (M3) has not run yet. Fade performance on HF is validated in the Watterson simulator only.

---

## Feature tables

### Adaptive rate profiles

Two session profiles, both on the single `hpx_hf` rate ladder, driven by ACK/NACK feedback
and per-level SNR floor/ceiling gates:

| Profile | SL range | Top mode | Occupied BW | Use when |
|---|---|---|---|---|
| `fast` (default) | SL1–SL14 | OFDM52-64QAM + LDPC r≈8/9 | up to ≈2031 Hz | Good conditions, a 2.4 kHz SSB filter, a linear PA |
| `robust` | SL1–SL6 | QPSK250-D | ≤500 Hz | Poor conditions or limited gear (narrow filters, small or non-linear PAs); single-carrier, all coded |

`robust` is the same ladder capped at SL6, so a `fast` and a `robust` station interoperate; the
robust side never climbs past SL6. `hpx_hf` is now `fast`; the ten other earlier profile names (`hpx500`, `hpx_modcod`,
`hpx_pilot*`, `hpx_ofdm_hf`, `hpx_wideband*`, `hpx_narrowband`, …) were deleted on 2026-10-01 with no
aliases; any other name is rejected with the list of valid names. Modes from the earlier profiles
remain selectable as fixed modes (`[modem] mode`). The ARDOP TNC floors its adaptive ladder at SL2
(it has no MFSK16).

Adaptive operation is opt-in on both front-ends: the daemon runs the ladder when
`[modem] ota_enabled = true` (default `false`, fixed mode `BPSK250`), and the ARDOP TNC when
`[ardop] enable_adaptive_arq = true`.

<a id="modulation-types"></a>

### The `hpx_hf` ladder (Release 1)

| SL | Mode | Plugin | FEC | Net bps | SNR floor (dB) | Declared BW (Hz) |
|---|---|---|---|---|---|---|
| 1 | MFSK16 | `mfsk16` | RS | ~9 | none (sub-floor deep-fade rung) | 500 |
| 2 | BPSK31 | `bpsk` | RS | 27 | 3 (initial level) | 62.5 |
| 3 | BPSK63 | `bpsk` | RS | 54 | 4 | 125 |
| 4 | BPSK100 | `bpsk` | RS | 87 | 4.5 | 200 |
| 5 | BPSK250 | `bpsk` | RS | 219 | 5 | 500 |
| 6 | QPSK250-D | `qpsk` | RS | 437 | 7 | 500 |
| 7 | OFDM52 | `ofdm` | soft-concatenated | 1264 | 9 | 2031 |
| 8 | OFDM52-8PSK | `ofdm` | soft-concatenated | 1895 | 10 | 2031 |
| 9 | OFDM52-16QAM | `ofdm` | soft-concatenated | 2527 | 12 | 2031 |
| 10 | OFDM52-32QAM | `ofdm` | soft-concatenated | 3159 | 14 | 2031 |
| 11 | OFDM52-64QAM | `ofdm` | soft-concatenated | 3790 | 16 | 2031 |
| 12 | OFDM52-16QAM | `ofdm` | LDPC high-rate (r≈8/9) | 5141 | 18 | 2031 |
| 13 | OFDM52-32QAM | `ofdm` | LDPC high-rate | 6426 | 19 | 2031 |
| 14 | OFDM52-64QAM | `ofdm` | LDPC high-rate | 7710 | 20 | 2031 |

The BPSK rungs carry RS(255,223), upgraded per frame to RS(255,191) where that costs no extra RS
block. ACKs go out on FSK4-ACK (`fsk4`, 300 Hz) or, when the receiver recommends SL1, on the MFSK16
return channel. "Declared BW" is what the plugin reports and what ARDOP `ARQBW` sizes against; for the
single-carrier modes it is a conservative 2 × baud. Preambles: BPSK31 sends a 32-symbol `--++`
run, BPSK63/100/250 a PN-63 sequence (x⁶+x+1), QPSK250-D its designed 16-symbol sequence, MFSK16 a
7-symbol Costas sync, OFDM52 a Schmidl-Cox symbol. Full rationale: [docs/mode-fec-ladder.md](docs/mode-fec-ladder.md).

### Other modulation modes (not part of Release 1)

These are registered and selectable as fixed modes (`[modem] mode`), but are not on the Release 1
ladder and carry no release gate. `openpulse modes` prints the live registry. Mode-name numbers are
the **baud** rate for single-carrier modes; SC-FDMA/OFDM modes are named by data-subcarrier count and
span `total_SCs × 31.25 Hz`. Bandwidths for single-carrier modes are approximate.

The single-carrier modes also have `-RRC` (α = 0.35) and `-HF` tuning variants not all listed here.
At 2000 baud use the `-RRC` variants: plain rectangular `QPSK2000` is registered but RRC-superseded,
and `8PSK2000` is no longer advertised (#1359), because the crossfade pulse is ISI-limited at 4
samples/symbol. The `PILOT-*` modes are a pilot-framed single-carrier family — see the
[pilot-framed waveform](docs/dev/design/hpx-waveform-design.md#pilot-framed-waveform) note.

| Mode | Plugin | Baud | Bits/sym | Gross&nbsp;bps | Approx.&nbsp;BW&nbsp;(Hz) | Waveform | Notes |
|---|---|---|---|---|---|---|---|
| BPSK250-RRC | `bpsk` | 250 | 1 | 250 | ~340 | Single-carrier + RRC | Off-ladder; keeps the `--++` preamble |
| QPSK125 | `qpsk` | 125 | 2 | 250 | ~140 | Single-carrier | |
| QPSK250 | `qpsk` | 250 | 2 | 500 | ~275 | Single-carrier | Coherent; the ladder uses the differential QPSK250-D |
| QPSK500 / QPSK500-D | `qpsk` | 500 | 2 | 1&nbsp;000 | ~550 | Single-carrier (+RRC) | `-D` = differential |
| 8PSK500 | `psk8` | 500 | 3 | 1&nbsp;500 | ~550 | Single-carrier (+RRC) | Gray-coded |
| 64QAM500 | `64qam` | 500 | 6 | 3&nbsp;000 | ~550 | Single-carrier | |
| PILOT-QPSK500 | `pilot` | 500 | 2 | 1&nbsp;000 | ~550 | Single-carrier (pilot-framed) | Pilot-aided carrier tracking |
| PILOT-8PSK500 | `pilot` | 500 | 3 | 1&nbsp;500 | ~550 | Single-carrier (pilot-framed) | Gray-coded |
| PILOT-16QAM500 | `pilot` | 500 | 4 | 2&nbsp;000 | ~550 | Single-carrier (pilot-framed) | Pilot-amplitude-referenced demap |
| PILOT-32APSK500 | `pilot` | 500 | 5 | 2&nbsp;500 | ~550 | Single-carrier (pilot-framed) | DVB-S2 32APSK geometry |
| OFDM16 | `ofdm` | — | 2 | ~889 | ~625 | OFDM (16 SCs, QPSK) | LS + ZF |
| SCFDMA16 | `scfdma` | — | 2 | ~889 | ~625 | SC-FDMA (16 SCs, QPSK) | DFT-CE + MMSE |
| SCFDMA26-8PSK | `scfdma` | — | 3 | ~2&nbsp;167 | ~1&nbsp;000 | SC-FDMA (26 SCs, 8PSK) | Narrowband HOM |
| SCFDMA26-16QAM | `scfdma` | — | 4 | ~2&nbsp;889 | ~1&nbsp;000 | SC-FDMA (26 SCs, 16QAM) | Narrowband HOM |
| SCFDMA26-32QAM | `scfdma` | — | 5 | ~3&nbsp;611 | ~1&nbsp;000 | SC-FDMA (26 SCs, cross-32QAM) | Narrowband HOM |
| QPSK1000 | `qpsk` | 1&nbsp;000 | 2 | 2&nbsp;000 | ~1&nbsp;100 | Single-carrier (+RRC/HF) | |
| 8PSK1000 | `psk8` | 1&nbsp;000 | 3 | 3&nbsp;000 | ~1&nbsp;100 | Single-carrier (+RRC/HF) | |
| 64QAM1000 | `64qam` | 1&nbsp;000 | 6 | 6&nbsp;000 | ~1&nbsp;100 | Single-carrier | |
| SCFDMA52 | `scfdma` | — | 2 | ~2&nbsp;889 | ~2&nbsp;031 | SC-FDMA (52 SCs, QPSK) | Adaptive pilot density |
| SCFDMA52-P2 | `scfdma` | — | 2 | ~2&nbsp;889 | ~2&nbsp;031 | SC-FDMA (52 SCs, PN pilots) | Low-PAPR demonstrator (Zadoff-Chu-phase pilots, ~2.15 dB lower PAPR, full DFT-CE) |
| SCFDMA52-LP | `scfdma` | — | 2 | ~3&nbsp;389 | ~2&nbsp;031 | SC-FDMA (61 SCs, localized) | Low-PAPR demonstrator; flat-channel CE only, mis-decodes on selective channels |
| SCFDMA52-8PSK | `scfdma` | — | 3 | ~4&nbsp;333 | ~2&nbsp;031 | SC-FDMA (52 SCs, 8PSK) | |
| SCFDMA52-16QAM | `scfdma` | — | 4 | ~5&nbsp;778 | ~2&nbsp;031 | SC-FDMA (52 SCs, 16QAM) | |
| SCFDMA52-32QAM | `scfdma` | — | 5 | ~7&nbsp;222 | ~2&nbsp;031 | SC-FDMA (52 SCs, cross-32QAM) | |
| SCFDMA52-64QAM | `scfdma` | — | 6 | ~8&nbsp;667 | ~2&nbsp;031 | SC-FDMA (52 SCs, 64QAM) | |
| SCFDMA52-64QAM-P4 | `scfdma` | — | 6 | ~8&nbsp;167 | ~2&nbsp;031 | SC-FDMA (49 SCs, dense pilots) | |
| QPSK2000-RRC | `qpsk` | 2&nbsp;000 | 2 | 4&nbsp;000 | ~2&nbsp;700 | Single-carrier + RRC | |
| 8PSK2000-RRC | `psk8` | 2&nbsp;000 | 3 | 6&nbsp;000 | ~2&nbsp;700 | Single-carrier + RRC | |
| 64QAM2000-RRC | `64qam` | 2&nbsp;000 | 6 | 12&nbsp;000 | ~2&nbsp;700 | Single-carrier + RRC | Needs a very clean channel |
| JS8-SLOW … JS8-ULTRA | `js8` | — | — | — | ~50 (NORMAL) | 8-GFSK | Discovery only; RX-only by default |

Each `PILOT-*` mode also has a `-RRC` variant and `1000` / `2000-RRC` baud rungs (e.g.
`PILOT-16QAM1000-RRC`). The 9600-baud `QPSK9600-RRC` / `8PSK9600-RRC` waveforms keep their code
but are not advertised: they need ≥ 38.4 kHz audio and the engine runs at 8 kHz (post-1.0).

### MAC / channel access types

| Mechanism | Where used | Release 1 | Description |
|---|---|---|---|
| **DCD / carrier detect** | `openpulse-core` (dcd.rs), `openpulse-modem` | yes | Energy gate with hold window (default 100 ms); since #1452 the noise floor follows the band behind any receive filter, and a spectral busy criterion (#1454) catches weak frames |
| **0.3-persistence CSMA** | `openpulse-modem` | yes | Transmit deferred when the channel is busy; configurable per `ModemEngine` |
| **HPX adaptive session** | `openpulse-core` (hpx.rs, rate.rs) | yes | ACK/NACK-driven speed-ladder state machine; `RateAdapter` with per-level SNR gates and NACK-decrement hysteresis |
| **ARQ retry loop** | `openpulse-modem` (`harq.rs` + `rate_policy.rs`) | yes | LLR-accumulating retransmission; mode switching on sustained NACK |
| **QSY frequency agility** | `openpulse-qsy` | no | SNR-ranked channel-list negotiation (QSY_REQ → LIST → VOTE/ACK) |
| **Cross-band repeater** | `openpulse-repeater` | no | Full-duplex digipeater with trust-policy filter; `EnableRepeater`/`DisableRepeater` daemon commands |
| **Mesh re-broadcast** | `openpulse-mesh` | no | TTL-limited beacon re-broadcast with (session_id, nonce) duplicate suppression |
| **Multi-hop relay** | `openpulse-core` (relay.rs) | no | `RelayForwarder` with trust-weighted path scoring and hop limits |

### Compression types

| Algorithm | Layer | Release 1 | Notes |
|---|---|---|---|
| **LZ4** | Session (in-band) | yes | `lz4_flex`; applied above FEC |
| **Zstd + HPX dictionary** | Session (in-band) | yes | Pre-trained dictionary; zstd's own frame header carries the dictionary id |
| **None** | Session (in-band) | yes | Payloads that are already compressed |
| **Gzip** | B2F wire (Type D) | no | `flate2`; Winlink Type D proposal |

Session compression is opt-in (`[compression] enabled`, default `false`) and configured locally,
not negotiated in the handshake (that field was removed in #1166 because nothing consumed it). A
packed frame is self-describing, so the receiver always unpacks it and one station can enable
compression on its own. A packed frame that fails to unpack is an integrity error: dropped, counted
and logged (REQ-CMP-05).

### ARQ types

| Type | Crate / module | Description |
|---|---|---|
| **Stop-and-wait ARQ** | `openpulse-modem` (engine.rs) | Per-frame ACK; NACK triggers retransmit |
| **LLR-accumulating ARQ (Memory-ARQ)** | `openpulse-modem` (`harq.rs` + `rate_policy.rs`) | Soft LLRs accumulated across retransmissions; mode switch on sustained NACK |
| **SAR (Segmentation and Reassembly)** | `openpulse-core` (sar.rs) | 4-byte header (segment_id, fragment_index, fragment_total); max 64 005 bytes per segment; duplicate-idempotent |
| **QSY ACK** (not in Release 1) | `openpulse-qsy` | Ed25519-signed ACK/REJECT frames completing the QSY negotiation |

### Error correction types

| Algorithm | Crate / module | Code rate | Notes |
|---|---|---|---|
| **Reed-Solomon RS(255,223)** (`Rs`) | `openpulse-core` (fec.rs) | 223/255 ≈ 0.87 | Corrects 16 byte errors per block; SL1–SL6 |
| **Reed-Solomon RS(255,191)** (`RsStrong`) | `openpulse-core` (fec.rs) | 191/255 ≈ 0.75 | Corrects 32 byte errors per block; per-frame upgrade on the BPSK rungs where it costs no extra block |
| **Block interleaver** (`RsInterleaved`) | `openpulse-core` (fec.rs) | 1.0 | Stride interleaver over RS blocks |
| **Soft-concatenated** (`SoftConcatenated`) | `openpulse-core` | ≈0.44 | K=7 convolutional code with soft Viterbi inside, RS(255,223) outside; SL7–SL11 |
| **LDPC high-rate** (`LdpcHighRate`) | `openpulse-core` (ldpc.rs) | ≈8/9 (k=1024, n=1152) | PEG graph, soft-decision; SL12–SL14 |
| **LDPC rate-1/2** (`Ldpc`) | `openpulse-core` (ldpc.rs) | 1/2 | Min-sum belief propagation; not on the ladder |
| **Concatenated** (`Concatenated`) | `openpulse-core` | ≈0.44 | Hard-decision Conv(rate-1/2) inner + RS outer; not on the ladder |
| **Turbo** (`Turbo`) | `openpulse-core` (turbo.rs) | 1/3 | PCCC, 3GPP QPP interleaver, Max-Log-MAP, 8 iterations; not on the ladder |
| **Short-block RS** (`ShortRs`) | `openpulse-core` (fec.rs) | varies | t=4, 8 ECC bytes; ACK and control frames |
| **Convolutional K=3** | `openpulse-core` (conv.rs) | 1/2 | G={7,5}, hard-decision Viterbi; standalone codec |

### GPU-accelerated features

Not part of Release 1. All GPU functions return `Option<T>`; `None` falls back to the CPU path. The code is behind the
`gpu` feature. `openpulse-server` and `openpulse-linksim` enable it by default and use a GPU when an
adapter is found; CI and the test gates build with `--no-default-features` and use CPU paths only.

| Feature | Kernel | Crate / file | Description |
|---|---|---|---|
| **BPSK modulation** | `bpsk_modulate.wgsl` | `openpulse-gpu` / `bpsk-plugin` | Symbol mapping and carrier generation |
| **BPSK IQ demodulation** | `bpsk_demodulate.wgsl` | `openpulse-gpu` / `bpsk-plugin` | Parallel IQ correlation across sample offsets |
| **Timing-offset search** | `timing_search.wgsl` | `openpulse-gpu` / `bpsk-plugin` | Symbol-timing search via parallel energy integration |
| **RRC FIR matched filter** | `rrc_fir.wgsl` | `openpulse-gpu` | RRC paths of BPSK, QPSK, 8PSK and 64QAM |
| **256-pt FFT / IFFT** | `fft256.wgsl` | `openpulse-gpu` | Radix-2 DIT; one workgroup per symbol |
| **Batch FFT (SC-FDMA)** | `fft256.wgsl` (batched) | `scfdma-plugin` | All per-symbol FFTs in one call; CPU DFT-CE + MMSE + demap/LLR |
| **64QAM / 8PSK soft demodulation** | `rrc_fir.wgsl` + `soft_demod.wgsl` | `64qam-plugin`, `psk8-plugin` | GPU matched filter + batched max-log-MAP LLR |

### Protocol and interfaces

- **ARDOP-compatible TCP TNC** (`openpulse-tnc`, Release 1) — the command set Pat uses, including
  GRIDSQUARE, ARQBW, ARQTIMEOUT, CWID, SENDID, PING. Receives on real audio (#1310); host blocks
  larger than one frame are split into frames (#1385); `ARQBW` sizes modes from the plugin.
- **KISS/AX.25 TNC** (`openpulse-kisstnc`, not in Release 1) — KISS byte stuffing, AX.25 UI frames.
- **B2F/Winlink** (not in Release 1) — ISS and IRS roles, FC/FS/Ff/Fq frames, Gzip (Type D);
  Type C/LZHUF is unsupported and inbound Type C proposals are rejected.
- **Direct TCP Winlink CMS gateway** (`openpulse-gateway`, not in Release 1).
- **QSY, mesh, cross-band repeater, multi-hop relay** (not in Release 1) — see the tables above.

### Security and identity

- **Ed25519** signed handshake (ConReq/ConAck, Release 1) and transfer-manifest signing.
- **Station ID** (Release 1) — periodic identification for regulatory compliance.
- **Three trust policies**: Permissive, Balanced, Strict.
- **Signed peer descriptors** — the peer ID is the verifying key.
- Not in Release 1: the **ML-DSA-44 + ML-KEM-768** post-quantum handshake (Hybrid and PQ-only
  modes), the **PKI service** (`pki-tooling`, trust-bundle signing with PostgreSQL), and **FreeDV
  authentication** (`openpulse-freedv-auth`).

### Channel simulation

- **Watterson HF fading** — Good F1/F2, Moderate, Poor profiles; Doppler-shaping filter
- **Gilbert-Elliott burst error** — configurable state machine; AWGN and burst modes
- **QRN/QRM/QSB/Chirp** — broadband noise, interference, slow fading, frequency drift

### Filtering and signal enhancement

**Spectral** sidelobes (out-of-band emissions, shaped by the pulse) and **autocorrelation**
sidelobes (the peak-sidelobe ratio of a sync sequence, which governs onset placement) are different
things: a pulse shape does nothing for the second and a sync sequence does nothing for the first.
Pulse-shape and FEC rows are per-mode; channel estimation and equalisation are intrinsic to their
waveforms; preambles are fixed per mode and cannot be selected.

| Technique | Where | Benefit | Notes |
|---|---|---|---|
| **CE-SSB envelope conditioning** (`openpulse_dsp::cessb`) | TX; OFDM16 and OFDM52 only (default on) | +1.6/+2.7/+3.8 dB average power at fixed PEP (2.5/2.0/1.5×rms, channel sim) | Look-ahead peak stretcher; gated by `cessb_benefits`; +1.18 dB measured on air; `[modem] cessb_enabled`, `SetCessb` control |
| **Half-Hann overlapping crossfade** (`PulseShape::Hann`) | Single-carrier modes (default) | ~−32 dB first sidelobe | 50 % symbol overlap |
| **Cosine overlap** (`PulseShape::CosineOverlap`) | Single-carrier alternative | ~−32 dB | GPU-compatible |
| **Root Raised Cosine** (`PulseShape::Rrc`) | `-RRC` modes | ~−35 dB OOB | α = 0.35 |
| **Preambles** | Fixed per mode | Onset placement | BPSK31 and BPSK250-RRC: 32-symbol `--++`. BPSK63/100/250: PN-63 (x⁶+x+1) since #1062. QPSK/8PSK/64QAM: designed 16-symbol sequence. MFSK16: Costas sync. OFDM: Schmidl-Cox symbol. Pilot: PN-63 cycled to 48 chips |
| **Sync-sequence library** (`openpulse_dsp::preamble`) | `bpsk`, `pilot` | Barker-11/13, PN-31/63, Zadoff-Chu-64 | No shipping mode uses Barker or ZC; `PreambleDetector` has no production caller |
| **DFT-CE + MMSE** | SC-FDMA | Multipath phase rotation, inter-subcarrier interference | Intrinsic to SC-FDMA |
| **LS channel estimation + ZF** | OFDM | Per-subcarrier equalisation | Intrinsic to OFDM |
| **LMS/DFE adaptive equaliser** | BPSK-RRC demod path | Residual ISI | `crates/openpulse-dsp` |
| **Gardner timing recovery** | BPSK, QPSK, 8PSK, 64QAM | Symbol clock recovery | Not the PILOT family, which times from its pilots |
| **PLL carrier phase tracking** | BPSK / QPSK / 8PSK | Phase noise rejection | `crates/openpulse-dsp` |
| **AFC IQ-squaring estimator** | BPSK | ±baud/4 | ±7.8 Hz (BPSK31) … ±62.5 Hz (BPSK250) per estimate |
| **Receive notch** | RX (default on) | Removes steady in-band tones | `[modem] notch_enabled` |

See [`docs/features.md`](docs/features.md) for the crossfade, AFC and FEC detail.

### Operator interfaces

| Interface | Release 1 | Description |
|---|---|---|
| **CLI** (`openpulse`) | yes | transmit, receive, modes, devices, mode-advisor, adaptive, arq, benchmark, monitor, calibrate, config init, and `daemon …` to control a running `openpulse-server` |
| **Operator panel** (`openpulse-panel`) | no | iced GUI on the daemon control port: controls, spectrum, waterfall, rate ladder, messages, event log |
| **TUI** (`openpulse-tui`) | no | ratatui terminal UI: HPX state, AFC/rate meters, DCD energy bar, transitions log |
| **Twin-station view** (`openpulse-twinview`) | dev tool | egui viewer over two daemons |
| **Signal testbench** (`openpulse-testbench`) | dev tool | egui TX / channel / mixed / RX views with waterfall, spectrum, scatter |

---

## Quick start

```bash
# Toolchain preflight (required: rustc >= 1.97.1)
./scripts/check-toolchain.sh

# Build (requires libasound2-dev on Linux for the CPAL audio feature)
cargo build --workspace

# Test suite — no audio hardware required
cargo test --workspace --no-default-features

# Lint
cargo clippy --workspace --no-default-features --all-targets -- -D warnings
cargo fmt --all -- --check

# Workspace gate (quote results only from its GATE: line)
scripts/gate.sh

# Benchmark regression gate
cargo run -p openpulse-cli --no-default-features -- --backend loopback --log error benchmark run

# Automated mode × channel test matrix (outputs to docs/test-reports/)
cargo run -p openpulse-testmatrix --no-default-features
```

If you are temporarily pinned to an older Rust toolchain, run the fallback core gates to keep
CI-relevant coverage for all non-PKI crates:

```bash
cargo clippy --workspace --exclude pki-tooling --no-default-features --all-targets -- -D warnings
cargo test --workspace --exclude pki-tooling --no-default-features
```

The `--no-default-features` flag disables the CPAL audio backend. All tests must
pass with this flag. Never add tests that require real audio hardware.

### Try it without a radio

None of this needs a radio or sound card, and none of it transmits.

```bash
# Two-station ARQ link simulator GUI: simulates both stations over a virtual HF
# channel. Change SNR / channel model / mode live and watch the rate ladder adapt.
cargo run -p openpulse-linksim --features gui --bin openpulse-linksim-gui

# With the operator panel: --serve makes the simulator speak the daemon control
# protocol. Press Run in the simulator, then Connect in the panel (default 127.0.0.1:9000).
cargo run -p openpulse-linksim --features "gui,serve" --bin openpulse-linksim-gui -- --serve 127.0.0.1:9000
cargo run -p openpulse-panel   # in another terminal
```

For on-air operation, build the daemon or TNC with audio (`--features cpal` on `openpulse-daemon`
and `openpulse-ardop`; the CLI has it by default) plus the PTT feature your backend needs
(`serial` for RTS/DTR, `gpio`), and configure `~/.config/openpulse/config.toml` — see
[`docs/config/openpulse.config.toml`](docs/config/openpulse.config.toml), the
[operator manual](docs/openpulse-manual.md) and the [on-air test plan](docs/on-air_testplan.md).

---

## Repository layout

### Core layer

| Crate | Role |
|---|---|
| `crates/openpulse-core` | Frame format, CRC, FEC (RS, conv, soft-concatenated, LDPC, turbo, interleaver), HPX session state machine, rate adaptation and profiles, plugin registry, trust/signing, SAR, ACK, LZ4/Zstd compression, relay, query propagation, peer cache, PQ handshake |
| `crates/openpulse-audio` | `LoopbackBackend` (testing) and `CpalBackend` (hardware, feature-gated); output-device name cache |
| `crates/openpulse-modem` | `ModemEngine`, `PipelineScheduler`, acquisition, DCD/CSMA, LLR-accumulating ARQ/HARQ (`harq.rs` + `rate_policy.rs`), benchmark harness, channel-sim harness |
| `crates/openpulse-channel` | Channel simulation: Watterson, Gilbert-Elliott, QRN/QRM/QSB/Chirp |
| `crates/openpulse-radio` | PTT backends (none, RTS/DTR, VOX, rigctld, CM108, GPIO), `SharedPtt`, rigctld and generic serial CAT |
| `crates/openpulse-dsp` | RRC filter, PLL, Gardner timing recovery, LMS/DFE equaliser, preamble library, CE-SSB conditioner |
| `crates/openpulse-config` | Typed TOML configuration with CLI-override precedence |
| `crates/openpulse-gpu` | wgpu compute kernels with CPU fallback (`gpu` feature) |
| `crates/openpulse-keystore` | `FileKeystore` — named secrets encrypted at rest (Argon2id → ChaCha20-Poly1305) |
| `crates/openpulse-linksec` | Control-channel link security: PSK-authenticated, encrypted daemon↔client link (Noise `Noise_NNpsk0`) |

### Protocol layer

| Crate | Role |
|---|---|
| `crates/openpulse-daemon` | Background daemon (binary `openpulse-server`): modem engine, PTT, NDJSON+WebSocket control port; also hosts QSY, repeater, discovery and file transfer when enabled |
| `crates/openpulse-ardop` | ARDOP-compatible TCP TNC (binary `openpulse-tnc`) |
| `crates/openpulse-kiss` | KISS/AX.25 TNC (binary `openpulse-kisstnc`) |
| `crates/openpulse-b2f` | B2F/Winlink state machine: FC/FS/Ff/Fq frames, Gzip (Type D) |
| `crates/openpulse-b2f-driver` | ISS/IRS session driver over ARDOP TCP |
| `crates/openpulse-gateway` | Direct TCP Winlink CMS gateway (binary `openpulse-gateway`) |
| `crates/openpulse-qsy` | QSY frequency agility: signed wire codec, `QsySession`, `QsyScanner` |
| `crates/openpulse-mesh` | Mesh broadcast daemon (binary `openpulse-mesh`) |
| `crates/openpulse-repeater` | Cross-band repeater with trust-policy filtering and auto-ID |
| `crates/openpulse-discovery` | JS8-based station discovery + rendezvous |
| `crates/openpulse-filexfer` | Direct P2P file transfer: `OPFX` wire codec, sender/receiver state machines, resume |
| `crates/openpulse-freedv-auth` | Ed25519 authentication beacons for FreeDV voice |

### UI and tooling

| Crate | Role |
|---|---|
| `crates/openpulse-cli` | CLI (binary `openpulse`) |
| `crates/openpulse-tui` | ratatui TUI |
| `apps/openpulse-panel` | iced operator panel on the daemon control port |
| `apps/openpulse-twinview` | egui viewer over two daemons |
| `apps/openpulse-testbench` | egui signal-path testbench |
| `apps/openpulse-testmatrix` | Automated mode × channel test matrix |
| `apps/openpulse-linksim` | Two-station ARQ link simulator (CLI + GUI); the goodput gate |
| `pki-tooling` | Trust-bundle signing service with PostgreSQL persistence |

### Plugins

| Crate | Registered modes |
|---|---|
| `plugins/mfsk16` | MFSK16 (SL1), MFSK16-ACK (return channel) |
| `plugins/bpsk` | BPSK31, BPSK63, BPSK100, BPSK250, BPSK250-RRC |
| `plugins/qpsk` | QPSK125, QPSK250, QPSK250-D, QPSK500, QPSK500-D, QPSK1000, QPSK1000-HF, QPSK2000, plus `-RRC` variants |
| `plugins/ofdm` | OFDM16, OFDM52, OFDM52-8PSK, OFDM52-16QAM, OFDM52-32QAM, OFDM52-64QAM |
| `plugins/fsk4` | FSK4-ACK |
| `plugins/psk8` | 8PSK500, 8PSK1000, 8PSK1000-HF, plus `-RRC` variants, 8PSK2000-RRC |
| `plugins/64qam` | 64QAM500, 64QAM1000, 64QAM2000-RRC |
| `plugins/scfdma` | SCFDMA16, SCFDMA26-{8PSK,16QAM,32QAM}, SCFDMA52, SCFDMA52-P2, SCFDMA52-LP, SCFDMA52-{8PSK,16QAM,32QAM,64QAM}, SCFDMA52-64QAM-P4 |
| `plugins/pilot` | PILOT-{QPSK,8PSK,16QAM,32APSK} at 500/1000 baud and `-RRC`, plus 2000-RRC |
| `plugins/js8` | JS8-SLOW, JS8-NORMAL, JS8-FAST, JS8-TURBO, JS8-ULTRA |

The first five are on the Release 1 ladder; the others are not part of Release 1.

---

## Non-GPL interfacing

OpenPulseHF is GPL v3, but several interfaces let non-GPL software interoperate without
GPL obligations.  All of the following sit behind a **process boundary** or a **network
protocol boundary**, which the FSF and courts have consistently held does not trigger
copyleft:

| Interface | Transport | Description |
|---|---|---|
| **ARDOP TNC** | TCP 8515 (cmd) / 8516 (data) | ARDOP ASCII command protocol, as used by Pat |
| **KISS/AX.25 TNC** | TCP 8100 | Standard KISS framing |
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
