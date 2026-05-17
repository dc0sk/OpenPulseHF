---
project: openpulsehf
doc: docs/ardop-research.md
status: living
last_updated: 2026-05-01
---

# ARDOP Research

This note captures publicly available technical information about ARDOP (Amateur Radio Digital Open Protocol) for use as background research and design reference in OpenPulseHF.

ARDOP is the primary open-source point of comparison for OpenPulseHF. Unlike VARA (proprietary shareware) and PACTOR (proprietary hardware), ARDOP is fully open-source under a public-domain or permissive licence. Its specification and reference implementation are available for study.

## Overview

ARDOP was developed by Rick Muething (KN6KB) and John Wiseman (G8BPQ) with contributions from other members of the Winlink Development Team. It is intended as an open alternative to PACTOR and VARA for Winlink and peer-to-peer digital communication over amateur HF/VHF.

Key properties:
- Open specification and open-source reference implementation (ardopc by KN6KB).
- Half-duplex ARQ, same operating model as VARA.
- Sound card TNC: audio I/O to any SSB radio, PTT via serial port or VOX.
- Winlink-compatible: designed as a drop-in replacement for PACTOR/VARA in the Winlink gateway architecture.
- TCP command/data interface: same TNC port model as VARA (command port + data port).

## Technical specifications

### Bandwidth modes

ARDOP supports four occupied bandwidth classes:

| Mode | Occupied bandwidth | Subcarriers | Max throughput |
|------|-------------------|-------------|----------------|
| ARDOP 200 | 200 Hz | 4 | ~200 bps |
| ARDOP 500 | 500 Hz | 4–8 | ~500 bps |
| ARDOP 1000 | 1000 Hz | 4–16 | ~1000 bps |
| ARDOP 2000 | 2000 Hz | 4–32 | ~2000 bps |

Throughput figures are approximate user data rates under good channel conditions. ARDOP throughput is significantly lower than VARA at equivalent bandwidth, reflecting ARDOP's design priority of robustness over peak rate.

### Modulation

ARDOP uses two distinct modulation schemes depending on frame type:

**4FSK for control and low-rate frames:**
- Four tones, 50 baud symbol rate in 200 Hz mode.
- Robust at low SNR; decodable when coherent PSK would fail.
- Used for connection request (CONREQ), connect acknowledgement (CONACK), disconnect (DISC), and other control frames.

**OFDM for data frames:**
- 4 to 32 subcarriers depending on bandwidth mode.
- Differential PSK per subcarrier (DBPSK or DQPSK), avoiding per-subcarrier phase recovery.
- ARDOP 200 uses 4 subcarriers in 200 Hz; ARDOP 2000 uses up to 32 subcarriers in 2000 Hz.

Note for OpenPulseHF: ARDOP's 4FSK control channel approach is notable. Using a distinct, more robust modulation for ACK/control frames (separate from the data channel modulation) is architecturally analogous to VARA's parallel-FSK ACK frame design. OpenPulseHF's HPX ACK frame design should consider this pattern.

### Forward Error Correction

ARDOP uses rate 2/3 convolutional codes with Viterbi decoding.

- Constraint length K = 5 (the reference implementation uses K=5 or K=9 depending on frame type).
- Code rate 2/3 means 2 information bits are encoded into 3 transmitted bits (33% FEC overhead).
- Viterbi decoding is a maximum-likelihood decoder for convolutional codes; complexity is manageable on all target platforms.
- No separate interleaver is documented in the public specification; the convolutional code provides some inherent spread against short burst errors through its constraint length, but burst errors exceeding the constraint length require channel-level recovery via ARQ.

Comparison to OpenPulseHF RS code: convolutional + Viterbi handles random errors more gracefully than RS in isolation, but RS with a block interleaver handles longer burst errors more effectively. Combining both (outer RS + inner convolutional) is the classical concatenated code approach used in satellite communications.

### Frame types

ARDOP defines the following frame type categories:

| Category | Examples | Modulation |
|----------|----------|-----------|
| Connection control | CONREQ, CONACK, DISC, DISCACK | 4FSK |
| Data | DATA, DATAACK, DATANAK | OFDM + convolutional FEC |
| Session management | END, BREAK, IDFRAME | 4FSK |
| Test/calibration | TXFRAME, TWOTONE | Special |

