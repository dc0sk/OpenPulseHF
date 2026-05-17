---
project: openpulsehf
doc: docs/pactor-research.md
status: living
last_updated: 2026-05-01
---

# PACTOR Research

This note captures publicly available and historically documented technical information about the PACTOR protocol family as background research for OpenPulseHF.

PACTOR represents the most technically mature line of HF ARQ protocols in the amateur radio community, spanning three decades of development from 1990 to the present. It is the direct ancestor of the operating model used by VARA and ARDOP. Understanding the progression from PACTOR-I through PACTOR-4 reveals which design decisions converged across all HF ARQ systems and which remain open choices.

**Legal boundary:** PACTOR-II, III, and IV are proprietary protocols owned by SCS (Special Communications Systems GmbH & Co. KG, Hanau, Germany). The SCS documentation is a vendor technical description, not an open specification. No claim is made here of bit-level protocol accuracy. No compatibility implementation is in scope without explicit legal review.

Sources used in this note are identified per section. PACTOR-I is substantially documented in public amateur radio literature and the OEVSV wiki. PACTOR-4 is covered by a vendor technical description PDF available at the SCS website. PACTOR-II and III are known primarily through secondary sources (operator reports, feature comparisons, SCS marketing material) and this note rates that material accordingly.

---

## Protocol generation overview

| Version | Released | Modulation | Speed | FEC | Hardware required |
|---------|----------|-----------|-------|-----|------------------|
| AMTOR | 1979 | FSK | 100 baud | Repetition / ARQ | Any TNC |
| PACTOR-I | 1990 | FSK | 100/200 baud | CRC + Memory-ARQ | Any TNC (open) |
| PACTOR-II | ~1994 | DPSK | 200 baud | Convolutional + Viterbi | SCS hardware TNC only |
| PACTOR-III | ~2000 | Multi-carrier DPSK | up to 2400 bps | Convolutional + Viterbi | SCS hardware TNC only |
| PACTOR-4 | ~2011 | Adaptive PSK/QAM | up to ~5000 bps | Concatenated convolutional | SCS Dragon TNC only |

---

## AMTOR — the predecessor

AMTOR (Amateur Teleprinting Over Radio) was developed by Peter Martinez (G3PLX) in 1979, based on SITOR (Simplex Teletype Over Radio), a maritime communications protocol. It is the direct ancestor of PACTOR.

AMTOR operates at 100 baud FSK with 170 Hz tone shift. It defines two modes:
- **Mode A (ARQ):** Three-character blocks, 250 ms TX cycle, IRS sends a 70 ms ACK or NAK. Error-free or retransmit.
- **Mode B (FEC):** Continuous transmission with each character sent twice at timed intervals; no feedback from receiver.

AMTOR's main limitation is the 3-character block size, which gives very low efficiency when the channel is good. Each successful block carries only ~20 useful bits; the ACK/NAK overhead dominates throughput. PACTOR was designed to fix this.

AMTOR is publicly documented, open, and fully implemented in many legacy TNCs. It is named explicitly in FCC §97.309.

---

## PACTOR-I

### History and authorship

PACTOR-I was developed by Ulrich Strate (DL4MAR) and Hans-Peter Helfert (DF4KV) and released in 1990. The protocol was initially published openly and implemented in the SCS PTC and in many third-party TNCs (KAM, PK-232, etc.). It became the de-facto standard for Winlink HF email in the 1990s.

