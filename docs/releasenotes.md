---
project: openpulsehf
doc: docs/releasenotes.md
status: living
last_updated: 2026-05-15
---

# Release Notes

## Unreleased

- Adaptive-rate ACK-UP progression now skips unmapped reserved profile rungs,
	avoiding transitions into `None` mode mappings during active sessions.
- The SNR-gated admission path remains limited to HPX wideband-HD SL13 -> SL14;
	non-wideband profiles keep expected ACK-UP progression.

- Project docs are now organized under docs/ with a consistent format.
- Pull requests now run docs frontmatter validation checks.
- Docs touched in pull requests now receive automatic last_updated stamping.

## v0.1.0

- First public OpenPulseHF release.
- Introduced plugin-based modem architecture in a Cargo workspace.
- Included BPSK mode support and loopback-based testing path.
