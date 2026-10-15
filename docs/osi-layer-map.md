---
project: openpulsehf
doc: docs/osi-layer-map.md
status: living
last_updated: 2026-10-15
---

# OpenPulseHF — OSI layer map

OpenPulseHF is a plugin-based HF (shortwave) software modem and data-network stack written in Rust.
Amateur/HF radio does not map perfectly onto the 7-layer OSI model — several components straddle
layers — but the model is a useful lens for showing *where each part lives* and how a message travels
from an operator's keyboard down to RF and back.

The map shows every component in the code. **Release 1 (`v0.17.0`)** ships only part of it: the
daemon and CLI, the ARDOP TNC, the `hpx_hf` ladder and its plugins (MFSK16, BPSK, QPSK250-D,
OFDM52, FSK4 ACK), each rung's FEC, session compression, the PTT backends, station ID and the signed
classical handshake. Everything else below (mesh, relay, repeater, discovery, QSY, file transfer,
FreeDV auth, Winlink B2F/gateway, KISS, the PQ handshake, GPU, panel/TUI, and the plugins not on
`hpx_hf`) is in the code but experimental and not part of Release 1. See
`docs/dev/project/workplan.md`.

```
        ┌───────────────────────────────────────────────────────────────────────┐
  L7    │ APPLICATION   CLI · TUI · Panel (iced) · Testbench · Daemon             │
        │               ARDOP TNC · KISS TNC · B2F/Winlink · CMS gateway · FreeDV │
        ├───────────────────────────────────────────────────────────────────────┤
  L6    │ PRESENTATION  Compression (LZ4/zstd, gzip) · Signing + manifests        │
        │               PQ crypto (ML-DSA/ML-KEM) · Control-channel encryption    │
        ├───────────────────────────────────────────────────────────────────────┤
  L5    │ SESSION       HPX state machine · Signed + PQ handshake · Trust store   │
        │               Adaptive session profiles · Secure session lifecycle      │
        ├───────────────────────────────────────────────────────────────────────┤
  L4    │ TRANSPORT     ARQ/HARQ retransmission · SAR (segment/reassemble)        │
        │               ACK taxonomy · Rate adaptation (speed-level ladder)       │
        ├───────────────────────────────────────────────────────────────────────┤
  L3    │ NETWORK       Peer cache · Multi-hop relay + route scoring · Query      │
        │               propagation · QSY freq-agility · Mesh · Digipeater · AX.25│
        ├───────────────────────────────────────────────────────────────────────┤
  L2    │ DATA LINK     Frame + CRC-16 · FEC (RS/Conv/LDPC/Turbo) + interleaver   │
        │               Modem engine · Modulation plugins · CSMA/DCD · AFC/EQ/GPU │
        ├───────────────────────────────────────────────────────────────────────┤
  L1    │ PHYSICAL      Audio backend (CPAL/loopback) · PTT/CAT · SSB transceiver │
        │               + antenna · HF channel  (channel simulator for testing)   │
        └───────────────────────────────────────────────────────────────────────┘
```

## Layer-by-layer components

| OSI layer | Role | OpenPulseHF components |
|---|---|---|
| **7 · Application** | User-facing apps and application protocols | `openpulse-cli`, `openpulse-tui`, `openpulse-panel` (iced operator GUI), `openpulse-testbench`, `openpulse-daemon` (control server); TNC/protocol front-ends `openpulse-ardop` (ARDOP TCP), `openpulse-kiss` (KISS), `openpulse-b2f` + `openpulse-b2f-driver` + `openpulse-gateway` (B2F/Winlink), `openpulse-freedv-auth`; tooling `openpulse-linksim`, `openpulse-twinview`, `openpulse-testmatrix`, `pki-tooling` |
| **6 · Presentation** | Data representation, compression, encryption | Session compression (LZ4 or zstd with a shared dictionary, whichever is smaller) and Winlink gzip / B2F Type D (`openpulse-core::compression`, `openpulse-b2f`); Ed25519 signing + SHA-256 transfer manifests; post-quantum crypto (ML-DSA-44 / ML-KEM-768); **control-channel encryption** (`openpulse-linksec` Noise + `openpulse-keystore`); CE-SSB TX conditioning (`openpulse-dsp::cessb`) |
| **5 · Session** | Session setup, dialogue control, security context | HPX session state machine (`HpxSession`/`HpxReactor`); signed handshake (CONREQ/CONACK) and PQ in-band handshake; trust store + trust policy; adaptive session profiles (`fast` and `robust`, both on the one `hpx_hf` speed-level ladder); secure-session begin/end |
| **4 · Transport** | Reliable delivery, segmentation, flow/rate control | ARQ/HARQ retransmission with soft-LLR combining (`harq`, `rate_policy`); SAR segmentation & reassembly (up to 255 × 251 = 64 005 bytes per segment); ACK taxonomy; rate adaptation (`SpeedLevel` / `RateAdapter`) |
| **3 · Network** | Addressing, routing, multi-hop relay | Peer cache + self-authenticating peer descriptors; multi-hop relay forwarding with trust-weighted route scoring, hop limits, duplicate suppression; query/route-discovery propagation; QSY frequency-agility (`openpulse-qsy`); mesh broadcast (`openpulse-mesh`); digipeater/repeater (`openpulse-repeater`); AX.25 addressing (`openpulse-kiss`) |
| **2 · Data link** | Framing, error control, medium access | Frame format + CRC-16 (`openpulse-core`); FEC — Reed-Solomon, convolutional, LDPC, turbo — with a block interleaver; the modem engine + pipeline scheduler (`openpulse-modem`); modulation plugins (BPSK/QPSK/8PSK/64QAM, OFDM, SC-FDMA, pilot-framed, FSK4-ACK); CSMA/DCD channel access; AFC, carrier recovery, LMS/DFE equalizers (`openpulse-dsp`); optional GPU DSP (`openpulse-gpu`) |
| **1 · Physical** | The signal on the wire/air | Audio backends (`openpulse-audio`: CPAL hardware / loopback); PTT + CAT keying (`openpulse-radio`: serial RTS/DTR, CM108, a GPIO line (`gpio` feature), VOX, rigctld, generic CAT; one `SharedPtt` keying path for every front-end); the operator's SSB transceiver + antenna (external); the HF ionospheric channel. `openpulse-channel` (Watterson/Gilbert-Elliott/QRN) *models* this layer for hardware-free testing |

## Two parallel security planes

OpenPulseHF secures **two** distinct links, at different layers:

- **On-air / RF peer link** (L5–L6): Ed25519 signed handshake, optional post-quantum (ML-DSA/ML-KEM),
  trust store — authenticating the *remote station* over the radio.
- **Local control channel** (L6, daemon ↔ operator panel): a PSK-authenticated, encrypted Noise
  channel (`openpulse-linksec`) — authenticating the *operator client* that commands the transmitter.

## How a message flows down the stack

An operator sends a message in the panel (L7) → it is optionally compressed and the session is
authenticated (L6/L5) → segmented and queued for reliable, rate-adapted delivery (L4) → addressed and,
if needed, routed via relays (L3) → framed with CRC + FEC and modulated by the selected waveform, with
channel-access checks (L2) → converted to audio and keyed onto the transceiver via PTT (L1) → across
the HF channel to the far station, where the stack runs in reverse.
