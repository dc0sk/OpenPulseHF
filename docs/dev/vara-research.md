---
project: openpulsehf
doc: docs/vara-research.md
status: living
last_updated: 2026-05-01
---

# VARA Research

This note captures publicly available technical information about VARA that may be useful as background research for OpenPulseHF.

The goal is not protocol emulation and not legal interpretation. It is a source-graded summary of what can be learned from public-facing material.

## Confirmed public facts

The items below are directly supported by public pages that were readable during research.

### Product family

- The public VARA product family includes VARA HF, VARA FM, VARA SAT, VARA Chat, and VARA Terminal.
- Public download listing on 2026-04-23 showed these package versions:
  - VARA HF v4.9.0
  - VARA FM v4.4.0
  - VARA SAT v4.4.5
  - VARA Chat v1.4.3

Sources:

- https://rosmodem.wordpress.com/2011/01/10/ros-2/
- https://downloads.winlink.org/VARA%20Products/

### VARA HF claims from the author page

- VARA HF is described as a high performance HF modem based on OFDM modulation.
- It is described as operating within a 2400 Hz SSB bandwidth.
- The public author page claims an uncompressed user data rate up to 5629 bps at S/N 14.5 dB at 4 kHz.
- The same page states a symbol rate of 37.5 baud with 52 carriers.

Source:

- https://rosmodem.wordpress.com/2017/09/03/vara-hf-modem/

### Winlink integration and ownership boundary

- Winlink publicly states that VARA products are hosted on Winlink download servers.
- Winlink also states the files are maintained by Jose Nieto Ros, remain third-party products, and are not managed by the Winlink team.
- Winlink site content and public gateway notices show operational use of both VARA HF and VARA FM in the broader Winlink ecosystem.

Sources:

- https://winlink.org/content/vara_products_now_downloadable_here
- https://www.winlink.org/

### Publicly visible integration parameters

- Public setup material for VARA FM shows a localhost control pattern using host address 127.0.0.1.
- The same setup material documents TNC command port 8300 and data port 8301 for local integration.
- The same source describes VARA as a sound card TNC in the user-facing integration model.
- Public setup guidance distinguishes 1200 and 9600 bps FM radio data paths and notes that wide or narrow FM system settings are selected accordingly.

Source:

- https://www.masterscommunications.com/products/radio-adapter/dra/vara-primer.html

### Public evidence of bandwidth selections in VARA ecosystem tools

- VarAC, a separate amateur-radio application that explicitly states it leverages the VARA protocol, publicly advertises 500 Hz and 2300 Hz support in its feature list.

Source:

- https://www.varac-hamradio.com/

### Claims and parameters from local VARA specification PDF (Rev 2.0.0, 2018-04-05)

The specification PDF contains detailed technical parameters extracted here for reference.

**System objectives:**
- Half-duplex ARQ within 2.4 kHz SSB bandwidth.
- Adaptive 11 speed levels, net data rate 60–7536 bps.
- PAPR 9 dB for DATA blocks (constant across all speed levels); PAPR 6 dB for ACK bursts.
- FCC §97.307 compliance: symbol rate 42 baud per carrier (≤ 300 baud limit).
- Automatic timing adjustment for PTT turnaround; guarantees 100 ms PTT switching budget.

**Equipment compatibility parameters:**
- Frequency offset tolerance: ±50 Hz between station frequencies.
- Frequency stability requirement: transmitter drift < 0.5 Hz/second for SSB.
- Audio interface: 16-bit soundcard at 48 kHz sample rate minimum.
- SDR latency: protocol accommodates typical SDR processing delays.
- PTT keying methods: CAT command, serial port COM RTS/DTR, VOX.
- Host interface: TCP (Telnet) connection to external applications.

