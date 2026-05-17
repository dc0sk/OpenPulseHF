---
project: openpulsehf
doc: docs/marketing/presentation.md
status: draft
last_updated: 2026-05-12
---

# OpenPulseHF — HAMRADIO 2026 Presentation

*Suggested format: 23 slides, 25–35 minutes + 10 min Q&A*

---

## Slide 1 — Title

**OpenPulseHF**  
*The Open HF Data Modem That Sets New Standards*

> "Covers core VARA/ARDOP-era workflows — plus open features not commonly available elsewhere."

*Speaker notes: Introduce yourself and the project. State upfront: 100% open source, GPL v3,
no subscription, no dongle. The audience will immediately understand this is different.*

---

## Slide 2 — The problem with HF data today

- HF channels are hostile: ionospheric fading, Doppler spread, burst noise
- The dominant modem (VARA) is closed-source; the backup (ARDOP) is abandoned
- Operators are locked in: no auditability, no customisation, no embedded use
- No HF modem has embraced post-quantum cryptography
- No HF modem ships a tool to let you *see* what your signal is doing

*Speaker notes: Show a Watterson fading envelope plot — visually demonstrate why HF is hard.*

---

## Slide 3 — What OpenPulseHF is

- A full-stack HF digital modem written in Rust
- Plugin architecture: drop in a new waveform without touching the engine
- ARQ session management, Winlink/B2F, KISS/AX.25 — all built in
- Hardware-free deterministic test suite: **322 cases, 322 passing** against published channel models
- Built-in live signal-path testbench GUI — no oscilloscope required
- Runs on a Raspberry Pi 4; cross-compiles in CI

*Speaker notes: Show the crate map briefly — one slide to show scope, not to explain every crate.*

---

## Slide 4 — 33 waveforms, one engine

| | BPSK | QPSK | 8PSK | 64QAM | OFDM | SC-FDMA | FSK4 |
|---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| Modes | 5 | 11 | 9 | 3 | 2 | 2 | 1 |
| Baud range | 31–250 | 125–9600 | 500–9600 | 500–2000 | — | — | 100 |
| Max bits/sym | 1 | 2 | 3 | **6** | QPSK/SC | QPSK/SC | 2 |
| Raw data rate (max) | 250 bps | 2000 bps | 3000 bps | **12 kbps** | ~2889 bps | ~2889 bps | ACK |
| Peak with LZ4† | ~500 bps | ~4000 bps | ~6 kbps | **~24 kbps** | ~5.8 kbps | **111 kbps lab peak†** | ACK |
| HF-compliant | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

† Raw data rate = symbol rate × bits/symbol. "Peak with LZ4" uses ≈ 2× for typical text payloads.
The SC-FDMA figure is a **lab peak** from the built-in testbench: **111 kbps on SCFDMA52 + LZ4**
with a 2048-byte highly repetitive frame (≈ 38× compression ratio); it is not a typical on-air
goodput number.

64QAM uses a Gray-coded rectangular constellation with a max-log-MAP soft demodulator.
All RRC variants use Root Raised Cosine pulse shaping — the same approach used in LTE
and DVB receivers.

*Speaker notes: Walk through the table quickly. The key message: 64QAM reaches 12 kbps raw in
the standard 2700 Hz SSB passband. The 111 kbps number is a lab compression peak on synthetic
data; present it explicitly as a best-case demonstration, not typical field throughput.*

---

## Slide 5 — RRC pulse shaping: cleaner spectrum, sharper recovery ★

**Why it matters:**  
A rectangular symbol window has −13 dB first sidelobes. At HF we share a crowded band.

**What OpenPulseHF does:**  
- RRC modes apply Root Raised Cosine filtering (α = 0.35) at both TX and RX
- Matched filtering at the receiver maximises SNR at the decision instant
- **Gardner timing error detector** (TED) + **Costas PLL** lock on the symbol clock
  and carrier phase automatically — the same algorithm used in LTE basestations

