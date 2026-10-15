---
project: openpulsehf
doc: docs/overview.md
status: living
last_updated: 2026-10-15
---

# Overview

OpenPulseHF is a software modem for sending and receiving data over amateur radio (HF and VHF)
through a sound interface and an SSB transceiver. It is a Rust workspace; modulation modes are
plugins, and every test runs against a loopback backend without audio hardware.

## Release 1

The first release is **Release 1 = `v0.17.0`**, not yet tagged. The latest tagged release is
**v0.16.0** (2026-07-18). The plan, milestones and decision log are in
[dev/project/workplan.md](dev/project/workplan.md); `1.0` stays reserved for the full
[1.0 criteria](dev/project/release-1.0-criteria.md).

**In Release 1:**

- `openpulse-daemon` (binary `openpulse-server`) and `openpulse-cli` (binary `openpulse`)
- the ARDOP-compatible TNC (`openpulse-ardop`, binary `openpulse-tnc`), for Pat
- the `hpx_hf` rate ladder, SL1–SL14, and its plugins: MFSK16 (with its return channel), BPSK,
  QPSK250-D, OFDM52, FSK4 ACK
- each rung's FEC, session compression, PTT backends, station ID, and the signed classical
  (Ed25519) handshake

**Not in Release 1** (in the code, but disabled or experimental and without a release gate): mesh,
repeater, relay, discovery/JS8, file transfer, QSY, FreeDV auth, Winlink B2F and the CMS gateway,
KISS, the post-quantum handshake, GPU acceleration, the operator panel and TUI, and the plugins not
on `hpx_hf` (`psk8`, `64qam`, `scfdma`, `pilot`).

## Profiles and the ladder

There are two session profiles, both on the one `hpx_hf` ladder:

- **`fast`** (default): SL1–SL14, from MFSK16 (~9 bps) through coded BPSK31–250 and QPSK250-D to
  OFDM52 with up to 64QAM and LDPC r≈8/9 (7710 bps), up to ≈2031 Hz occupied.
- **`robust`**: the same ladder capped at SL6 (QPSK250-D, 437 bps), ≤500 Hz, single-carrier, all
  coded.

A `fast` and a `robust` station interoperate. The ten earlier profile names were deleted on
2026-10-01 without aliases; their modes remain selectable as fixed modes (`[modem] mode`). The ARDOP
TNC floors its ladder at SL2. The full rung table is in [mode-fec-ladder.md](mode-fec-ladder.md) and
the [README](../README.md#the-hpx_hf-ladder-release-1).

## Wire format

The v0.17 wire format is declared in
[dev/design/protocol-wire-spec.md](dev/design/protocol-wire-spec.md). Since 2026-10-10 (#1062)
BPSK63, BPSK100 and BPSK250 (SL3–SL5) use a PN-63 preamble; BPSK31 keeps its `--++` preamble and
QPSK250-D its designed sequence. That is one of several breaks against v0.16.0: every frame is
now whitened (#1027, fixed in #1148; v0.16.0 has no whitener) and the handshake is now binary (#1147), so a v0.16.0 station and a current
station do not decode each other's frames on any rung. Update both ends of a link together.

## Status (2026-10-15)

- M0 (governance reset) and M1 (v0.17 wire format) are done.
- M2 (release path) is done in code; its on-air follow-ups wait for the radios.
- **M3 is next: on air on 2 m** (rpi51 + IC-9700 ↔ rpi53 + FT-818, 144.640 MHz). The test stages
  are virtual audio → hardware loopback → 2 m → HF, and HF is tried only with the release candidate.
- M4 cuts `v0.17.0`.

## Inspiration

OpenPulseHF is inspired by established HF digital-mode systems, including VARA, PACTOR and ARDOP.
[dev/research/vara-research.md](dev/research/vara-research.md) summarises public-source findings
about VARA collected during project research.

## Project shape

The workspace is split into:

- core protocol and traits (`openpulse-core`), DSP (`openpulse-dsp`), modem engine
  (`openpulse-modem`), audio backends (`openpulse-audio`), radio control (`openpulse-radio`) and
  configuration (`openpulse-config`)
- front-ends: the daemon, the CLI and the ARDOP TNC, plus experimental ones (KISS TNC, Winlink
  gateway, panel, TUI)
- modulation plugins under `plugins/`
- test and simulation tools under `apps/` (link simulator, test matrix, testbench)

See [dev/design/architecture.md](dev/design/architecture.md) for crate-level detail and data flow,
and [dev/crate-map.md](dev/crate-map.md) for the full crate map.
