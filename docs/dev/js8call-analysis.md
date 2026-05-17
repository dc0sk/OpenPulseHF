---
project: openpulsehf
doc: docs/js8call-analysis.md
status: living
last_updated: 2026-05-02
---

# JS8Call Analysis

This document captures technical findings from an analysis of the JS8Call project and its community fork JS8Call-improved (https://github.com/JS8Call-improved/JS8Call-improved) relevant to OpenPulseHF design. JS8Call adds keyboard-to-keyboard ARQ messaging, store-and-forward relay, and adaptive speed selection on top of FT8 modulation. It is the closest existing system to HPX in terms of use-case: it attempts to provide reliable directed messaging over HF with multiple modes at different throughput/sensitivity trade-offs.

Sources used:
- JS8Call-improved GitHub repository: https://github.com/JS8Call-improved/JS8Call-improved
- JS8Call upstream: https://github.com/js8call/js8call
- Source files analysed: `commons.h`, `JS8Submode.cpp`, `JS8Submode.hpp`, `MessageClient.cpp`
- Release notes: v2.4.0, v2.5.0, v2.5.2

---

## What JS8Call is

JS8Call (by Jordan Sherer, KN4CRD) layers a messaging protocol over FT8's modulation and FEC, adding:
- Directed messages (callsign-addressed transmission)
- ARQ acknowledgement and retransmission
- Store-and-forward relay (messages buffered and forwarded by intermediate stations)
- Heartbeat/CQ beacon loop for station announcement
- On-air protocol commands for link quality measurement and network discovery

JS8Call is not a file-transfer TNC. It is a keyboard-to-keyboard chat and lightweight message relay system. Session bandwidth is low (a few dozen to a few hundred bytes per minute). Its relevance to HPX is in its protocol command design, relay architecture, speed ladder rationale, and empirical SNR threshold data.

---

## JS8Call-improved fork

The JS8Call-improved fork, maintained by AC9KH and contributors since November 2025, is the active development branch for JS8Call. Key changes from upstream JS8Call:

| Change | Technical detail |
|--------|-----------------|
| Fortran → C++ port | All signal processing code (previously inherited from WSJT-X Fortran) converted to C++. Removes the Fortran toolchain requirement. |
| Separate decoder process removed | The original design ran the FT8 decoder in a subprocess communicating via shared memory. The fork integrates decoding into the main process. Lower latency, simpler IPC. |
| Qt5 → Qt6 migration | Audio subsystem substantially refactored. Windows, macOS, Linux compatibility improved. |
| Fixed decode depth = 2 | Multi-pass iterative decoding (signal subtraction) is capped at depth 2. Testing by the fork found that depth > 2 yielded marginal additional decodes and was removed. |
| SNR decoder optimisations (v2.5.0) | Low-SNR decode improvements by contributor punk-kaos. Specific algorithmic details not publicly documented in release notes. |
| Heartbeat/CQ loop rewrite (v2.5.0) | Major rewrite of the automatic heartbeat and CQ beacon loop logic. |
| Auto-reply prioritisation (v2.5.2) | Priority queue for auto-replies: directed commands (`SNR?`, `HEARING?`, `INFO?`, `GRID?`, `STATUS?`, `QUERY MSGS`, `QUERY CALL`) are served before general messaging traffic. |

---

## Submode parameters

JS8Call operates at a 12000 Hz sample rate. All submodes use 79 symbols per frame (58 data + 21 sync, identical to FT8), 8-FSK modulation with tone spacing equal to the baud rate. The five submodes are defined in `commons.h` and parameterised in `JS8Submode.cpp`.

### Parameters from source

The following values are taken directly from the source files.

**`commons.h` constants:**

| Symbol | Value | Notes |
|--------|-------|-------|
| `JS8_RX_SAMPLE_RATE` | 12000 | Samples per second |
| `JS8_NUM_SYMBOLS` | 79 | Total symbols per frame (data + sync) |
| `JS8A_SYMBOL_SAMPLES` | 1920 | Samples per symbol, Normal mode |
| `JS8A_TX_SECONDS` | 15 | T/R cycle length, Normal mode |
| `JS8A_START_DELAY_MS` | 500 | Transmit start delay, Normal mode |
| `JS8B_SYMBOL_SAMPLES` | 1200 | Samples per symbol, Fast mode |
| `JS8B_TX_SECONDS` | 10 | T/R cycle length, Fast mode |
| `JS8B_START_DELAY_MS` | 200 | Transmit start delay, Fast mode |
| `JS8C_SYMBOL_SAMPLES` | 600 | Samples per symbol, Turbo mode |
| `JS8C_TX_SECONDS` | 6 | T/R cycle length, Turbo mode |
| `JS8C_START_DELAY_MS` | 100 | Transmit start delay, Turbo mode |
| `JS8E_SYMBOL_SAMPLES` | 3840 | Samples per symbol, Slow mode |
| `JS8E_TX_SECONDS` | 30 | T/R cycle length, Slow mode |
| `JS8E_START_DELAY_MS` | 500 | Transmit start delay, Slow mode |
| `JS8I_SYMBOL_SAMPLES` | 384 | Samples per symbol, Ultra mode |
| `JS8I_TX_SECONDS` | 4 | T/R cycle length, Ultra mode |
| `JS8I_START_DELAY_MS` | 100 | Transmit start delay, Ultra mode |
| `JS8_ENABLE_JS8I` | 0 | Ultra mode **disabled by default** |

**SNR thresholds from `JS8Submode.cpp`:**

| Submode | `rxSNRThreshold` (dB) | `rxThreshold` (internal units) |
|---------|----------------------|-------------------------------|
| JS8A (Normal) | −24 | 10 |
| JS8B (Fast) | −22 | 16 |
| JS8C (Turbo) | −20 | 32 |
| JS8E (Slow) | −28 | 10 |
| JS8I (Ultra) | −18 | 50 |

### Derived parameters

Baud rate = `JS8_RX_SAMPLE_RATE / SYMBOL_SAMPLES`. Occupied bandwidth ≈ 7 × baud rate (8 tones, tone spacing = baud rate, outer tones span 7 × spacing).

| Submode | Name | Baud rate | T/R cycle | SNR threshold | Approx. BW | Default |
|---------|------|-----------|-----------|---------------|------------|---------|
| JS8A | Normal | 6.25 | 15 s | −24 dB | ~44 Hz | Yes |
| JS8B | Fast | 10.0 | 10 s | −22 dB | ~70 Hz | Yes |
| JS8C | Turbo | 20.0 | 6 s | −20 dB | ~140 Hz | Yes |
| JS8E | Slow | 3.125 | 30 s | −28 dB | ~22 Hz | Yes |
| JS8I | Ultra | 31.25 | 4 s | −18 dB | ~219 Hz | No |

---

## The baud-rate / SNR trade-off rule

The JS8 submode data reveals a consistent empirical rule: **each halving of baud rate yields approximately 2 dB of additional SNR sensitivity**, at fixed modulation, FEC code, and frame structure. Specifically:

| Step | Baud ratio | SNR delta |
|------|-----------|-----------|
| JS8C (20) → JS8B (10) | ÷2 | −20 → −22 = 2 dB |
| JS8B (10) → JS8A (6.25) | ÷1.6 | −22 → −24 = 2 dB |
| JS8A (6.25) → JS8E (3.125) | ÷2 | −24 → −28 = 4 dB |

The JS8A → JS8E step gives 4 dB rather than 2 dB. This is consistent with the longer T/R cycle (30 s vs 15 s, a doubling) compounding the baud-rate improvement: cycle-length doubling gives approximately 3 dB (coherent integration doubling), which adds to the ~1 dB from the baud-rate halving at this step.

The JS8I Ultra mode (31.25 baud) is disabled by default. JS8Call's user base optimises for sensitivity (chat, DX); JS8I is the mode where throughput is preferred over sensitivity. HPX takes precisely this position (file transfer over chat), which is why the HPX mode ladder covers baud rates from 31.25 (BPSK31) to 500 (QPSK500) — the JS8I-and-faster territory.

---

## SNR floor comparison: JS8 versus HPX modes

The table below estimates the HPX mode SNR floors using the JS8 empirical rule (2 dB per baud-rate doubling), adjusted for modulation order and FEC type. These are design targets to be verified by testbench measurement.

| Mode | Baud rate | Modulation | FEC | Estimated SNR floor |
|------|-----------|------------|-----|--------------------|
| JS8E (Slow) | 3.125 | 8-FSK | LDPC(174,91) | −28 dB (measured) |
| JS8A (Normal) | 6.25 | 8-FSK | LDPC(174,91) | −24 dB (measured) |
| JS8C (Turbo) | 20 | 8-FSK | LDPC(174,91) | −20 dB (measured) |
| JS8I (Ultra) | 31.25 | 8-FSK | LDPC(174,91) | −18 dB (measured) |
| BPSK31 | 31.25 | BPSK | RS(255,223) | ~−10 to −14 dB (estimated) |
| BPSK63 | 62.5 | BPSK | RS(255,223) | ~−8 to −12 dB (estimated) |
| BPSK250 | 250 | BPSK | RS(255,223) | ~−2 to −6 dB (estimated) |
| QPSK500 | 500 | QPSK | RS(255,223) | ~0 to +4 dB (estimated) |

The gap between JS8I and BPSK31 at equal baud rate (~4–8 dB) is attributable to the FEC difference: LDPC(174,91) + belief-propagation decoding versus RS(255,223). This is consistent with the LDPC coding gain of ~5–8 dB documented in `docs/wsjtx-analysis.md`.

The testbench SNR sweep range (−30 to +30 dB) is set to span the full JS8E floor at the low end and QPSK500's operating point at the high end.

---

## ARQ and relay protocol commands

JS8Call defines a text-based directed-message protocol transmitted over the FT8 air interface. Commands are callsign-addressed strings exchanged within the 79-symbol frame payload. The following commands are documented through JS8Call source code and release notes and are directly relevant to HPX Phase 2 design:

| Command | Direction | Purpose |
|---------|-----------|---------|
| `SNR?` | A → B | Request B's measured received SNR of A's most recent transmission. B replies with `SNR: −12`. |
| `HEARING?` | A → B | Request a list of callsigns B has received recently. Used to identify relay candidates. |
| `QUERY MSGS` | A → relay | Request delivery of messages buffered for A at a store-and-forward relay node. |
| `QUERY CALL` | A → net | Ask whether any station has recently heard a specific callsign. Relay-propagated. |
| `INFO?` | A → B | Request station metadata: capabilities, software version, grid square. |
| `GRID?` | A → B | Request B's Maidenhead grid locator. |
| `STATUS?` | A → B | Request B's current operating status. |

**Priority ordering (v2.5.2):** The improved fork auto-replies to directed commands in priority order: directed commands (`SNR?`, `HEARING?`, `INFO?`, etc.) take precedence over general outgoing messages. This prevents a busy store-and-forward node from dropping network query responses due to outgoing message queuing.

### Mapping to HPX Phase 2 design

| JS8Call mechanism | HPX equivalent |
|---|---|
| `SNR?` / `SNR:` exchange | Diagnostic probe outside a session. HPX ACK-UP/ACK-DOWN implicitly carry this during a session. An explicit SNR diagnostic command is a candidate addition to Phase 2.1. |
| `HEARING?` / callsign list | HPX Phase 2.5 peer query. The JS8Call approach is text-encoded over-air; HPX uses a binary wire format (docs/peer-query-relay-wire.md) for efficiency. |
| `QUERY MSGS` | HPX Phase 2.5 store-and-forward delivery request. Functionally identical concept. |
| `QUERY CALL` | HPX Phase 2.5 network query propagation (bounded by hop limit). |
| Heartbeat/CQ beacon | HPX §97.119 identification beacon. The beacon frame is the vehicle for periodic callsign transmission. |

---

## Heartbeat and identification beacon

JS8Call's heartbeat loop broadcasts a station beacon at a configurable interval. The beacon contains callsign, grid square, and optional status. The improved fork substantially rewrote this loop in v2.5.0 to improve timing reliability and handling of collisions with incoming directed messages.

**Regulatory mapping:** This pattern directly fulfils §97.119 (FCC) and equivalent CEPT/Ofcom identification requirements. The beacon is not a separate identification transmission — it is the primary station announcement mechanism that also serves as identification. HPX's session heartbeat/beacon frame should adopt the same principle: the identification is embedded in normal protocol operation rather than being a separate periodic override.

The UK identification interval (15 minutes per Ofcom Amateur Licence) differs from the US/CEPT 10-minute interval. The HPX heartbeat interval must be user-configurable to accommodate both jurisdictions. JS8Call's configurable heartbeat interval is the correct model.

---

## Store-and-forward relay

JS8Call implements opportunistic store-and-forward relay: when a station receives a directed message not addressed to itself but to a known callsign, it can buffer the message and attempt delivery when the target station next appears on the channel. `QUERY MSGS` allows a station to explicitly poll a relay node for buffered messages.

Key design observations:
- The relay function requires no pre-established path or routing table. Any station that hears the message and knows the destination can attempt forwarding.
- Relay introduces duplicate-message risk (multiple stations hear the original and all attempt forwarding). JS8Call addresses this through sequence numbering in the message format.
- Relay trust is implicit: any station can relay to any destination. HPX Phase 2.6 adds trust-policy enforcement at each hop.

---

## Fixed decode depth = 2: empirical finding

The JS8Call-improved fork capped multi-pass iterative decoding at depth 2 after testing showed that passes 3+ yielded marginal additional decodes. This finding is consistent with WSJTX's multi-pass analysis (see `docs/wsjtx-analysis.md`). The signal subtraction approach gains most of its benefit from the second pass; deeper passes face diminishing returns because:

1. After two passes, most strong-to-moderate signals have been decoded and subtracted.
2. Remaining residual signals are at or below the detection threshold; additional passes do not add enough SNR to decode them.
3. Decoder false-alarm rate increases with additional passes, degrading precision.

**HPX implication:** The HPX receiver design should not plan for more than two decode passes. This also limits worst-case decoder latency to 2 × (one LDPC decode time), which is predictable and bounded.

---

## Working conclusions for OpenPulseHF

### Protocol design

- **The `SNR?` / `SNR:` command pattern** (one-RTT link quality measurement) is a useful diagnostic primitive for HPX outside of active sessions. This should be a separate diagnostic frame type rather than being embedded in the ARQ ACK taxonomy, which is already occupied by ACK-UP/ACK-DOWN/NACK.

- **The `HEARING?` → callsign-list pattern** maps directly to HPX Phase 2.5 peer query. The HPX binary wire format is more efficient but the semantic purpose is identical: a station discovers its neighbours' reachability via a trusted intermediate.

- **Store-and-forward relay requires duplicate suppression** from the start. JS8Call's sequence-number approach works for text messages. HPX relay frames should carry a session-unique message ID to enable deduplication at each relay hop, consistent with the Phase 2.6 "duplicate-suppressed" relay forwarding requirement.

- **Heartbeat beacon = identification**: do not add a separate identification frame. Embed the callsign in the standard session heartbeat/beacon frame. Make the interval configurable (default 10 minutes; 15 minutes for UK operation).

### Speed ladder

- The JS8Call submode ladder (3.125 → 6.25 → 10 → 20 → 31.25 baud) demonstrates that useful SNR gains come from reducing baud rate, not from shortening frames. HPX's current ladder (BPSK31 → BPSK250 → QPSK500) covers a wider range of baud rates and is appropriate for file transfer.

- JS8I (31.25 baud, same as BPSK31) is disabled by default in JS8Call because the user base does not want it. This is a relevant data point: users of a chat protocol prefer sensitivity. HPX users (file transfer) will prefer throughput. Default mode in HPX should be a mid-ladder choice (BPSK250 or QPSK250) rather than BPSK31.

### SNR floor and testbench

- The testbench SNR sweep range of −30 to +30 dB is calibrated to span the JS8E floor (−28 dB) at the low end. This enables direct comparison of HPX modes against the JS8 reference thresholds.
- BPSK31 with RS(255,223) FEC is expected to floor at approximately −10 to −14 dB, roughly 10 dB above the JS8I threshold at equal baud rate, due to the LDPC vs RS FEC gap. The testbench will produce the measured value.
- If LDPC is adopted for a future HPX mode at 31.25 baud, the expected floor improvement is 5–8 dB, potentially reaching −15 to −22 dB — approaching JS8I territory.

### Decode depth

- Cap the HPX receiver at depth 2. The JS8Call-improved empirical finding confirms this is sufficient. Do not spend engineering time on depth-3+ decoding.