**What you get:**  
- Sidelobe suppression: < −40 dB adjacent channel interference  
- Symbol timing recovered within a few symbols — reliable at 8192+ symbols per frame  
- Gardner TED mu clamp (±0.49) prevents strobe interval jumps that cause symbol slips

*Speaker notes: Show a TX spectrum comparison: rectangular window vs RRC — the sidelobes
disappear. Mention that the Gardner + Costas combination is what makes 8PSK/QPSK-RRC at
1000 baud reliable over a real soundcard.*

---

## Slide 6 — Adaptive rate ladder (world first in open source)

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
- ACK is a 5-byte FSK4 frame — only **200 ms** of air time per acknowledgement

*Speaker notes: Show a time-series plot of a session stepping up from SL2 to SL6 as conditions
improve.*

---

## Slide 7 — FEC: the full stack — with unlimited payload size

| Mode | Mechanism | Corrects | Overhead | Max payload |
|---|---|---|---|---|
| RS(255,223) | Reed-Solomon t=16 | 16 byte errors/block | 14% | **Unlimited** |
| RS(255,191) Strong | Reed-Solomon t=32 | 32 byte errors/block | 25% | **Unlimited** |
| Concatenated | Conv K=3 hard + RS | burst + random | 2.28× | Unlimited |
| **Soft Viterbi** | **K=7 soft-decision** | random noise | **2.28×, +5 dB** | Unlimited |
| Memory-ARQ | Sample averaging over N retx | noise floor | none | — |

**"Unlimited" means it:** the RS codec automatically splits any payload into N × 255-byte
blocks — a 2048-byte payload gets full RS protection across 10 independent blocks.
No other open HF modem does this.

*Speaker notes: Contrast with typical implementations that cap at one RS block (219 bytes
with the 4-byte length prefix). Show that concatenated and soft modes work identically at
any size.*

---

## Slide 8 — SC-FDMA: world first in amateur HF ★

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

*Speaker notes: Show PAPR comparison histogram: OFDM52 ~12 dB natively vs SCFDMA52 ~9 dB
natively — SC-FDMA's single-carrier DFT-spreading eliminates the high-PAPR multi-carrier
peaks. Contrast with OFDM52 which needs a 50-iteration iterative clipping loop to stay safe.*

---

## Slide 9 — QSY frequency agility ★

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

## Slide 10 — Post-quantum security ★ (first in amateur radio)

The threat: Shor's algorithm breaks RSA and elliptic-curve signatures when a cryptographically
relevant quantum computer arrives.  We don't know when — but signed session frames you transmit
*today* must still be verifiable as authentic *then*.

OpenPulseHF implements **FIPS 203 + FIPS 204** post-quantum primitives:

| Role | Algorithm | Key size |
|---|---|---|
| Authentication | ML-DSA-44 (Dilithium2) | 1312 B public key |
| Forward secrecy | ML-KEM-768 (Kyber768) | 1184 B enc key |
| Classical (hybrid) | Ed25519 | 32 B |

**Hybrid mode**: both Ed25519 and ML-DSA-44 signatures are required — safe during the
transition period when classical keys are still trusted.

Every data transfer is accompanied by a **SHA-256 + Ed25519 transfer manifest** — the receiver
verifies content integrity before passing data to the application.

*Speaker notes: Emphasise: this is a first for any open amateur radio modem.*

---

## Slide 11 — Automatic frequency correction (AFC)

Real radios drift. Cheap USB soundcard interfaces add their own clock error.

**OpenPulseHF AFC:**

- IQ-squaring estimator measures carrier offset without knowing the data
- Tracking range scales with baud rate: ±62.5 Hz at BPSK250, ±7.8 Hz at BPSK31
- First-order correction loop converges to < 1.1 Hz residual within 25 frames
- Exposed as a structured engine event (`AfcUpdate`) for external monitoring

