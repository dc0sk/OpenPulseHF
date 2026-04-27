---
project: openpulsehf
doc: docs/vara-research.md
status: living
last_updated: 2026-04-27
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

- The specification describes VARA HF as a proprietary shareware system.
- It describes VARA as half-duplex ARQ over HF with adaptive speed levels.
- It states operation within 2.4 kHz SSB bandwidth and claims net data rates from 60 to 7536 bps.
- It describes OFDM data frames with cyclic prefix and Turbo-based FEC concepts.
- It describes ACK bursts as parallel FSK signaling and lists ACK frame types (`START`, `ACK1`, `ACK2`, `ACK3`, `NACK`, `BREAK`, `REQ`, `QRT`).
- It describes host/software integration over TCP and notes Windows compatibility.

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

### Historical PACTOR design notes (German-language wiki)

- The OEVSV PACTOR page includes a detailed (historical) description of PACTOR-I style ARQ timing and packet structure, including 1.25 s cycle framing and CRC16 usage.
- It describes control-signal concepts (`CS1`..`CS4`), direction switching, and link teardown behavior in a way that is useful for understanding legacy HF ARQ design patterns.
- It also discusses adaptive speed switching (100/200 baud), Huffman compression, and Memory-ARQ concepts.
- The page appears old (site indicates last edit many years ago), so values there should be treated as historical context, not normative specification for current proprietary PACTOR variants.

Source:

- https://wiki.oevsv.at/wiki/PACTOR

### PACTOR-4 technical document (vendor PDF)

- The SCS document describes PACTOR-4 as an extension of PACTOR-3 using single-carrier modulation for most speed levels, with adaptive equalization and RAKE receiver concepts.
- It states 10 adaptive speed levels (SL1..SL10), with differing modulation and coding profiles by level.
- It describes ARQ cycle timing of 1.25 s (short) and 3 x 1.25 s (long), with long-path mode values of 1.4 s and 3 x 1.4 s.
- It states packet structure as preamble plus symbol data field, with status byte and CRC16 (CCITT) in the bit-layer data structure.
- It describes robust mode (spread DQPSK), normal mode (coherent PSK/QAM with training sequences), and chirp mode as a special narrowband case.
- It documents channel coding as concatenated convolutional coding with puncturing options including rate 1/2 and 5/6 patterns.

Source:

- https://www.p4dragon.com/download/PACTOR-4%20Protocol.pdf

Confidence note:

- This is a vendor technical description for PACTOR-4 (2011), not a VARA document. It is useful as historical HF ARQ/modem design reference material, but should not be treated as VARA protocol evidence.

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