Sources:
- OEVSV PACTOR wiki page (https://wiki.oevsv.at/wiki/PACTOR) — historical description; site indicates last edit many years ago, treat as historical context
- General amateur radio literature (widely reproduced)

### Modulation and timing

- Carrier: two FSK tones, 1600 Hz and 1800 Hz centre frequencies (200 Hz shift), within SSB audio passband
- Symbol rate: 100 baud (Level 1) or 200 baud (Level 2)
- Occupied bandwidth: approximately 200 Hz
- ARQ cycle time: 1.25 seconds per cycle

The 1.25-second cycle is the fundamental timing unit inherited by PACTOR-II, III, and IV. It was chosen to accommodate typical HF propagation delays and PTT switching times while giving adequate throughput.

### Packet structure

Each PACTOR-I packet contains:
- Status byte (control signalling: CS1–CS4)
- Data payload: 96 bits (Level 1) or 192 bits (Level 2)
- CRC-16 (CCITT polynomial)

Within one 1.25-second ARQ cycle:
- ISS (Information Sending Station) transmits one data packet (~0.96 s).
- Brief guard interval (~0.05 s) for PTT switching.
- IRS (Information Receiving Station) transmits a short ACK/NAK (~0.24 s).

### Control signals (CS1–CS4)

PACTOR-I defines four control signal types exchanged during the IRS reply slot:

| Signal | Meaning |
|--------|---------|
| CS1 | Packet received correctly; advance to next |
| CS2 | Packet received correctly; request speed change |
| CS3 | Packet received with errors; retransmit |
| CS4 | Packet received with errors; request direction change |

Direction change (BREAK in VARA terminology) follows CS4: IRS and ISS roles swap, allowing bidirectional data exchange within one session.

### Memory-ARQ

Memory-ARQ is PACTOR-I's most significant technical innovation over AMTOR. When a packet is received with CRC errors, the receiver stores the received signal (or a soft representation of it). If the retransmitted packet also has errors, the receiver combines the two attempts before decoding. This soft-decision combining effectively doubles the integration time and can recover packets that neither attempt alone decoded correctly.

Memory-ARQ provides approximately 3–6 dB improvement in sensitivity under typical HF fading conditions compared to hard-decision ARQ with discard on error. It is particularly effective on slow-fading channels where the channel state is correlated between the original transmission and the retransmission.

This technique is directly relevant to OpenPulseHF: HPX could implement Memory-ARQ as a soft receiver enhancement without changing the wire protocol. The retransmission trigger (NACK) already exists in the ACK taxonomy; the receiver simply needs to retain the previous attempt.

### Huffman compression

PACTOR-I includes built-in Huffman compression optimised for ASCII text. The compression table is tuned to the statistical distribution of English text. Typical compression ratio: 1.5:1 to 2.5:1 for natural language, with higher ratios for repetitive content.

Compression is transparent to the application: the TNC compresses before transmission and decompresses after reception. The compressor must be synchronised at both ends; a mis-synchronised compressor produces garbage output silently. For this reason, PACTOR negotiates compression state in the connection handshake.

Note: VARA also implements Huffman compression (see docs/VARA Doc/VARA HUFFMAN COMPRESSION.pdf). OpenPulseHF's compression requirement (session layer, negotiated) is consistent with this established pattern.

### Adaptive speed (Level 1 / Level 2)

PACTOR-I can switch between 100 baud and 200 baud mid-session. The speed-up request is encoded in the CS2 control signal. This is the earliest form of adaptive rate control in HF ARQ, and the model (rate embedded in ACK signal) directly anticipates VARA's ACK1/ACK2/ACK3 design.

### Throughput

PACTOR-I effective user data throughput:
- Level 1, uncompressed: approximately 100–120 bps
- Level 2, uncompressed: approximately 180–220 bps
- Level 2, with Huffman compression (English text): approximately 300–400 bps

These figures are modest by modern standards but represent a large improvement over AMTOR (~50 bps effective).

---

## PACTOR-II

Source confidence: secondary (operator reports, feature comparisons, SCS marketing material). Parameters here should be treated as approximately correct, not bit-level precise.

### Modulation

PACTOR-II moved from FSK to Differential PSK (DPSK), maintaining the same 200 baud symbol rate and 1.25-second ARQ cycle. DPSK provides approximately 6 dB SNR advantage over FSK at the same symbol rate.

Two modulation orders are used:
- DBPSK (1 bit/symbol) for lower rate levels
- DQPSK (2 bits/symbol) for higher rate levels

Differential encoding avoids the need for carrier phase recovery — the same design choice made in PSK31 (G3PLX, 1998) and in OpenPulseHF's BPSK plugin.

### FEC

PACTOR-II introduced convolutional FEC with Viterbi decoding, replacing PACTOR-I's CRC-only approach. The convolutional code provides forward error correction, reducing the retransmission rate and improving effective throughput.

Memory-ARQ is retained and enhanced: PACTOR-II's Viterbi decoder can combine soft metrics from multiple received attempts, exploiting path diversity more effectively than hard-decision combining.

### Hardware TNC requirement (significant)

PACTOR-II is the first generation that requires the SCS hardware TNC (PTC series). The SCS PTC-II was released alongside PACTOR-II. This is a deliberate proprietary lock-in: the protocol is owned by SCS and the hardware TNC is the only authorised implementation.

Consequences:
- PACTOR-II hardware cost: approximately €400–600 for the SCS PTC-II (as of mid-2020s).
- Sound card TNC implementations (VARA, ARDOP) cannot be used for PACTOR-II on-air.
- This cost barrier drove adoption of VARA for operators seeking higher throughput than PACTOR-I.

Backward compatibility: PACTOR-II stations can negotiate down to PACTOR-I with any PACTOR-I TNC, ensuring legacy interoperability.

### Throughput

PACTOR-II effective user data throughput: approximately 400–800 bps uncompressed; higher with compression. Exact figures depend on channel conditions and SCS TNC model.

---

## PACTOR-III

Source confidence: secondary (operator reports, SCS marketing, feature comparisons).

### Modulation

PACTOR-III introduced multi-carrier modulation using 18 differential PSK subcarriers. This is functionally similar to OFDM with a small number of subcarriers, providing multipath resistance through frequency diversity without the high PAPR of a large OFDM system.

Occupied bandwidth: approximately 2.2 kHz (fits within a standard SSB channel).

### Speed levels

6 adaptive speed levels, using DBPSK, DQPSK, and D8PSK at varying code rates. Automatic level selection based on received signal quality, similar in principle to VARA's 11-level adaptive system.

### Backward compatibility

PACTOR-III stations negotiate down to PACTOR-II or PACTOR-I, maintaining the full PACTOR backward compatibility chain. This is a significant engineering constraint: the 1.25-second ARQ cycle is preserved through all generations to allow this negotiation.

### Hardware TNC requirement

PACTOR-III requires the SCS PTC-IIpro, PTC-IIIpro, or PTC-IIIusb. Hardware cost: approximately €700–900. This is significantly higher than the typical amateur operator's entry point for HF digital modes.

### Throughput

PACTOR-III effective user data throughput: approximately 800–2400 bps uncompressed depending on channel conditions and speed level.

---

## PACTOR-4

Source: SCS vendor technical description PDF "The PACTOR-4 Protocol — A Technical Description", © 2011 SCS Spezielle Communications Systeme GmbH & Co. KG. This document is locally available at `docs/VARA Doc/PACTOR-4 Protocol.pdf`. It is a vendor technical description, not a full open specification, but it contains sufficient detail to characterise the architecture and key parameters precisely.

**Correction note:** An earlier version of this document incorrectly described PACTOR-4 as using multi-carrier modulation for higher speed levels. The PDF is explicit: PACTOR-4 uses *single-carrier* modulation for all speed levels except SL1 (which is a 2-tone chirp). This is the opposite of VARA's all-OFDM approach and makes PACTOR-4 architecturally closer to OpenPulseHF than VARA.

### General architecture

PACTOR-4 (P4) extends PACTOR-3 toward single-carrier modulation with adaptive equalisation and RAKE receivers for constructive path superposition. The 10 speed levels are treated as distinct sub-protocols ("waveforms"), each with a different modulation and coding profile. Many ARQ mechanisms are adopted from PACTOR-2/3.

The PDF describes three mode families:

**Chirp mode (SL1 long packets only):** Two-tone 2-carrier DBPSK at 66.66 Bd with a 294.0 Hz/s linear frequency chirp on both carriers simultaneously. Carrier frequencies before chirp: 550 Hz and 1530 Hz. Lower carrier delayed by T/2. This is the most robust mode, designed for the worst HF conditions. Cycle time 3.75 s (triple long).

**Robust mode (SL2–4, and SL1 short):** Single-carrier spread DQPSK with spreading factor (SF) 16 or 8. Symbol rate after spreading: always 1800 symbols/s. Information-carrying DPSK symbol rate: 1800/16 = 112.5 or 1800/8 = 225 symbols/s. RAKE receivers exploit the spread structure for multipath path combining.

**Normal mode (SL5–10):** Single-carrier coherent PSK/QAM at 1800 Bd, shaped by a quasi-RRC pulse filter (roll-off 0.33, optimised for side lobe suppression). Training sequences (CAZAC-16) are inserted every data block for channel estimation. Adaptive equalisation enables coherent detection.

### Speed level table (from PDF, Table 3.1)

| SL | Modulation | SF | R (long) | R (short) | N bytes (long) | N bytes (short) |
|----|-----------|-----|----------|----------|---------------|----------------|
| 1 | DBPSK, Chirp | 1 | 1/2 | — | 25 | — |
| 2 | DQPSK | 16 | 1/2 | — | 43 | — |
| 3 | DQPSK | 16 | 5/6 | — | 72 | — |
| 4 | DQPSK | 8 | 5/6 | — | 144 | — |
| 5 | BPSK | 1 | 1/3 | 1/3 | 206 | 43 |
| 6 | BPSK | 1 | 5/6 | (=SL5) | 517 | 43 |
| 7 | QPSK | 1 | 5/6 | 1/3 | 1034 | 87 |
| 8 | 8-PSK | 1 | 5/6 | 1/3 | 1552 | 131 |
| 9 | 16-QAM | 1 | 5/6 | 1/3 | 2069 | 175 |
| 10 | 32-QAM | 1 | 5/6 | (=SL9) | 2587 | 175 |

SF = spreading factor (SF > 1 = robust mode spread).
R = code rate (ratio of data bits to total transmitted bits after FEC).
N = total packet length including status byte and 2 CRC bytes (user payload = N − 3).
SL1–4 have no short packet variant. SL6 short = SL5 short; SL10 short = SL9 short.

Symbol rate (normal and robust modes, after spreading): **1800 symbols/s**.
Chirp mode symbol rate: 66.66 symbols/s (T = 15 ms).
Centre frequency (SCS DR-7800 modem): 1500 Hz. Bandwidth: approximately 2400 Hz at −25 dB.

### Packet structure (bit layer)

All packets (except chirp header): Datablock | Status byte | CRC-16

**Status byte bit field: E / S1 S0 / M2 M1 M0 / C1 C0**

| Field | Bits | Meaning |
|-------|------|---------|
| E | 1 | Extended status follows at packet start when set |
| S1 S0 | 2 | 00 = nothing; 01 = BK (break/changeover); 10 = QRT (session end); 11 = SPUG (speedup/long-cycle suggestion) |
| M2 M1 M0 | 3 | Compression mode (see table below) |
| C1 C0 | 2 | Packet counter (same as P1–P3) |

**Compression mode table (M2 M1 M0):**

| Code | Compression mode |
|------|----------------|
| 000 | ASCII (8-bit transparent, no compression) |
| 001 | Huffman (English text) |
| 010 | Huffman "swapped" (reversed capitalisation) |
| 011 | Reserved (planned P4 compression mode) |
| 100 | Pseudo-Markov, German |
| 101 | Pseudo-Markov, German, swapped |
| 110 | Pseudo-Markov, English |
| 111 | Pseudo-Markov, English, swapped |

The Pseudo-Markov modes are language-model-based compression: they use a first-order Markov model of German or English character transition probabilities for arithmetic coding. This is significantly more effective than Huffman for text in the target language (typical ratio 3:1 vs 2:1 for Huffman). The compression mode is selected per-packet and signalled in the status byte, allowing the TNC to switch modes dynamically based on detected content.

**User data lengths (bytes) per SL (pure payload, excluding status byte and CRC):**

| SL | Long | Short |
|----|------|-------|
| 1 | 22 | — |
| 2 | 40 | — |
| 3 | 69 | — |
| 4 | 141 | — |
| 5 | 203 | 40 |
| 6 | 514 | 40 |
| 7 | 1031 | 84 |
| 8 | 1549 | 128 |
| 9 | 2066 | 172 |
| 10 | 2584 | 172 |

**CRC:** CCITT-CRC16 with 0xFFFF pre-assigned register and complement at end. Lower 8 bits transmitted first.

### Channel coding (FEC)

Robust and normal modes use a **partially punctured concatenated convolutional code**.

The encoder consists of two equal recursive systematic component encoders (constraint length k = 4, i.e. 4 delay stages) with generator polynomials (15, 13)₈. This structure is explicitly identified in the PDF as identical to the (13, 15)₈ encoder used in **3GPP W-CDMA (UMTS)** — making PACTOR-4's FEC the same encoder topology as a cellular turbo code.

Signal flow after encoding:
- V0: original data bits (systematic, transmitted un-punctured in all rates)
- V1: parity output from encoder C1 (non-interleaved data path)
- V2: parity output from encoder C2 (interleaved data path)

Encoder C1 receives data directly. Encoder C2 receives data after bit-interleaving. This is the defining structural feature of a turbo code encoder. Whether the decoder uses full iterative turbo decoding (BCJR/MAP algorithm) or a simplified concatenated Viterbi strategy is not specified in the PDF.

**Puncturing matrices:**

Rate 1/2 (maximum protection, SL1–2, SL5):
```
V0: 1 1   (all data bits transmitted)
V1: 1 0   (odd parity bits from C1)
V2: 0 1   (even parity bits from C2)
Result: 2 data bits / 4 total bits = rate 1/2
```

Rate 5/6 (high throughput, SL3–4, SL6–10):
```
V0: 1 1 1 1 1 1 1 1 1 1   (all data bits)
V1: 1 0 0 0 0 0 0 0 0 0   (every 10th parity bit from C1)
V2: 0 0 0 0 1 0 0 0 0 0   (every 10th parity bit from C2)
Result: 10 data / 12 total bits = rate 5/6
```

Rate 1/3 is used for SL5 and SL7 short packets (all parity bits retained, no puncturing of V1 and V2).

**Chirp mode FEC:** uses the PACTOR-2/3 convolutional code with constraint length k = 9, rate 1/2. Generator polynomials: G1 = 111101011, G2 = 101110001.

### Interleaver

PACTOR-4 implements block-bit interleaving within each packet. The interleaver design rule from the PDF:

```
INTERLEAVER_DEPTH >= 2 * (k - 1)
```

For k = 5 (normal/robust mode code): depth ≥ 8. At chirp mode (k = 9): depth = 16.

The permutation algorithm (reproduced from the PDF):

```c
S = 1;                    // auxiliary variable
P = 0;                    // initial permutation pointer
M = INTERLEAVER_DEPTH;

for (I = 0; I < PACKET_SIZE; I++) {
    OUTPUT_PACKET[I] = INPUT_PACKET[P];
    P = P + M;
    if (P > PACKET_SIZE)
        P = S++;
}
```

This is a stride-based interleaver: each output bit is taken from position P = I × M (mod PACKET_SIZE, with wrap-around by incrementing S). For PACKET_SIZE divisible by INTERLEAVER_DEPTH, this produces a complete permutation with interleaving depth equal to M.

### Normal mode packet structure (symbol layer)

Normal mode packets (SL5–10) interleave training sequences between data blocks for continuous channel tracking:

```
Header | T | Datablock | T | Datablock | T | ... | T
```

- Header: one of 19 Chu19 sequences (spreading factor SF = 8), used for packet variant detection and synchronisation.
- T: 32-symbol CAZAC-16 training sequence inserted between every data block.
- CAZAC = Constant Amplitude Zero AutoCorrelation. These sequences have ideal autocorrelation properties for channel estimation, the same property used in LTE/5G pilot sequences.
- CAZAC-16 sequence: two cyclic repetitions of C[1–16] with alternating complex conjugation (odd training sequences use C, even use C*).

Data block counts:
- Short packet: 6 data blocks × 176 symbols = 1056 data symbols total.
- Short_BreaKin: 4 data blocks × 210 symbols = 840 data symbols.
- Long packet: 24 data blocks × 207 symbols = 4968 data symbols.

### Robust mode packet structure

Robust mode (SL2–4) packets:

```
Header | R | Datablock
```

- Header: one of 19 Chu19 sequences, always spread by factor 16.
- R: phase reference symbol (phase 0), spread by factor 8 or 16.
- Datablock: DQPSK payload symbols spread by factor 8 or 16.

Spreading is performed by complex multiplication with a fixed spreading sequence (published in the PDF for both SF8 and SF16).

### Modulator

The P4 symbol filter is a quasi-RRC (root raised-cosine) filter with roll-off 0.33, optimised for lower side lobes and better orthogonality compared to a standard RRC. The filter has 129 coefficients at 16 samples per symbol. The full coefficient table is published in the PDF. This is a matched-filter design: the same filter shape should be applied at both transmitter (pulse shaping) and receiver (matched filter), giving near-zero ISI at symbol sampling instants under AWGN.

### ARQ cycle timing

- Short path (≤ 2500 km, typical): 1.25 s cycle, or 3 × 1.25 s = 3.75 s (long cycle).
- Long path (> 2500 km): 1.4 s cycle, or 3 × 1.4 s = 4.2 s (long cycle).
- Cycle times are identical to PACTOR-3.
- SPUG (status byte S1 S0 = 11): "Long-Cycle/Speedup Suggestion" — the IRS can request a cycle length change or rate change via the status byte, analogous to VARA's ACK2/ACK3.

The "short packet" mechanism: at SL5–10, short packets carry substantially less data but are sent within the same cycle time as long packets. Short packets are used when little data is queued or when a direction change is imminent, improving response latency without changing cycle timing.

### Hardware TNC (SCS Dragon)

PACTOR-4 requires the SCS Dragon hardware TNC (approximately €1200–1500). The coherent single-carrier demodulator with CAZAC-based channel estimation, RAKE receiver, and adaptive equalisation requires DSP compute that is feasible on dedicated hardware and on modern PCs, but was impractical for sound card implementations in 2011. On 2024-era CPUs (including Raspberry Pi 4) these operations are feasible in software — this is one reason VARA and ARDOP have displaced PACTOR-4 for most amateur use cases despite PACTOR-4's technical maturity.

### Throughput

From the speed level table, maximum user data per long packet at SL10: 2584 bytes. At cycle time 1.25 s: 2584 × 8 / 1.25 = **16,538 bps raw bit rate** before FEC overhead. After rate 5/6 FEC: 16,538 × 5/6 ≈ **13,782 bps**. In practice, overhead (header, training sequences, ARQ retransmissions) reduces the effective user data rate substantially. Realistic peak effective throughput on excellent channels is approximately 4,000–5,000 bps, consistent with secondary-source reports.

---

## Key design patterns across the PACTOR family

These patterns recur in VARA and ARDOP and should inform OpenPulseHF's HPX design:

### 1.25-second ARQ cycle as the fundamental unit

Every PACTOR generation uses 1.25 s (or 1.4 s for long-path) as the cycle time. This was not accidental: it represents a practical lower bound on the HF half-duplex turnaround budget including propagation delay, PTT switching (100 ms), and receiver settling. VARA and ARDOP inherit similar cycle structures.

OpenPulseHF's turnaround timing budget (50 ms PTT release, 150 ms RX acquisition as defined in requirements) is consistent with the PACTOR/VARA/ARDOP precedent. The ARQ cycle duration for HPX modes should be explicitly specified and benchmarked against this constraint.

### Persistent CRC-16 (CCITT)

All PACTOR versions use CRC-16 (CCITT) at the packet layer. VARA uses CRC-16 in its DATA block. OpenPulseHF uses CRC-16 (CCITT). This is the correct choice for HF frame integrity: CRC-16 provides adequate error detection at small frame sizes with minimal overhead.

### Adaptive rate via ACK-embedded feedback

Every PACTOR generation embeds rate-change requests in the ACK/NAK signal (CS2 in PACTOR-I; similar mechanisms in later versions). VARA uses ACK2 (up) and ACK3 (down). This is the validated design pattern for HPX.

### Memory-ARQ (soft combining of retransmissions)

PACTOR-I introduced this; PACTOR-II and IV enhanced it with soft Viterbi combining. The principle is applicable to OpenPulseHF's NACK/retransmit flow independent of the wire protocol. A receiver that retains the previous failed attempt and combines it with the retransmission before FEC decoding gains 3–6 dB without any change to the transmitter or protocol.

### Huffman compression at TNC layer (below application)

PACTOR-I, VARA both implement transparent Huffman compression at the TNC layer. OpenPulseHF's session-layer compression (Phase 2.7) follows this pattern. The key operational requirement is compression negotiation at handshake time and a "don't compress if result is larger" rule — both documented in requirements.md.

### Direction change (BREAK) as first-class operation

Every PACTOR version supports ISS/IRS role reversal within a session. VARA supports it via the BREAK ACK type. OpenPulseHF HPX defines BREAK in the ACK taxonomy. This is essential for interactive two-way data flows without session teardown/reconnect overhead.

---

## PACTOR versus VARA versus ARDOP: ecosystem context

| Property | PACTOR-III | PACTOR-4 | VARA HF | ARDOP 2000 |
|----------|-----------|----------|---------|-----------|
| Hardware required | SCS TNC ~€700 | SCS Dragon ~€1400 | PC sound card | PC sound card |
| Software cost | Licence included with TNC | Licence included | Shareware (~€70) | Free/open |
| Peak throughput (~2 kHz) | ~2400 bps | ~5000 bps effective | ~7536 bps | ~2000 bps |
| Modulation | Multi-carrier DPSK | **Single-carrier** adaptive PSK/QAM (BPSK→32QAM); SL1: 2-tone chirp | OFDM 52-carrier BPSK–32QAM | 4FSK (control) + OFDM DBPSK/DQPSK |
| PAPR | Low (multi-carrier, fewer subcarriers than VARA) | Low (single-carrier) | High (9 dB) | Low (4FSK) / moderate (OFDM) |
| FEC | Convolutional + Viterbi | Concatenated RSC code (turbo encoder topology, k=4, rate 1/2 or 5/6); + block interleaver | Turbo codes | Convolutional k=5–9 + Viterbi |
| Channel coding | — | CAZAC-16 pilot training sequences for coherent detection | OFDM sync pilots + EQ pilots | 4FSK preamble |
| Compression | Huffman | Huffman + Pseudo-Markov (German/English) | Huffman | None documented |
| Winlink compatible | Yes (primary) | Yes | Yes | Yes |
| Open spec | PACTOR-I only | Vendor PDF (partial) | Vendor PDF (partial) | Yes (fully open) |
| Backward compat | Yes (I/II/III/IV chain) | Yes — negotiates down to P1 | No (VARA only) | No (ARDOP only) |

The backward compatibility chain (PACTOR-I → IV) is PACTOR's unique strength. Any PACTOR-4 station can still talk to a PACTOR-I TNC from 1990. VARA and ARDOP start fresh with no legacy compatibility.

---

## Working conclusions for OpenPulseHF

- **Memory-ARQ is worth implementing.** No wire protocol change required — only the receiver retains the previous soft attempt and combines it with the retransmission before FEC decoding. Gain: 3–6 dB on fading channels. Add to HPX mid-term roadmap alongside the NACK/retransmit flow.

- **The 1.25 s ARQ cycle is the convergent design point across all HF ARQ systems.** PACTOR-I through IV, VARA, and ARDOP all use approximately this cycle time. HPX timing budget should be designed within this envelope.

- **PACTOR-4 is single-carrier — architecturally closer to OpenPulseHF than VARA.** This is the key finding from the PDF. VARA achieves higher peak throughput through OFDM; PACTOR-4 achieves competitive throughput through single-carrier coherent detection with adaptive equalisation. OpenPulseHF's path to higher throughput (HPX2300) can follow the PACTOR-4 trajectory (single-carrier + equaliser) rather than the VARA trajectory (OFDM).

- **PACTOR-4's FEC encoder is a turbo code topology.** Two recursive systematic convolutional encoders with a bit interleaver between them — identical to 3GPP UMTS. The interleaver is integrated with the encoder, not a separate post-FEC stage. This means the interleaver depth constraint (depth ≥ 2(k−1)) is directly tied to the code constraint length. OpenPulseHF's separate RS + block interleaver design achieves the same goal differently; the PACTOR-4 approach is more elegant for convolutional FEC.

- **Pseudo-Markov compression is more effective than Huffman.** PACTOR-4's M2 M1 M0 compression field shows it supports language-model compression (Pseudo-Markov, German and English) in addition to Huffman. For the primary use case of email-style text data over Winlink, Pseudo-Markov compression achieves approximately 3:1 ratios versus 2:1 for Huffman. OpenPulseHF's session-layer compression requirement should consider whether to support multiple compression algorithms with per-packet negotiation.

- **CAZAC training sequences for coherent detection are applicable to HPX normal mode.** When HPX moves to coherent detection at higher speed levels (Phase 3+ territory), CAZAC sequences are the standard choice for channel estimation. They have ideal autocorrelation properties and the same design is used in LTE/5G.

- **The stride-based block interleaver algorithm from PACTOR-4 is directly implementable.** The algorithm is a simple modular arithmetic permutation — about 10 lines of code. The depth constraint (≥ 2(k−1)) for pairing with a convolutional code of constraint length k is a well-established rule. OpenPulseHF's block interleaver implementation should follow this depth rule when paired with a convolutional FEC.

- **RAKE receiver for spread modes.** If OpenPulseHF adds a robust spread-spectrum mode (analogous to PACTOR-4's SL2–4 robust mode), RAKE combining is the correct multipath diversity combiner. This is Phase 4+ territory.

- **PACTOR-I is the only open PACTOR variant.** PACTOR-II through IV must be treated as proprietary. The vendor PDF for PACTOR-4 is detailed enough to understand the architecture but does not constitute an open specification for implementation. No compatibility work without legal review.

- **The hardware TNC cost barrier explains the market shift.** VARA achieves competitive throughput (higher than PACTOR-4) with a €70 shareware licence and a PC sound card versus a €1400 hardware TNC. OpenPulseHF is free/open-source and achieves the same sound card TNC model — a further advantage in the same competitive tier.
