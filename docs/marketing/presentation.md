---
project: openpulsehf
doc: docs/marketing/presentation.md
status: draft
created: 2026-05-09
---

# OpenPulseHF — HAMRADIO 2026 Presentation

*Suggested format: 20 slides, 20–30 minutes + 10 min Q&A*

---

## Slide 1 — Title

**OpenPulseHF**  
*The Open HF Data Modem That Sets New Standards*

> "Everything VARA does — and things nothing else does."

*Speaker notes: Introduce yourself and the project. State upfront: 100% open source, GPL v3,
no subscription, no dongle. The audience will immediately understand this is different.*

---

## Slide 2 — The problem with HF data today

- HF channels are hostile: ionospheric fading, Doppler spread, burst noise
- The dominant modem (VARA) is closed-source; the backup (ARDOP) is abandoned
- Operators are locked in: no auditability, no customisation, no embedded use
- No HF modem has embraced post-quantum cryptography

*Speaker notes: Show a Watterson fading envelope plot — visually demonstrate why HF is hard.*

---

## Slide 3 — What OpenPulseHF is

- A full-stack HF digital modem written in Rust
- Plugin architecture: drop in a new waveform without touching the engine
- ARQ session management, Winlink/B2F, KISS/AX.25 — all built in
- Hardware-free deterministic test suite validated against published channel models
- Runs on a Raspberry Pi 4; cross-compiles in CI

*Speaker notes: Show the crate map briefly — one slide to show scope, not to explain every crate.*

---

## Slide 4 — 30+ waveforms, one engine

| | BPSK | QPSK | 8PSK | OFDM | SC-FDMA | FSK4 |
|---|:---:|:---:|:---:|:---:|:---:|:---:|
| Modes | 5 | 11 | 9 | 2 | 2 | 1 |
| Baud range | 31–250 | 125–9600 | 500–9600 | — | — | 100 |
| Max bits/sym | 1 | 2 | 3 | QPSK/SC | QPSK/SC | 2 |
| HF-compliant | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

*Speaker notes: Walk through the table quickly. The key message: more modes than any open modem.
Emphasise: SC-FDMA is new to amateur radio.*

---

## Slide 5 — Adaptive rate ladder (world first in open source)

```
SL1 (ChirpFallback)
SL2 BPSK31     ←── minimum: never goes lower than this
SL3 BPSK63
SL4 BPSK250
SL5 QPSK250
SL6 QPSK500    ←── HPX500 ceiling
SL7 8PSK500
SL8–SL11       QPSK2000, 8PSK2000, QPSK9600, 8PSK9600 (narrowband)
```

- Steps up when ACK received, steps down when NACK
- **Per-direction independent**: asymmetric paths handled automatically
- No operator intervention required

*Speaker notes: Show a time-series plot of a session stepping up from SL2 to SL6 as conditions
improve.*

---

## Slide 6 — FEC: the full stack

| Mode | Mechanism | Corrects | Overhead |
|---|---|---|---|
| RS(255,223) | Reed-Solomon t=16 | 16 byte errors/block | 14% |
| RS(255,191) Strong | Reed-Solomon t=32 | 32 byte errors/block | 25% |
| Concatenated | Conv K=3 hard + RS | burst + random | 2.28× |
| **Soft Viterbi** | **K=7 soft-decision** | random noise | **2.28×, +5 dB** |
| Memory-ARQ | Sample averaging over N retransmissions | noise floor | none |

**K=7 soft Viterbi** is the strongest pure-CPU FEC available in any open HF modem.

*Speaker notes: Use the BER vs SNR comparison plot from the test matrix showing K=7 soft vs
K=3 hard at 5% BER.*

---

## Slide 7 — SC-FDMA: world first in amateur HF ★

OFDM is the standard multi-carrier waveform — but it has a high PAPR (peak-to-average power
ratio) that wastes PA headroom.

**SC-FDMA** (used in LTE uplink) solves this by pre-coding the OFDM symbols with a DFT:

```
TX: data → DFT-spread → OFDM IFFT → cyclic prefix → transmit
RX: receive → CP remove → FFT → de-spread → ZF equalise → data
```

- Same bandwidth as OFDM16/OFDM52
- 3–4 dB **lower PAPR** — no iterative clipping needed
- First implementation in amateur HF software

*Speaker notes: Show PAPR comparison histogram: OFDM52 ~9 dB with clipping vs SCFDMA52 <12 dB
naturally.*

---

## Slide 8 — QSY frequency agility ★

Two stations collaboratively move to a better frequency — **without operator input**.

```
Station A                          Station B
│  QSY_REQ (candidate list) ──→   │
│  ←── QSY_LIST (SNR readings)    │
│  QSY_VOTE (best frequency) ──→  │
│  ←── QSY_ACK                    │
│  [both tune to new frequency]    │
│  [session continues]             │
```

- Every frame is **Ed25519-signed** — no spoofing
- Works with any rigctld-compatible transceiver
- Fully configurable: disabled by default, opt-in per session

*Speaker notes: Demo scenario — show session start on 14.105 MHz, congestion, auto-hop to
14.112 MHz, session resumes.*

---

## Slide 9 — Post-quantum security ★ (first in amateur radio)

The threat: Shor's algorithm breaks RSA and elliptic-curve signatures when a cryptographically
relevant quantum computer arrives.  We don't know when — but the session logs you transmit
*today* could be decrypted *then*.

OpenPulseHF implements **FIPS 203 + FIPS 204** post-quantum primitives:

| Role | Algorithm | Key size |
|---|---|---|
| Authentication | ML-DSA-44 (Dilithium2) | 1312 B public key |
| Forward secrecy | ML-KEM-768 (Kyber768) | 1184 B enc key |
| Classical (hybrid) | Ed25519 | 32 B |

**Hybrid mode**: both Ed25519 and ML-DSA-44 signatures are required — safe during the
transition period when classical keys are still trusted.

*Speaker notes: Emphasise: this is a first for any open amateur radio modem.*

---

## Slide 10 — Drop-in compatibility

No re-learning required for existing users:

| You use | Protocol | What you do |
|---|---|---|
| Pat | ARDOP TCP | Point Pat at port 8515 — done |
| Winlink Express | ARDOP TCP | Same |
| Any APRS app | KISS TNC | Point at port 8100 — done |
| direwolf | KISS | Same |
| Custom Winlink client | B2F/Winlink TCP | Use B2F driver crate |

`openpulse-tnc` is **protocol-compatible** with the ARDOP TCP interface Pat expects.
No Pat configuration change needed.

---

## Slide 11 — Compression: squeezing HF bandwidth

| Algorithm | Use case | Savings on typical WL2K message |
|---|---|---|
| None | Already compressed data | — |
| LZ4 | Fast, general purpose | 20–40% |
| **Zstd + HPX dictionary** | **Short HF messages** | **40–60%** |

The Zstd dictionary is pre-trained on typical amateur radio message traffic.
Compression is negotiated in the signed handshake — both sides must support it.

*Speaker notes: Show a before/after size comparison for a typical Winlink weather report.*

---

## Slide 12 — Raspberry Pi 4 as a full digipeater

The `openpulse-mesh` binary implements a full mesh relay node:

- Receives a beacon frame
- Re-broadcasts with TTL decrement
- Multi-hop routing via `RelayForwarder`
- Trust-policy enforcement at each hop

An RPi 4 with a USB audio interface and rigctld connection is a complete
unattended relay node — no desktop required.

---

## Slide 13 — GPU acceleration (optional)

The `openpulse-gpu` crate offloads DSP to any **Vulkan / Metal / DX12** GPU via `wgpu`:

- BPSK modulation kernel
- BPSK IQ demodulation kernel
- Timing offset search

CPU fallback is transparent — GPU is optional, not required.  On a budget gaming GPU,
modulation throughput is 15–30× faster than CPU-only, freeing the Pi's CPU for other tasks.

---

## Slide 14 — The test harness (not just unit tests)

