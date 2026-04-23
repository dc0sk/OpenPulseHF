---
project: openpulse
doc: docs/vara-research.md
status: living
last_updated: 2026-04-23
---

# VARA Research

This note captures publicly available technical information about VARA that may be useful as background research for OpenPulse.

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

## Linked technical files found but not readable here

ROSModem links to several potentially important technical documents, but their contents were not directly readable through the available fetch path because they are hosted on MEGA and require client-side interaction.

The unread technical links include:

- VARA TNC Commands 11-01-2026
- VARA KISS Interface 13-02-2024
- VARA Documentation
- VARA Huffman compression
- VARA HF 4.7 Quick Guide

These links are still useful as pointers for manual follow-up research.

Primary index source:

- https://rosmodem.wordpress.com/2011/01/10/ros-2/

## Working conclusions for OpenPulse

- It is reasonable to treat VARA as a practical reference point for product shape and user expectations rather than as a publicly specified protocol.
- Publicly verifiable material supports studying the following themes:
  - adaptive or multi-rate modem operation
  - local TNC-style command/data interfaces
  - HF versus FM product variants
  - 500 Hz versus wider-band operating modes in user workflows
- Publicly verifiable material does not currently provide enough detail here to claim a full open protocol description.
- Any future interoperability or compatibility work should be based only on legally and technically defensible public documentation or first-principles design work.