```
BPSK250: ±62.5 Hz range  (covers most USB soundcard drift)
BPSK63:  ±15.6 Hz range
BPSK31:  ± 7.8 Hz range  (standard PSK31 tolerance)
```

*Speaker notes: A ±50 Hz uncompensated offset would make BPSK31 unusable. AFC makes
any SSB radio + soundcard combination viable without manual frequency adjustment.*

---

## Slide 12 — Drop-in compatibility

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

## Slide 13 — Compression: squeezing HF bandwidth

| Algorithm | Use case | Savings on typical WL2K message |
|---|---|---|
| None | Already compressed data | — |
| LZ4 | Fast, general purpose | 20–40% |
| **Zstd + HPX dictionary** | **Short HF messages** | **40–60%** |

The Zstd dictionary is pre-trained on typical amateur radio message traffic.
Compression is negotiated in the **signed handshake** — both sides must support it,
and a man-in-the-middle cannot downgrade the selection without breaking the signature.

*Speaker notes: Show a before/after size comparison for a typical Winlink weather report.*

---

## Slide 14 — Built-in signal-path testbench ★ (unique in open-source HF)

No other open HF modem ships a tool like this.

```
┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐
│  TX tap  │  │ Noise tap│  │Mixed tap │  │  RX tap  │
│ spectrum │  │ spectrum │  │ spectrum │  │ spectrum │
│waterfall │  │ waterfall│  │ waterfall│  │ waterfall│
└──────────┘  └──────────┘  └──────────┘  └──────────┘
         IQ scatter ──────────────────────────────────────→
         BER · ECC% · SNR · effective data rate
```

**What you can do in 5 minutes:**
- Watch OFDM52 subcarriers appear in the spectrum as baud rate changes
- See an 8PSK constellation collapse under G-E burst noise, recover with FEC enabled
- Measure BER at each SNR step to reproduce published performance curves
- Connect a real HF receiver and demodulate live signals (CPAL backend)

7 channel models: AWGN, Watterson F1/F2/Poor, G-E Light/Burst, QRN, QRM, QSB, Chirp.

*Speaker notes: Live demo here if possible — start with BPSK250 clean, add AWGN noise,
enable RS FEC, watch BER drop. Then switch to the Watterson F2 fading channel.*

---

## Slide 15 — Raspberry Pi 4 as a full digipeater

The `openpulse-mesh` binary implements a full mesh relay node:

- Receives a beacon frame
- Re-broadcasts with TTL decrement
- Multi-hop routing via `RelayForwarder`
- Trust-policy enforcement at each hop: Verified → PskVerified → Unknown → Reduced

Trust-weighted path scoring uses the **bottleneck model**: the route score is the
minimum per-hop score across all intermediate relays, so one weak relay limits the
whole path.

An RPi 4 with a USB audio interface and rigctld connection is a complete
unattended relay node — no desktop required.

---

## Slide 16 — GPU acceleration (optional)

The `openpulse-gpu` crate offloads DSP to any **Vulkan / Metal / DX12** GPU via `wgpu`:

- BPSK modulation kernel
- BPSK IQ demodulation kernel
- Timing offset search

CPU fallback is transparent — GPU is optional, not required.  On a budget gaming GPU,
modulation throughput is 15–30× faster than CPU-only, freeing the Pi's CPU for other tasks.

---

## Slide 17 — The test harness: 322 cases, all validated

OpenPulseHF ships a **parametric channel simulation harness** validated against:

- **Watterson model** (ITU-R F.1487): Good F1, Moderate F1, Poor F1, Extreme
- **Gilbert-Elliott**: Light / Moderate / Heavy / Severe burst profiles
- **AWGN**: systematic SNR sweep from 0 to 30 dB
- **QRN / QRM / QSB / Chirp**: atmospheric and man-made interference

**Test matrix result: 322/322 cases passing.**

The `openpulse-testmatrix` binary covers every mode × FEC × compression × channel
combination and produces a Markdown + CSV report.  CI blocks any merge that regresses
a case — not just unit tests but end-to-end demodulation correctness.