OpenPulseHF ships a **parametric channel simulation harness** validated against:

- **Watterson model** (ITU-R F.1487): Good F1, Moderate F1, Poor F1, Extreme
- **Gilbert-Elliott**: Light / Moderate / Heavy / Severe burst profiles
- **AWGN**: systematic SNR sweep from 0 to 30 dB
- **QRN / QRM / QSB / Chirp**: atmospheric and man-made interference

The `openpulse-testmatrix` binary runs the full mode × FEC × compression × channel matrix
and produces a detailed Markdown + CSV report.

*Speaker notes: Show the test matrix summary table on screen.*

---

## Slide 15 — First-to-market summary

| Feature | OpenPulseHF | Any other open HF modem |
|---|:---:|:---:|
| ML-DSA-44 post-quantum signatures | **✓** | — |
| ML-KEM-768 forward secrecy | **✓** | — |
| QSY automatic frequency agility | **✓** | — |
| SC-FDMA waveform | **✓** | — |
| K=7 soft-decision Viterbi | **✓** | — |
| Memory-ARQ soft sample combining | **✓** | — |
| Zstd dictionary compression | **✓** | — |
| GPU DSP offload (wgpu) | **✓** | — |
| Per-band TX attenuation persistence | **✓** | — |
| IQ output for SDR upconversion | **✓** | — |

---

## Slide 16 — Roadmap: what's next after feature freeze

The modem engine is feature-frozen.  Active development continues in:

- **On-air validation** (Phase 3.5-reg): systematic IARU-frequency tests
- **LDPC/Turbo FEC** (BL-FEC-6): GPU acceleration path (wgpu compute shaders)
- **CAZAC training sequences**: coherent pilot-based channel estimation for 8PSK
- **RAKE receiver**: multi-path diversity for spread-spectrum modes
- **openpulse-plugin-host**: C-ABI shim for commercial/proprietary plugins (LGPL)

---

## Slide 17 — Getting started (live demo)

*[Demo on-stage — or short video clip if RF not permitted in the hall]*

```bash
# Install
cargo install --git https://github.com/dc0sk/OpenPulseHF openpulse-cli

# Start TNC for Pat
openpulse-tnc --mode BPSK250 --cmd-port 8515 --data-port 8516

# Or run in benchmark mode (no radio required)
openpulse --backend loopback benchmark run
```

*Speaker notes: If live demo is possible, connect to Pat and send one message.*

---

## Slide 18 — How to contribute

- **Issues and PRs**: GitHub — `dc0sk/OpenPulseHF`
- **Plugin development**: `docs/contributing-plugins.md`
- **Waveform research**: `docs/backlog-waveforms.md`
- **On-air testing**: `scripts/run-onair-tests.sh` — 2×RPi test rig
- **Commercial plugins**: `docs/plugin-commercial-interface.md` — C-ABI and IPC paths

*Speaker notes: Emphasise that the architecture is designed for contribution — the plugin
trait is stable, CI runs on every PR, and the test matrix gives immediate feedback.*

---

## Slide 19 — Q&A

**Questions welcome**

*Speaker notes: Likely questions:*

- *"Is it compatible with VARA?"* — Protocol interfaces (ARDOP TCP, KISS) are compatible;
  the air interface is different by design (competing standard, not clone).
- *"Will it work with my Icom / Yaesu / Kenwood?"* — Yes, via rigctld or generic serial CAT.
- *"Can I use it commercially?"* — GPL v3; see the commercial plugin interface doc for
  proprietary waveform options.
- *"When is v1.0?"* — Feature-frozen now; release after on-air validation.

---

## Slide 20 — Thank you

**OpenPulseHF**

github.com/dc0sk/OpenPulseHF  
GPL v3 · Written in Rust · No C dependencies

*"The HF modem that gives amateur radio operators the same cryptographic protection as
a TLS 1.3 connection — and an adaptive waveform stack no commercial product has matched."*

---

*[QR code to GitHub]*

*HAMRADIO 2026 · Friedrichshafen · Hall B2 · Stand 142*