**OFDM frame structure:**
- DATA frame: 196 OFDM symbols × 26.66 ms/symbol = 5225 ms total frame duration.
- Each OFDM symbol: 21.33 ms FFT block + 5.33 ms cyclic prefix = 26.66 ms.
- 52 carriers per symbol; 4 pilot symbols per carrier used for time/frequency synchronization.
- Levels 1–8 use differential PSK modulation; Levels 9–11 use QAM requiring channel equalization.
- Levels 9–11 include 1020 additional equalization pilots per DATA frame.

**Speed level table (from specification):**

| Level | Symbol Rate | Carriers | Modulation | Bytes/Packet | Net Data Rate | User Data Rate |
|-------|-------------|----------|-----------|--------------|---------------|----------------|
| 1 | 42 | 52 | BPSK | 20 | 60 bps | 50 bps |
| 2 | 42 | 52 | BPSK | 32 | 126 bps | 105 bps |
| 3 | 42 | 52 | BPSK | 71 | 260 bps | 217 bps |
| 4 | 42 | 52 | BPSK | 150 | 529 bps | 441 bps |
| 5 | 42 | 52 | BPSK | 308 | 1070 bps | 892 bps |
| 6 | 42 | 52 | BPSK | 626 | 2143 bps | 1786 bps |
| 7 | 42 | 52 | 4PSK | 1257 | 3214 bps | 2641 bps |
| 8 | 42 | 52 | 8PSK | 1887 | 4287 bps | 3511 bps |
| 9 | 42 | 52 | 8PSK | 2951 | 5024 bps | 4115 bps |
| 10 | 42 | 52 | 16QAM | 3690 | 6281 bps | 4972 bps |
| 11 | 42 | 52 | 32QAM | 4429 | 7536 bps | 5802 bps |

Note: FEC is Turbo codification with varying redundancy to produce the 11 speed levels. The specification notes this places VARA close to the Shannon limit.

**DATA block structure:** 1-byte control (changeover flag) + 2-byte CRC16 + information payload.

**ACK frame structure:**
- Two parallel FSK modulations of 48 tones each, 31 symbols per modulation.
- Total: 62 FSK symbols, each 26.66 ms. ACK frame duration: 842 ms.
- Parallel FSK gives 6 dB PAPR; at 100 W peak TX, ACK power is 50 W RMS.
- Session-unique ACK codes prevent false commands when two stations share a dial frequency.

**ACK frame types:**

| Type | Meaning |
|------|---------|
| START | Wake up the gateway station |
| ACK1 | DATA block received (maintain rate) |
| ACK2 | DATA block received + request rate increase |
| ACK3 | DATA block received + request rate decrease |
| NACK | DATA block failed |
| BREAK | Changeover (IRS requests ISS role) |
| REQ | ACK failed; request retransmission |
| QRT | End of session |

**KISS interface parameters:**
- Default KISS port: 8100. Control port: 8300. Data port: 8301.
- Three KISS frame modes: byte 0 = standard AX.25, byte 1 = 7-char callsign AX.25, byte 2 = generic data.
- Collision avoidance: 0.3-persistence CSMA (transmit with 30% probability when channel appears clear).
- KISS in VARA HF uses 500 Hz Levels 1–4 for broadcast; interoperable with any 500/2300/2750 VARA station.

Source (local document set):

- docs/VARA Doc/VARA Specification.pdf

### Local KISS and quick-guide integration details

- The KISS interface document describes three KISS frame modes keyed by second byte (`0` standard AX.25, `1` 7-char callsign AX.25 variant, `2` generic data).
- The same KISS document states default KISS port `8100` and says KISS is available in VARA HF, FM, and SAT.
- The quick guide states default control TCP port `8300` for typical VARA app integration and discusses multiple-folder/multiple-port setups for concurrent VARA applications.
- The quick guide states three HF bandwidth modes in that release family: 500 Hz (Narrow), 2300 Hz (Standard), and 2750 Hz (Tactical).

Sources (local document set):

- docs/VARA Doc/VARA KISS Interface.pdf
- docs/VARA Doc/VARA 4.7 quick guide.pdf

### Local speed-level tables (HF/FM)

