---
project: openpulse
doc: docs/overview.md
status: living
last_updated: 2026-04-23
---

# Overview

OpenPulse is a cross-platform software modem for sending and receiving data over amateur radio (HF and VHF) via a soundcard.

## Inspiration

OpenPulse is inspired by established HF digital mode ecosystems including:

- VARA
- PACTOR
- ARDOP

See docs/vara-research.md for a public-source technical summary of VARA-related findings collected during project research.

## Project shape

OpenPulse is implemented as a Cargo workspace with split responsibilities:

- Core protocol and traits
- Audio backends
- Modem engine
- CLI frontend
- Modulation plugins

See docs/architecture.md for crate-level detail and data flow.