The CONREQ frame includes source callsign, target callsign, and a session bandwidth negotiation field. This is the connection establishment handshake.

### ARQ operation

ARDOP uses a selective-repeat ARQ scheme:
- The ISS (Information Sending Station) transmits data frames.
- The IRS (Information Receiving Station) sends ACK or NAK after each data frame.
- A DATAACK acknowledges a correctly received frame; a DATANAK requests retransmission.
- After a configurable number of NAKs, the ISS can reduce rate (bandwidth mode) or terminate the session.
- Role reversal (BREAK) allows the IRS to become ISS without session teardown.

### Rate adaptation

ARDOP's rate adaptation is simpler than VARA's 11-level system:
- Bandwidth mode is selected at connection time based on negotiation.
- Within a bandwidth mode, there is no mid-session modulation-order change.
- Retry limits trigger a bandwidth downgrade to the next lower mode.
- This is coarser than VARA's fine-grained per-packet ACK2/ACK3 rate stepping.

### Timing parameters

- Frame sync preamble: a known pilot sequence transmitted before each frame for synchronisation.
- ARQ timeout: configurable, typically 2–5 seconds per frame attempt.
- PTT turnaround: ARDOP budgets approximately 100–200 ms for half-duplex switching.
- Maximum session idle time before automatic teardown: configurable.

### Host interface

ARDOP uses a TCP interface with separate command and data ports, identical in model to VARA:
- Command port (default 8515): ASCII command/response protocol (LISTEN, CONNECT, DISCONNECT, VERSION, etc.).
- Data port (default 8516): raw binary data stream for payload bytes.

This interface is compatible with the same host application architecture as VARA. An application supporting ARDOP can theoretically be adapted to VARA (and vice versa) with a thin adapter layer.

## ARDOP versus VARA: key differences

| Property | ARDOP | VARA HF |
|----------|-------|---------|
| Licence | Open source (public domain) | Proprietary shareware |
| Source available | Yes (ardopc by KN6KB) | No |
| Modulation | 4FSK + OFDM | OFDM only |
| FEC | Convolutional + Viterbi | Turbo codes |
| Speed levels | 4 bandwidth modes (coarse) | 11 levels (fine) |
| Peak throughput (2 kHz) | ~2000 bps | ~7536 bps |
| PAPR | Low (4FSK: ~0 dB; OFDM: moderate) | 9 dB DATA / 6 dB ACK |
| Rate adaptation granularity | Per bandwidth mode | Per data frame (ACK2/ACK3) |
| Windows dependency | No (cross-platform) | Windows only (Wine on Linux) |
| Winlink compatible | Yes | Yes |

## ARDOP's open-source reference implementation

The `ardopc` source code (C language) by Rick Muething is available on GitHub. It implements the complete ARDOP protocol stack including the DSP (FFT-based OFDM, 4FSK modulator/demodulator, Viterbi decoder) and the TCP command/data interface.

This source is useful for:
- Understanding how a production HF modem implements timing-critical audio processing in C.
- Referencing the Viterbi decoder implementation for potential comparison or adaptation.
- Understanding the command/data TCP protocol in detail.

OpenPulseHF must not derive its implementation from ardopc source code without an explicit legal review of the licence terms and attribution requirements. Research of the architecture and documented protocol is appropriate; code derivation requires separate evaluation.

## Working conclusions for OpenPulseHF

- ARDOP is the benchmark OpenPulseHF must equal or exceed on robustness in 200–500 Hz profiles, and on throughput in 2000 Hz profiles.
- ARDOP's 4FSK control frame design is a validated pattern for robust low-SNR channel access and ACK delivery; HPX ACK frame design should consider a similarly decoupled control channel modulation.
- ARDOP's coarse bandwidth-mode rate adaptation is simpler to implement than VARA's 11-level scheme but leaves throughput on the table. HPX targets a middle path: multiple rate steps within a bandwidth class without requiring full OFDM.
- The ARDOP TCP command/data interface is a de-facto standard for sound card TNCs. OpenPulseHF CLI and plugin interfaces should be designed to support this interface model for application compatibility.
- ARDOP's open specification and source make it the legally clean reference implementation for HF ARQ design study. VARA's specification PDFs add detail on OFDM and Turbo FEC design but must be treated as reference-only for architecture rather than as a copy template.