- The HF levels sheet (v4.0.0) provides modulation/rate tables for VARA HF 2300 and VARA HF 500, including FSK/PSK/QAM progression by level.
- The FM levels sheet (v3.0.5) provides Wide/Narrow level tables with symbol rate, carrier count, modulation family, and net rate progression.

Sources (local document set):

- docs/VARA Doc/VARA HF v4.0 Levels.pdf
- docs/VARA Doc/VARA FM v3.0.5 Levels.pdf

## Public but lower-confidence observations

The items below are technically interesting but rely on user comments, third-party interpretation, or indirect evidence rather than stable product documentation.

### Comment-sourced performance statements

- Public comment threads on the VARA HF page describe a free or evaluation mode with lower speeds and a paid registration unlocking higher performance.
- A public comment by the author states that, under suitable conditions, the 2300 mode starts taking advantage over the 500 mode above about 450 bps, with example upper figures of about 7050 bps for the 2300 mode and about 1540 bps for the 500 mode.

Source:

- https://rosmodem.wordpress.com/2017/09/03/vara-hf-modem/

### Public signal-analysis discussion

- Public comments include third-party observations describing recordings that appear to show multi-tone signaling such as 48-tone or 52-carrier behavior.
- These comments are useful as hints, but they are not enough to treat as definitive protocol specification.

Source:

- https://rosmodem.wordpress.com/2017/09/03/vara-hf-modem/

### Peer-to-peer design statements

- Public comment replies by the author state that VARA was designed for peer-to-peer connection.
- That is relevant for understanding the intended operating model, but it is still comment-level evidence rather than a formal protocol document.

Source:

- https://rosmodem.wordpress.com/2017/09/03/vara-hf-modem/

### PACTOR design notes

A dedicated PACTOR research note is maintained at docs/pactor-research.md and covers all four PACTOR generations, Memory-ARQ, Huffman compression, the 1.25 s ARQ cycle, concatenated FEC, and working conclusions for OpenPulseHF. The following is a brief summary for cross-reference.

**PACTOR-I (1990, open protocol):**
- OEVSV PACTOR wiki describes PACTOR-I style ARQ timing and packet structure, including 1.25 s cycle framing and CRC16.
- Control signals CS1–CS4 encode ACK, NAK, speed change, and direction change (BREAK).
- Adaptive speed switching (100/200 baud) and Huffman compression are built-in.
- Memory-ARQ soft-combines failed receive attempts before decoding.
- Source: https://wiki.oevsv.at/wiki/PACTOR (historical; treat as context not normative)

**PACTOR-4 (2011, proprietary, vendor PDF):**
- SCS vendor PDF describes 10 adaptive speed levels (SL1–SL10).
- ARQ cycle: 1.25 s short-path, 1.4 s long-path, triple variants for extreme delays.
- Modulation: robust mode (spread DQPSK), normal mode (coherent PSK/QAM with training sequences and adaptive equalisation), chirp mode.
- FEC: concatenated convolutional coding, rate 1/2 and 5/6 puncturing; RAKE receiver for multipath combining.
- Packet structure: preamble + symbol data field + status byte + CRC16 (CCITT).
- Source: https://www.p4dragon.com/download/PACTOR-4%20Protocol.pdf
- Confidence note: vendor technical description, not an open specification. Useful as design reference, not as VARA protocol evidence.

## Local document set now reviewed

The previously unread MEGA-linked materials were provided locally and reviewed from `docs/VARA Doc/`.

Reviewed files:

- docs/VARA Doc/VARA 4.7 quick guide.pdf
- docs/VARA Doc/VARA HF Tactical v4.3.0.pdf
- docs/VARA Doc/VARA KISS Interface.pdf
- docs/VARA Doc/VARA HUFFMAN COMPRESSION.pdf
- docs/VARA Doc/VARA Specification.pdf
- docs/VARA Doc/VARA HF v4.0 Levels.pdf
- docs/VARA Doc/VARA FM v3.0.5 Levels.pdf

Notes:

