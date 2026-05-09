---
project: openpulsehf
doc: README.md
status: living
last_updated: 2026-05-09
---

# OpenPulseHF

> A plugin-based HF software modem written in Rust — built for reliable data over real ionospheric channels.

[![CI](https://github.com/dc0sk/OpenPulseHF/actions/workflows/ci.yml/badge.svg)](https://github.com/dc0sk/OpenPulseHF/actions/workflows/ci.yml)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE)

OpenPulseHF is a full-stack HF digital radio modem: modulation plugins, ARQ session management, Winlink/B2F compatibility, AX.25/KISS bridging, a channel-simulation test harness, and a live signal-path testbench GUI — all in a single Rust workspace, no external C dependencies.

---

## Why OpenPulseHF?

HF data links are hostile: ionospheric fading, burst noise, Doppler spread, and narrow bandwidth. OpenPulseHF was designed from the start to cope — with adaptive rate ladders that respond per-direction to real channel conditions, a collaborative frequency-agility protocol that moves the link to a better channel without operator intervention, and a post-quantum-capable handshake that protects session identity today and in the post-quantum era.

Every feature ships with a deterministic, hardware-free test suite and a parametric channel-simulation harness validated against published Watterson and Gilbert-Elliott models.

---

## Key features

### Modulation and waveforms

| Mode | Baud | Typical use |
|---|---|---|
| BPSK31 / BPSK63 / BPSK100 / BPSK250 | 31–250 | Low-speed, maximum sensitivity |
| QPSK125 / QPSK250 / QPSK500 / QPSK1000 | 125–1000 | Mid-range, balanced throughput |
| 8PSK500 / 8PSK1000 | 500–1000 | High throughput on good paths |
| FSK4-ACK | 100 baud, 200 ms | Ultra-compact ACK frames |

Rate adaptation steps across these modes automatically in response to ACKs and NACKs — independently per direction, so an asymmetric path (good downlink, noisy uplink) is handled without penalising the better direction.

### ARQ session layer

- HPX state machine with eight ACK types and eleven speed levels
- ChirpFallback: after three consecutive NACKs at the lowest adaptive level, falls back to a narrowband chirp-spread waveform
- Segmentation and reassembly (SAR) handles payloads up to 64 KB in a single session
- Optional LZ4 session-layer compression; negotiated in the signed handshake

### QSY frequency agility (FF-1) — first to market

Two stations can collaboratively negotiate a move to a less congested frequency without operator input. Each side scans candidate frequencies via rigctld, exchanges SNR readings over the existing data channel, votes for the best common frequency, and switches on a coordinated timer.

- Five-frame ASCII protocol (QSY_REQ / QSY_LIST / QSY_VOTE / QSY_ACK / QSY_REJECT)
- Every frame is Ed25519-signed; tampering returns an explicit `InvalidSignature` error
- Fully operator-configurable: disabled by default, enabled per-session via `[qsy]` config section
- Tested against a mock rigctld TCP server — no hardware required for CI

### Post-quantum in-band handshake

OpenPulseHF supports three signing modes, negotiated in the connection handshake:

- **Classical**: Ed25519 only
- **Hybrid**: Ed25519 + ML-DSA-44 (dual signatures, both must verify)
- **PQ-only**: ML-DSA-44

Key encapsulation for forward secrecy uses ML-KEM-768. The hybrid mode provides a smooth migration path: sessions are authenticated against today's classical trust stores while carrying a PQ signature that will matter when classical keys are threatened.

### Compatibility with existing HF software

| Application | Protocol | Status |
|---|---|---|
| Pat (Winlink client) | ARDOP TCP command + data ports | Shipped |
| Winlink CMS | B2F / Winlink over TCP | Shipped |
| Any APRS or AX.25 application | KISS TNC over TCP | Shipped |
| direwolf / soundmodem | KISS framing (FEND/FESC/TFEND/TFESC) | Shipped |
| hamlib / flrig | rigctld CAT (PTT, frequency, S-meter) | Shipped |

The `openpulse-tnc` binary speaks the ARDOP TCP protocol natively — Pat connects to it without configuration changes. The `openpulse-kisstnc` binary does the same for any KISS client.

---

## What is already delivered

All items below are merged, tested, and in `main`:

- **Phases 1–9 complete**: modulation plugins (BPSK, QPSK, 8PSK, FSK4), rate adaptation, ACK taxonomy, signed handshake, SAR, DCD/CSMA, peer cache and query subsystem, multi-hop relay forwarding, post-quantum handshake, GPU acceleration (optional, wgpu), B2F/Winlink session layer, ARDOP TNC, KISS TNC, direct CMS gateway, TOML config management, structured JSON event stream, TUI frontend, egui testbench GUI
- **FF-1 QSY frequency agility**: collaborative channel-switching via rigctld
- **FF-2 I/Q SDR output**: complex baseband I/Q audio output for direct SDR upconversion
- **FF-3 RRC matched filtering**: root-raised-cosine TX/RX filters + Gardner timing recovery + Costas PLL for all `-RRC` modes (BPSK, QPSK, 8PSK)
- **FF-4 OFDM wideband HF profile**: multi-carrier OFDM plugin with LS channel estimation + ZF equalization; OFDM16 (≈625 Hz, ≈889 bps) and OFDM52 (≈2031 Hz, ≈2889 bps); `hpx_ofdm_hf()` session profile
- **FF-5 UHF/VHF wideband modes**: 2000 baud (8 kHz audio, ~2700 Hz BW) and 9600 baud (48 kHz audio, ~13 kHz BW) variants for QPSK and 8PSK; `hpx_narrowband` and `hpx_narrowband_hd` session profiles
- **FF-6 Binary spectrum channel**: `OPSP` binary frame interleaving on the daemon control port for 20 Hz waterfall updates; panel waterfall bypasses JSON overhead
- **FF-7 Tanh TX limiter**: soft-clip audio output to reduce PA back-off on 8PSK/RRC amplitude peaks
- **FF-8 Per-band TX attenuation**: per-band TX gain remembered and restored on band change via rigctld
- **FF-9 HPX reactor pattern**: event-driven `HpxReactor` replacing the polling-loop state machine
- **FF-10 zstd dictionary compression**: pre-trained shared dictionary for sub-500-byte payloads
- **FF-11 FreeDV authenticated voice shim**: Ed25519-signed beacon injected into FreeDV Qt-GUI via UDP data port; `TrustVerdict` Unix-socket API for companion UI polling
- **FF-1 ext — QSY trust gating**: `allow_trustlevels` wired from config into `QsyPolicy`; fail-closed parsing rejects unknown trust-level strings at startup
- **Phase 9 signal-path analytics**: IQ scatter plot, SNR trend, asymmetric rate adaptation, SNR-driven step-down, broadcast/beacon mode alongside ARQ
- **Winlink gateway**: direct TCP connection to `cms.winlink.org`, ISS and IRS roles, LZHUF (Type C) and Gzip (Type D) compression
- **PKI service**: Ed25519 trust-bundle signing, REST API, PostgreSQL backend

See [`docs/roadmap.md`](docs/roadmap.md) for per-item ✅ markers and PR references.

---

## What is coming

Active work tracks (see [`docs/roadmap.md`](docs/roadmap.md) FF-series):

All FF-series features have shipped. See the roadmap for the FEC improvements backlog (BL-FEC series).

---

## Test and validation harness

OpenPulseHF ships a multi-layer test harness that validates correctness without radio hardware:

### Channel simulation

`openpulse-channel` implements four published ionospheric channel models:

- **Watterson F1** (Good): 0.1 Hz Doppler spread, 0.5 ms delay spread — typical stable mid-latitude path
- **Watterson F2** (Moderate): 1.0 Hz Doppler spread, 1.0 ms delay spread
- **Watterson F3** (Poor): 10 Hz Doppler spread, 2.0 ms delay spread
- **Gilbert-Elliott**: burst-error model with configurable good/bad state transition rates and BER
- **AWGN**, **QRN** (impulsive noise), **QRM** (co-channel interference), **QSB** (slow fading)

The `ChannelSimHarness` wires two full `ModemEngine` instances through a channel model — the same encode/channel/decode stack used in production.

### Benchmark harness

```bash
cargo run -p openpulse-cli --no-default-features -- --backend loopback --log error benchmark run
```

Gate criterion: **100% frame pass rate, mean state transitions ≤ 20**. CI enforces this on every PR.

### Testbench GUI

`apps/openpulse-testbench` is a live four-column egui/eframe signal-path viewer:

- TX (clean) → Noise channel → Mixed (TX+noise) → RX (decoded)
- Per-tap: FFT spectrum line plot + plasma-colourmap waterfall
- Mode selector, noise model dropdown, SNR slider, FEC toggle
- Live BER and rolling event log
- Optional live audio capture (`--features cpal`) for real-signal testing

### End-to-end loopback

`crates/openpulse-b2f-driver/tests/e2e_loopback.rs` runs a full B2F message exchange through two modem engines and a channel model — no TCP server, no radio hardware, deterministic seed.

---

## Legal compliance

**OpenPulseHF uses digital signatures for authentication, not encryption for content privacy.**

This distinction is intentional and legally significant:

- **FCC Part 97.113(a)(4)** prohibits messages encoded for the purpose of obscuring their meaning. OpenPulseHF does not encrypt payload content. The Ed25519 and ML-DSA signatures authenticate the sender and detect tampering — they do not hide the message body.
- **CEPT/ERC/REC 25-10** and most EU national amateur radio regulations apply the same principle. Content encryption over amateur bands is prohibited; cryptographic authentication of the sender is not.
- **All frame payloads are human-readable** or decodable with standard tools. The ARDOP and KISS interfaces carry standard AX.25/Winlink message formats.
- **Callsign identification** is preserved through the B2F/Winlink message headers and the HPX session handshake. The `MYID` command on the ARDOP TNC interface sets the station callsign in all connection frames.

If you use OpenPulseHF on amateur radio frequencies, ensure your callsign is correctly configured in `~/.config/openpulse/config.toml` under `[station] callsign`. The software will refuse to connect to Winlink CMS with the default `N0CALL` placeholder.

---

## Quick start

```bash
# Build (requires libasound2-dev on Linux for CPAL; omit --no-default-features for audio hardware)
cargo build --workspace --no-default-features

# Run all tests (no audio hardware required)
cargo test --workspace --no-default-features

# Start a KISS TNC for your AX.25 application (default port 8100)
cargo run -p openpulse-kiss --no-default-features

# Start an ARDOP TNC for Pat (default cmd port 8515, data port 8516)
cargo run -p openpulse-ardop --no-default-features

# Send a Winlink message direct to CMS via TCP
cargo run -p openpulse-gateway -- send --to W1AW --subject "Hello" --message "Test"

# Initialise a config file
cargo run -p openpulse-cli --no-default-features -- config init > ~/.config/openpulse/config.toml
```

See [`docs/cli-guide.md`](docs/cli-guide.md) for full CLI reference and [`docs/on-air_testplan.md`](docs/on-air_testplan.md) for hardware setup and on-air test procedures.

---

## Documentation

| Document | Contents |
|---|---|
| [`docs/roadmap.md`](docs/roadmap.md) | Phase gates, completion status, FF-series feature list |
| [`docs/architecture.md`](docs/architecture.md) | Crate map, plugin system, session layer design |
| [`docs/requirements.md`](docs/requirements.md) | Functional and non-functional requirements |
| [`docs/cli-guide.md`](docs/cli-guide.md) | CLI subcommands, flags, config file reference |
| [`docs/on-air_testplan.md`](docs/on-air_testplan.md) | Hardware prerequisites, test matrix, regulatory checklist |
| [`docs/peer-query-relay-wire.md`](docs/peer-query-relay-wire.md) | Binary envelope and payload wire format |
| [`docs/hpx-session-state-machine.md`](docs/hpx-session-state-machine.md) | HPX session state machine spec |
| [`docs/benchmark-harness.md`](docs/benchmark-harness.md) | Benchmark harness and channel model spec |
| [`docs/testbench-design.md`](docs/testbench-design.md) | Testbench GUI design and channel model wiring |
| [`docs/regulatory.md`](docs/regulatory.md) | FCC Part 97, CEPT, and ITU compliance notes |
| [`docs/vara-research.md`](docs/vara-research.md) | VARA ACK taxonomy and rate adaptation analysis |
| [`docs/pactor-research.md`](docs/pactor-research.md) | PACTOR Memory-ARQ and FEC research |

---

## License

GNU General Public License v3.0 or later — see [LICENSE](LICENSE).