*Speaker notes: Show the test matrix summary table on screen. Emphasise: this is the difference
between a modem that passes unit tests and one that actually works on the air.*

---

## Slide 18 — First-to-market summary

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
| Built-in signal-path testbench GUI | **✓** | — |
| Multi-block RS FEC (unlimited payload) | **✓** | — |
| RRC matched filtering + Gardner TED | **✓** | — |
| Automatic frequency correction (AFC) | **✓** | — |
| 322-case automated channel test matrix | **✓** | — |
| Per-band TX attenuation persistence | **✓** | — |
| IQ output for SDR upconversion | **✓** | — |

---

## Slide 19 — Roadmap: what's next after feature freeze

The modem engine is feature-frozen.  Active development continues in:

- **On-air validation** (Phase 3.5-reg): systematic IARU-frequency tests
- **LDPC/Turbo FEC** (BL-FEC-6): GPU acceleration path (wgpu compute shaders)
- **CAZAC training sequences**: coherent pilot-based channel estimation for 8PSK
- **RAKE receiver**: multi-path diversity for spread-spectrum modes
- **openpulse-plugin-host**: C-ABI shim for commercial/proprietary plugins (LGPL)

---

## Slide 20 — Getting started (live demo)

*[Demo on-stage — or short video clip if RF not permitted in the hall]*

```bash
# Install
cargo install --git https://github.com/dc0sk/OpenPulseHF openpulse-cli

# Start TNC for Pat
openpulse-tnc --mode BPSK250 --cmd-port 8515 --data-port 8516

# Or run in benchmark mode (no radio required)
openpulse --backend loopback benchmark run

# Launch the signal testbench GUI (no radio required)
openpulse-testbench
```

*Speaker notes: If live demo is possible, connect to Pat and send one message.
Alternatively, launch openpulse-testbench and demonstrate the channel simulation live.*

---

## Slide 21 — How to contribute

- **Issues and PRs**: GitHub — `dc0sk/OpenPulseHF`
- **Plugin development**: `docs/contributing-plugins.md`
- **Waveform research**: `docs/backlog-waveforms.md`
- **On-air testing**: `scripts/run-onair-tests.sh` — 2×RPi test rig
- **Commercial plugins**: `docs/plugin-commercial-interface.md` — C-ABI and IPC paths

*Speaker notes: Emphasise that the architecture is designed for contribution — the plugin
trait is stable, CI runs on every PR, and the test matrix gives immediate feedback.*

---

## Slide 22 — Q&A

**Questions welcome**

*Speaker notes: Likely questions:*

- *"Is it compatible with VARA?"* — Protocol interfaces (ARDOP TCP, KISS) are compatible;
  the air interface is different by design (competing standard, not clone).
- *"Will it work with my Icom / Yaesu / Kenwood?"* — Yes, via rigctld or generic serial CAT.
- *"Can I use it commercially?"* — GPL v3; see the commercial plugin interface doc for
  proprietary waveform options.
- *"When is v1.0?"* — Feature-frozen now; release after on-air validation.
- *"What about FEC on large files?"* — RS FEC automatically splits any payload into
  255-byte blocks; 2048-byte payloads get full RS protection, not just 219-byte ones.
- *"What's the testbench useful for?"* — Reproduce published BER curves, compare FEC
  modes side-by-side, verify a waveform plugin before going on-air.

---

## Slide 23 — Thank you

**OpenPulseHF**

github.com/dc0sk/OpenPulseHF  
GPL v3 · Written in Rust · No bundled C DSP libraries

*"The HF modem that gives amateur radio operators cryptographically authenticated sessions,
post-quantum identity integrity, and a built-in signal analyser — at zero cost, with
full source code, and 322 automated channel-simulation test cases to back every claim."*

---

*[QR code to GitHub]*

*HAMRADIO 2026 · Friedrichshafen · Hall B2 · Stand 142*