- The included PowerPoint file (`VARA HF Modem.ppt`) did not yield reliable plain-text extraction in this environment.
- The extracted PDF text is sufficient to capture high-level architecture, interface facts, and published rate/level claims for research context.

## FEC algorithm comparison

VARA uses Turbo codes. PACTOR-4 uses concatenated convolutional codes. OpenPulseHF Phase 1 uses Reed-Solomon. This section documents the reasoning.

### Reed-Solomon (current choice)

- Block code over GF(2^8). RS(255, k) corrects up to (255-k)/2 symbol errors per 255-byte block.
- OpenPulseHF uses RS(255, 223) with ECC_LEN=32, correcting up to 16 byte errors per block.
- Strengths: simple deterministic implementation, correction capacity known before decoding, well-understood failure modes, excellent burst error correction within one block boundary.
- Weakness: burst errors longer than 16 consecutive bytes in the same block exceed correction capacity. Mitigation: block interleaver must be used alongside RS to convert burst errors into dispersed symbol errors.
- The 255-byte block size is not accidental: it matches the current frame payload limit (1 byte length field), meaning one RS block fits in one frame. This simplifies implementation at the cost of inflexibility.
- Applicable: when burst error length is bounded and an interleaver is used; when deterministic decoder latency matters; when Raspberry Pi class hardware is the target.

### Convolutional codes + Viterbi decoder

- Streaming code: each output bit depends on the current and previous K-1 input bits (constraint length K).
- Rate R = 1/2 (one data bit produces two coded bits) is typical; puncturing achieves higher rates.
- ARDOP uses rate 2/3 convolutional codes with Viterbi decoding.
- PACTOR-4 uses concatenated convolutional codes with rate 1/2 and 5/6 puncturing.
- Strengths: streaming (no fixed block latency), excellent random error correction, well-understood implementation.
- Weakness: less effective against burst errors than RS without interleaving; decoder complexity grows with constraint length K.
- Applicable: where streaming operation is important and an interleaver is used; widely deployed in legacy HF systems.

### Turbo codes

- Two recursive systematic convolutional encoders with an interleaver between them; iterative decoding (Turbo decoding) approaches the Shannon limit.
- VARA uses Turbo codes; the different redundancy levels across the 11 speed levels suggest varying code rate rather than modulation order alone.
- Strengths: near-Shannon performance; a single FEC scheme covers a wide range of rates by adjusting puncturing; built-in interleaving in the encoder structure.
- Weakness: high decoder complexity; iterative decoding has variable latency and is not deterministic in timing; sensitive to early termination of iterations.
- On a Raspberry Pi 4, Turbo decoding at practical HF block sizes is feasible but consumes significantly more CPU than RS. A benchmark is needed before committing.
- Applicable: when approaching Shannon capacity is the priority and decoder latency variability is acceptable.

### LDPC (Low-Density Parity Check)

- Sparse parity-check matrix; belief-propagation decoder also approaches Shannon limit.
- Used in 802.11n (Wi-Fi), DVB-S2, WiMAX.
- Strengths: near-Shannon performance, highly parallelisable decoder, well-suited to GPU acceleration.
- Weakness: encoder complexity varies by construction; belief-propagation decoder has similar iteration-count variability to Turbo; long block lengths (thousands of bits) needed for best performance.
- No LDPC implementation currently exists in the codebase. Applicable as a future option if GPU acceleration is pursued.

### Rateless / fountain codes (Raptor, LT)

- Generate an unlimited stream of encoded symbols from a source block; receiver decodes once enough symbols are received regardless of which ones arrive.
- Strengths: ideal for lossy channels with unknown erasure patterns; no ARQ needed for the FEC layer itself; works well when RTT is long (as in HF with propagation delays and PTT turnaround overhead).
- Weakness: requires large source blocks (hundreds to thousands of symbols) for good efficiency; decoder is more complex; less applicable to small HF frame payloads.
- Applicable as an optional session-layer strategy for large file transfer (HPX file mode) rather than as a replacement for per-frame FEC.

### Summary

For OpenPulseHF's current scope (Raspberry Pi, small frames, bounded burst errors with interleaving), Reed-Solomon is the right first implementation. Turbo codes are worth benchmarking on Pi hardware before committing to for HPX high-throughput profiles. LDPC becomes relevant if GPU acceleration is pursued.

### Phase 3.2 evaluation result

No pure-Rust Turbo code (iterative BCJR/MAP) implementation exists as a crate on crates.io. The evaluation used a **rate-1/2 convolutional code (K=3, generators G={7,5} octal, hard-decision Viterbi decoder)** as the closest achievable benchmark proxy — the same class of streaming FEC used by ARDOP and PACTOR-4.

**Benchmark results** (500-byte payload, 50 repetitions; `cargo test -p openpulse-core --test fec_comparison -- --nocapture`):

| Channel BER | RS post-decode BER | ConvCodec post-decode BER |
|---|---|---|
| 0.1% (0.001) | 0.000000 | 0.000000 |
| 1% (0.01) | **0.4973** | **0.0004** |
| 5% (0.05) | 0.4973 | 0.2040 |

**CPU time** (1000-byte payload encode+decode): RS = 1.7 ms, ConvCodec = 6.3 ms (3.8× overhead — well within budget).

**Interpretation:**
- At 1% random channel BER, RS fails almost completely (0.497 ≈ 50% post-BER) because 1% bit-level noise produces ~20 byte errors per 255-byte block — exceeding the 16-byte RS correction capacity. The convolutional code corrects isolated bit errors via the Viterbi trellis, achieving post-BER < 0.04%.
- RS is designed for **burst errors** (via interleaver); the above comparison with random noise is unfair to RS. For burst-error channels (Gilbert-Elliott), RS+interleaver will outperform K=3 Viterbi.
- The 3.8× CPU overhead is acceptable on Raspberry Pi 4.

**Decision: ConvCodec ACCEPTED as optional FEC for HPX high-rate profiles.**

The convolutional codec (`openpulse_core::conv::ConvCodec`) is added as an optional alternative to `FecCodec` (RS). Recommended usage:
- Use `FecCodec` (RS) as the default for all HF channel types with an interleaver.
- Use `ConvCodec` (convolutional/Viterbi) as an experimental alternative for AWGN-dominant paths (e.g., line-of-sight VHF/UHF links) where random noise dominates over burst fading.
- A K=7 convolutional code with soft-decision Viterbi would give ~5 dB additional gain but is deferred pending a pure-Rust implementation or suitable crate.

## PSK31 design principles

PSK31 was designed by Peter Martinez (G3PLX) in 1998 and is the direct inspiration for OpenPulseHF's BPSK family. Understanding the original design choices explains several architectural decisions.

### Differential encoding (DBPSK)

PSK31 encodes data as phase *transitions*, not absolute phase. A 0-bit leaves the phase unchanged; a 1-bit shifts the phase by 180°. This is DBPSK (differential BPSK). Consequences:
- No carrier phase recovery loop is needed at the receiver. This eliminates a major source of synchronisation failure on fading HF channels where carrier phase is constantly disrupted.
- The cost is approximately 3 dB SNR relative to coherent BPSK. At the SNR levels typical of HF (often 0–10 dB), this trade is accepted to gain robustness.
- OpenPulseHF inherits this choice for all current BPSK modes. Coherent detection is a future option for high-rate modes where the 3 dB gain is worth the complexity.

### Raised-cosine symbol shaping

PSK31 applies a raised-cosine amplitude envelope to each symbol (α = 1.0, full cosine rise/fall). This is an amplitude shaping operation, not a root-raised-cosine matched filter pair. Consequences:
- Out-of-band emissions fall as 1/f³ rather than 1/f for rectangular pulses, giving near-zero spectral sidelobes.
- Adjacent-channel interference to other stations is extremely low, making PSK31 compatible with busy HF sub-bands.
- The full-cosine shape (α = 1.0) means the signal bandwidth equals the symbol rate. BPSK31 occupies approximately 31 Hz of bandwidth — narrower than a single voice channel by two orders of magnitude.
- ISI sensitivity increases relative to rectangular shaping, but at 31 baud the symbol period (32 ms) far exceeds typical HF multipath delay spread (0.5–2 ms), so ISI is not a practical concern.

### Varicode character encoding

PSK31 uses a variable-length prefix-free character encoding called Varicode. Each ASCII character maps to a unique binary codeword; codewords for common characters are short, rare characters are long. Character boundaries are marked by two or more consecutive zero bits.

Key practical consequence: English text averages approximately 1.7 bits/character in Varicode versus 7 bits in standard ASCII. This makes PSK31 useful for keyboard-to-keyboard text at 31 baud — effective text throughput is roughly 50 WPM.

OpenPulseHF does **not** use Varicode. The payload encoding is raw bytes (8 bits/byte). This is correct for binary data transfer and is consistent with how BPSK63 and higher operate (PSK63+ modes in amateur practice also use byte encoding, not Varicode). The architecture table in this document explicitly records this per-mode.

### Timing recovery

Single-carrier PSK requires symbol timing recovery at the receiver. The Gardner Timing Error Detector (Gardner TED) is the standard algorithm for BPSK/QPSK: it estimates timing error from three consecutive samples without requiring a decision on the current symbol. It is decision-directed (low implementation complexity) and works well at the SNRs encountered in HF operations. The architecture should document which TED is implemented in the BPSK plugin.

## Single-carrier versus OFDM: comparative analysis

This section provides a structured comparison for use when evaluating future mode additions to HPX.

| Property | Single-carrier (OpenPulseHF) | OFDM (VARA, PACTOR-3+) |
|----------|------------------------------|------------------------|
| PAPR | ~0 dB (BPSK), 3–4 dB (QPSK) | 9–12 dB (52 carriers) |
| Full-power operation | Yes — no amplifier back-off | No — back-off required |
| Transceiver requirements | Any SSB radio | Requires linear PA; IQ balance matters for high-order QAM |
| Multipath resistance | Requires equalization above ~250 baud; good below 100 baud without EQ | Excellent — cyclic prefix absorbs multipath delay spread |
| Frequency offset tolerance | Good — single AFC corrects one carrier | Moderate — DFT leakage if offset not corrected |
| Receiver complexity | Low — correlator per symbol | Moderate — FFT + per-subcarrier equalisation |
| Peak throughput in given BW | Moderate | High — subcarrier overlap via OFDM |
| Implementation complexity | Low | Moderate–high |
| PAPR VARA note | — | VARA explicitly targets 9 dB DATA / 6 dB ACK PAPR |

Conclusion: single-carrier is the correct choice for the current project scope (reliability, portability, Raspberry Pi class hardware). OFDM becomes attractive when peak throughput in wide-band (2300 Hz) profiles is the primary goal and linear amplifiers are available. HPX2300 profile planning should revisit this choice.

## Working conclusions for OpenPulseHF

- The main product goal is an independent OpenPulse protocol designed from scratch to compete on performance and robustness.
- It is reasonable to treat VARA as a practical reference point for product shape and user expectations rather than as a publicly specified protocol.
- The local specification and guide PDFs add useful implementation-oriented context (ARQ model, timing/bandwidth/rate tables, KISS/TCP integration defaults), but they still do not define an open interoperability standard.
- Publicly verifiable material supports studying the following themes:
  - adaptive or multi-rate modem operation
  - local TNC-style command/data interfaces
  - HF versus FM product variants
  - 500 Hz versus wider-band operating modes in user workflows
- Publicly available material still does not provide enough rigor for a clean-room, bit-accurate protocol clone claim.
- Compatibility modes targeting VARA or PACTOR-4 should be treated as optional follow-on work and require legal checks before any implementation begins.
- Any future interoperability or compatibility work should be based only on legally and technically defensible public documentation or first-principles design work.

