---
project: openpulsehf
doc: CLAUDE.md
status: living
last_updated: 2026-05-27
---

# CLAUDE.md — OpenPulseHF Agent Contract

This file is the authoritative guide for any coding agent working in this repository. Read it before touching code. Mandatory agent safety rules are in `AGENTS.md` (root) and `docs/dev/AGENTS.md`.

---

## Build and test commands

```bash
# Toolchain preflight (required: rustc >= 1.94.0)
./scripts/check-toolchain.sh

# Full workspace build (requires libasound2-dev on Linux)
cargo build --workspace

# Full test suite (no audio hardware required)
cargo test --workspace --no-default-features

# Run a specific test file
cargo test --package openpulse-modem --no-default-features --test fec_loopback

# Clippy (treat warnings as errors)
cargo clippy --workspace --no-default-features -- -D warnings

# Format check
cargo fmt --all -- --check

# Cross-compile check for Raspberry Pi (requires `cross` installed)
cross check --workspace --target aarch64-unknown-linux-gnu --no-default-features

# Run the benchmark and capture JSON output
cargo run -p openpulse-cli --no-default-features -- --backend loopback --log error benchmark run

# CI benchmark regression gate (run locally to verify before PR)
cargo run -p openpulse-cli --no-default-features -- --backend loopback --log error benchmark run >/tmp/bench.json
jq '.passed == .total and .mean_transitions <= 20.0' /tmp/bench.json  # must print true

# Run the quick-tier test matrix (virtual channels, no hardware) — outputs to docs/test-reports/
cargo run -p openpulse-testmatrix --no-default-features

# Run the full test matrix (all propagation channels and payload sizes)
cargo run -p openpulse-testmatrix --no-default-features -- --full --output docs/test-reports

# Fallback core gates when full workspace checks are blocked by local toolchain constraints
cargo clippy --workspace --exclude pki-tooling --no-default-features -- -D warnings
cargo test --workspace --exclude pki-tooling --no-default-features
```

The `--no-default-features` flag disables the CPAL audio backend and is required for CI. All tests must pass with this flag. Never add tests that require real audio hardware.

---

## Crate map

### Core layer

| Crate | Path | Role |
|---|---|---|
| `openpulse-core` | `crates/openpulse-core` | Traits, frame format, CRC-16, `FecCodec` (RS+Conv), `HpxSession`/`HpxReactor` state machine, plugin registry, trust/signing, SAR, ACK, rate adaptation, relay, query propagation, peer cache, compression, PQ handshake, Hilbert I/Q |
| `openpulse-audio` | `crates/openpulse-audio` | `LoopbackBackend` (testing) and `CpalBackend` (hardware, feature-gated) |
| `openpulse-modem` | `crates/openpulse-modem` | `ModemEngine`, `PipelineScheduler`, benchmark harness, diagnostics, CSMA/DCD, channel sim harness |
| `openpulse-channel` | `crates/openpulse-channel` | Channel simulation (Watterson, Gilbert-Elliott, QRN/QRM/QSB/Chirp) |
| `openpulse-radio` | `crates/openpulse-radio` | `PttController` trait + `NoOpPtt`, `SerialRtsDtrPtt`, `VoxPtt`, `RigctldPtt`, `RigctldController` (CAT) |
| `openpulse-dsp` | `crates/openpulse-dsp` | DSP primitives: RRC filter, PLL, Gardner timing recovery, LMS/DFE adaptive equalizer |
| `openpulse-config` | `crates/openpulse-config` | Typed TOML schema; `load()`, `init_template()`, CLI-override pattern |
| `openpulse-gpu` | `crates/openpulse-gpu` | wgpu-backed BPSK DSP kernels; CPU fallback when GPU unavailable; gated by `gpu` feature in `bpsk-plugin` |
| `openpulse-keystore` | `crates/openpulse-keystore` | Secret storage (REQ-SEC-CTL-04): `FileKeystore` — named secrets encrypted at rest under an operator master password (Argon2id KDF → ChaCha20-Poly1305 AEAD) |
| `openpulse-linksec` | `crates/openpulse-linksec` | Control-channel link security (REQ-SEC-CTL-01/02): PSK-authenticated encrypted daemon↔client control link via Noise (`Noise_NNpsk0`, X25519); non-loopback auth gate |

### Protocol layer

| Crate | Path | Role |
|---|---|---|
| `openpulse-ardop` | `crates/openpulse-ardop` | ARDOP-compatible TCP TNC interface; `openpulse-tnc` binary; Pat-compatible command set |
| `openpulse-kiss` | `crates/openpulse-kiss` | KISS/AX.25 TNC interface; `openpulse-kisstnc` binary |
| `openpulse-b2f` | `crates/openpulse-b2f` | B2F/Winlink protocol state machine (banner, FC/FS/Ff/Fq frames, gzip+LZHUF compression) |
| `openpulse-b2f-driver` | `crates/openpulse-b2f-driver` | High-level ISS/IRS session driver over ARDOP TCP; e2e loopback tests |
| `openpulse-gateway` | `crates/openpulse-gateway` | Direct TCP Winlink CMS gateway; `openpulse-gateway` binary |
| `openpulse-qsy` | `crates/openpulse-qsy` | QSY frequency-agility protocol: wire frame codec, Ed25519 signing, `QsySession` state machine, `QsyScanner` |
| `openpulse-discovery` | `crates/openpulse-discovery` | JS8-based station discovery + rendezvous (FF-15) — RX + beacon TX + rendezvous SHIPPED (Phases A–G): pure no-I/O protocol logic. `hint.rs` (`@OPULSE` OPHF codec), `station.rs` (StationTable), `scheduler.rs` (`Js8Clock`), `discovery_sm.rs`, `runtime.rs` (`DiscoveryRuntime` — beacon scheduler + rendezvous orchestration), `peer_map.rs` (→ shared `PeerCache`), `hint_assembler.rs` (cross-slot `@OPULSE` beacon → peer recognition), `rendezvous.rs` (`RendezvousMsg` Propose/Accept/Reject codec + `RendezvousInitiator` + `respond()`; channels are per-band table indices; no signature — post-QSY CONREQ is the auth), `rendezvous_assembler.rs` (cross-slot RX reassembly of overs directed at us). Only Phase H (on-air) remains |
| `openpulse-mesh` | `crates/openpulse-mesh` | Mesh broadcast daemon; beacon re-broadcast with TTL, `openpulse-mesh` binary |
| `openpulse-repeater` | `crates/openpulse-repeater` | Digipeater / relay node; configurable filter and forwarding policy |
| `openpulse-daemon` | `crates/openpulse-daemon` | Unified background daemon aggregating modem, PTT, and control-protocol services |
| `openpulse-freedv-auth` | `crates/openpulse-freedv-auth` | External shim adding Ed25519 frame signing to FreeDV via the codec2 data channel (FF-11) |
| `openpulse-filexfer` | `crates/openpulse-filexfer` | Direct P2P file-transfer protocol (`OPFX`): pure no-I/O `FxFrame` codec + `SenderSession`/`ReceiverSession` state machines + offer/manifest/policy/sanitize + `blocks.rs` (split/pack/SAR mapping, `BlockAssembler`, fragment bitmaps, block-level resume). FF-16 Phases A–E SHIPPED (crate + modem loopback + daemon SendFile/twin round-trip + panel Files tab + real-radio PTT burst queue/drain + airtime-bounded burst splitting + `.partial` resume + `ListFiles`/CLI surface; PRs #730–#743, #787); on-air (Phase F) deferred |

### UI and tooling layer

| Crate | Path | Role |
|---|---|---|
| `openpulse-cli` | `crates/openpulse-cli` | CLI binary; thin wrapper over modem engine and protocol crates |
| `openpulse-tui` | `crates/openpulse-tui` | ratatui TUI frontend: HPX state, AFC/rate meters, DCD energy bar, transitions log |
| `openpulse-testbench` | `apps/openpulse-testbench` | egui/eframe signal-path testbench: 4-column waterfall/spectrum/scatter, 7 channel models |
| `openpulse-panel` | `apps/openpulse-panel` | Operator panel GUI (**iced**; connects to openpulse-daemon control port). Controls band + spectrum/waterfall/ladder + tabbed info/config/messages/log; Dark/Light/Contrast/System themes. `theme.rs` has an iced-free, unit-tested theme core. The egui version was retired 2026-07 (REQ-UX-04). |
| `openpulse-testmatrix` | `apps/openpulse-testmatrix` | Automated mode × channel test matrix runner |
| `openpulse-linksim` | `apps/openpulse-linksim` | Two-station bidirectional ARQ link simulator (lib + CLI): effective two-way transfer rate under simulated SNR/noise, with FSK4 ACKs, turnaround, retransmission, and over-the-air rate adaptation |
| `pki-tooling` | `pki-tooling` | Key management, trust store, bundle signing, PKI web service |

### Plugins

| Crate | Path | Role |
|---|---|---|
| `bpsk-plugin` | `plugins/bpsk` | BPSK31/63/100/250 modulation plugin; optional GPU path; LMS equalizer on RRC path |
| `qpsk-plugin` | `plugins/qpsk` | QPSK125/250/500/1000 modulation plugin |
| `psk8-plugin` | `plugins/psk8` | 8PSK500/1000 modulation plugin |
| `qam64-plugin` | `plugins/64qam` | 64QAM500/1000/2000-RRC modulation plugin; Gray-coded 8×8 PAM-8; soft demodulator |
| `fsk4-plugin` | `plugins/fsk4` | FSK4-ACK modulation plugin (ACK channel) |
| `mfsk16-plugin` | `plugins/mfsk16` | Constant-envelope non-coherent 16-GFSK weak-signal sub-floor waveform (REQ-WSIG-01): mode `MFSK16`, 31.25 baud, 500 Hz, 4 bits/sym, one 255-byte RS block; self-acquiring (Costas-16 sync + timing×freq search, `estimate_afc_hz = None`); soft-capable, frame-median-calibrated LLRs. Measured to beat coherent BPSK31 by ~4 dB on moderate fade / decode where BPSK31 fails on fast fade, at a PAPR credit. Broadcast-first; ACK/ladder deferred (PR-C/D) |
| `js8-plugin` | `plugins/js8` | JS8-compatible 8-GFSK weak-signal waveform (FF-15) — full TX+RX SHIPPED. `Js8Plugin` ModulationPlugin (submode/costas/GFSK/LDPC(174,87)/CRC-12/tones); native RX decoder (`decoder.rs` window multi-decode, `demodulate.rs` soft 8-FSK, `sync.rs` Costas, `ldpc174.rs` BP) — B-6 −18 dB go/no-go PASSES. Message layer: `frame.rs`/`grammar.rs` (callsign/grid/compound/directed unpack), `varicode.rs` (Huffman) + `jsc.rs` (full 262k JSC codebook) free-text decode. TX packers `encode.rs` (`pack_compound_frame`/`pack_alphanumeric50`/`pack_heartbeat_frame`/`pack_huff_frame`) + `beacon.rs` (`heartbeat`/`opulse_hint`/`directed` over assembly + `frame_audio`). Tables ported from GPL-3.0 JS8Call, validated vs real boost+Qt5 |
| `ofdm-plugin` | `plugins/ofdm` | OFDM16/52 + OFDM52-{8PSK,16QAM,32QAM,64QAM} multicarrier; Schmidl-Cox preamble, LS channel est + ZF equalization; soft demod |
| `scfdma-plugin` | `plugins/scfdma` | SC-FDMA16/52 + SCFDMA52/26-{8PSK,16QAM,32QAM,64QAM} single-carrier-FDM; DFT-CE pilot channel est + MMSE; per-symbol SFO deramp; soft demod |
| `pilot-plugin` | `plugins/pilot` | Pilot-framed `PILOT-{QPSK,8PSK,16QAM,32APSK}{500,1000}` (+ `-RRC`, + `2000-RRC`); pilot-aided carrier recovery (cycle-slip-immune, SRO-robust); soft demod; 32APSK = DVB-S2 |

---

## Current phase and execution order

**Completed**: Phases 1–9, Phase 7 (7.1–7.5), Phase 8 (8.1–8.3), FF series (FF-1 through FF-13), BL-FEC series (BL-FEC-1 through BL-FEC-6), all code stubs (PR #187–#189). See `docs/dev/project/roadmap.md` for full history.

**Active tracks**:
- **FF-15 JS8 discovery + rendezvous** — RX + **beacon TX (Phase E)** + **rendezvous → HPX handoff (Phase F) COMPLETE** (native TX+RX waveform, full message layer incl. JSC, discovery runtime, `@OPULSE` peer recognition, shared `PeerCache`, CLI + panel surfaces; TX packers/beacon assembly, `transmit_raw_audio` seam, slot scheduler + daemon wiring — off-by-default behind `[discovery] mode = "beacon"`/`"full"` + a callsign + ±2 s clock-skew/DCD/self-ID gates; §97.221 doc in `docs/regulatory.md`; PRs #744–#797). **Phase F** (PRs #798–#805): 2-message Propose/Accept/Reject rendezvous over JS8 directed free text → `RendezvousWith` daemon command → scheduled QSY (`switch_in_slots` delay) → `ConnectPeer` CONREQ handoff; channel-index table in config; two-runtime GFSK-audio end-to-end test. Remaining: **H on-air** only.
- **FF-16 file transfer** — Phases A–E SHIPPED (PRs #730–#743, + `ListFiles`/CLI #787); on-air (Phase F) deferred.

**Deferred (no target date)**:
- On-air regulatory validation (Phase 5.5-reg): on-air tests, station ID audit, compliance report

**Recently shipped (PRs #316–#321)**:
- `crates/openpulse-daemon/src/lib.rs`: QSY RF wiring — `QsySession` state machine wired into `AcceptQsy`; QSY_REQ + QSY_LIST frames transmitted via modem engine; `process_received_bytes` drives responder role from incoming RF (PR #321)
- `crates/openpulse-daemon/src/lib.rs`: CrossBandRepeater wiring — pre-built in `main.rs`; `EnableRepeater` spawns thread via `run_full_duplex`; `DisableRepeater` stops and joins it (PR #321)
- `apps/openpulse-panel/src/app.rs`: mode list updated to include RRC modes added in #319 and correct SCFDMA names (PR #321)
- `plugins/scfdma`: DFT-CE pilot-aided channel estimation; SCFDMA52-16QAM, SCFDMA52-32QAM (cross-32QAM), SCFDMA52-64QAM, SCFDMA52-64QAM-P4 modes; MMSE equalization (PR #316)
- `crates/openpulse-modem/src/arq_session.rs` (since refactored — the ARQ/HARQ logic now lives in `harq.rs` + `rate_policy.rs`): `ArqSession` — ARQ retry loop with soft LLR accumulation across retransmissions; runtime mode switching between registered plugins (PR #318)
- `crates/openpulse-core/src/profile.rs`: `hpx_narrowband_hd()` profile — SL8=QPSK9600-RRC, SL9=8PSK9600-RRC; `hpx_narrowband()` gains QPSK2000-RRC (SL10) and 8PSK2000-RRC (SL11) (PR #319)
- `plugins/qpsk`: `QPSK2000-RRC` and `QPSK9600-RRC` modes; `plugins/psk8`: `8PSK2000-RRC` and `8PSK9600-RRC` modes (PR #319)
- `crates/openpulse-daemon/src/main.rs`: PTT controller wired from config; `apply_command_to_engine` skips dispatch on PTT hardware assertion failure (PR #319)
- `apps/openpulse-testmatrix`: LDPC FEC entries added (PR #319)
- `crates/openpulse-core/src/profile.rs`: `hpx_wideband_hd()` updated to SL12–SL15 (SCFDMA52-16QAM → SCFDMA52-64QAM → 64QAM2000-RRC); ACK-UP gate at SL14 protecting SL15 admission (PR #320)
- `crates/openpulse-b2f/src/session.rs`: `queue_message_type_c()` — ISS Type C proposals using `compress_lzhuf_winlink` (LE prefix, Winlink-compatible) (PR #320)

**Previously shipped (PRs #193–#195)**:
- `crates/openpulse-b2f`: `compress_lzhuf_winlink` / `decompress_lzhuf_winlink` — 4-byte LE prefix matching Winlink Type C convention; IRS receive path switched to Winlink codec (PR #193)
- `crates/openpulse-dsp`: `LmsEqualizer` — complex symbol-rate LMS/DFE, supervised preamble training then decision-directed; wired into BPSK-RRC demodulation path after Gardner TED (PR #194)
- `plugins/64qam`: full 64QAM plugin — Gray-coded 8×8 PAM-8 constellation, rectangular-windowed and RRC modulator/demodulator, max-log-MAP soft demodulator; modes `64QAM500`, `64QAM1000`, `64QAM2000-RRC` (PR #195)
- `crates/openpulse-core/src/rate.rs`: `SpeedLevel` extended to SL20 (PR #195)
- `crates/openpulse-core/src/profile.rs`: initial `hpx_wideband_hd()` profile (SL12–SL14); profile slot arrays widened to 21 (PR #195)

**Previously shipped (PRs #187–#192)**:
- `plugins/psk8`: max-log-MAP `demodulate_soft()` replacing ±1.0 fallback
- `openpulse-cli`: `manifest verify` fully wired to `verify_manifest()`
- `openpulse-core::ldpc`: real rate-1/2 min-sum BP replacing passthrough stub
- `openpulse-modem`: `transmit_with_ldpc` / `receive_with_ldpc` and `transmit_with_fec_mode` / `receive_with_fec_mode` dispatch (multi-block since PR #691; a `Frame`'s `u8` payload length caps it at three 128-byte blocks)
- `openpulse-core::trust_store_file`: `load_trust_store_from_file()` — parses CLI JSON trust store format into `InMemoryTrustStore`
- ARDOP + KISS bridges: trust store loaded at startup; `RelayForwarder` wired into worker receive loop when `relay.enabled`

Execute Phase 1 tasks in this order. Tasks within the same group are independent and may be parallelised.

### Group 1 — ✅ Complete

**1.1a — Re-enable multi-platform CI** ✅ Done (PR #67)

**1.1b — Add block interleaver to `openpulse-core`** ✅ Done (PR #68)

**1.1c — Create `openpulse-channel` crate scaffold** ✅ Done (PR #69)

**1.1d — Refactor `openpulse-cli/src/main.rs`** ✅ Done
- `src/commands/` module with one file per subcommand
- `main.rs` is under 150 lines; all CLI behavior preserved

### Group 2 — ✅ Complete

**1.4 — Implement channel models** ✅ Done (PR #71)
Full spec in `docs/dev/design/testbench-design.md` and `docs/dev/benchmark-harness.md`.

**1.3 — Wire interleaver into `FecCodec` and `ModemEngine`** ✅ Done (PR #70)

**1.5 — Radio interface layer (`openpulse-radio` crate)** ✅ Done
- `PttController` trait, `PttError`, `NoOpPtt`, `SerialRtsDtrPtt`, `VoxPtt`, `RigctldPtt` all implemented and tested

### Group 3 — ✅ Complete

**1.2 — SAR (Segmentation and Reassembly)** ✅ Done (Option B)
- `crates/openpulse-core/src/sar.rs`: `sar_encode()` and `SarReassembler` with `ingest()` / `expire()`
- SAR header: `segment_id (u16) | fragment_index (u8) | fragment_total (u8)` — 4 bytes, leaving 251 bytes data per fragment
- Max transportable segment: 255 × 251 = 64 005 bytes; encoder rejects larger inputs with `SarError::DataTooLarge`
- Reassembly keyed on `(session_id, segment_id)`; configurable timeout; duplicate fragments are idempotent
- Integration tests: `tests/sar_roundtrip.rs` — 256-byte, 64 KB, max-size round-trips; missing-fragment; timeout; session isolation

### Remaining 1.5 integration items

**1.5a — AFC loop in BPSK demodulator** ✅ Done
- `estimate_frequency_offset()` in `plugins/bpsk/src/demodulate.rs` — IQ-squaring estimator
- `afc_estimate_hz()` — pure function, called via `ModulationPlugin::estimate_afc_hz()` default override in BPSK plugin
- `ModemEngine::last_afc_offset_hz()` — getter updated after every receive call
- `SessionDiagnostics::afc_offset_hz` — `Option<f32>` field, ready for CLI to populate
- Tracking range: ±baud_rate/4 (BPSK250: ±62.5 Hz; BPSK31: ±7.8 Hz)

**1.5b — PTT CLI wiring** ✅ Done
- `--ptt <none|rts|dtr|vox|rigctld>` and `--rig <address:port>` global options added to `openpulse-cli`
- `crates/openpulse-cli/src/radio.rs`: `build_ptt_controller()` factory selects backend at startup
- `commands/transmit.rs`: wraps transmit with PTT assert/release (release guaranteed on TX error)
- `serial` feature added to `openpulse-cli` propagates to `openpulse-radio/serial`
- Integration tests: `ptt_wiring_integration.rs` — none/default/unknown-backend cases

### Phase 2 — ✅ Partial (2.1, 2.2, 2.3, 2.4 complete)

**2.1 — ACK taxonomy and rate adaptation** ✅ Done
- `crates/openpulse-core/src/ack.rs`: `AckType` (8 variants, `#[repr(u8)]`), `AckFrame` (5-byte codec with CRC-8/SMBUS and FNV-1a session hash), `AckError`
- `crates/openpulse-core/src/rate.rs`: `SpeedLevel` (SL1–SL11), `RateEvent`, `RateAdapter::apply_ack()` state machine
  - ACK-DOWN floors at SL2; SL1 only reachable via 3 consecutive NACKs at SL2 (ChirpFallback)
  - 3 consecutive NACKs at SL3+ → NackDecrement; nack_threshold configurable (default 3)
- `plugins/fsk4/`: `Fsk4Plugin` implementing `ModulationPlugin` for mode `"FSK4-ACK"`
  - 4 tones at fc±50 Hz and fc±150 Hz (default fc=1050 Hz), 100 baud, Hann-windowed, Goertzel demodulator
  - 5 bytes = 20 symbols = 200 ms at 8 kHz
- Integration tests: `crates/openpulse-core/tests/rate_adaptation.rs` (11 tests); FSK4 loopback in `plugins/fsk4/src/lib.rs` (3 tests)

**2.2 — HPX500 and HPX2300 adaptive profiles** ✅ Done
- `plugins/qpsk/src/lib.rs`: added `QPSK1000` mode (1000 baud, 8 samples/symbol @ 8 kHz)
- `plugins/psk8/`: `Psk8Plugin` implementing `ModulationPlugin` for `"8PSK500"` and `"8PSK1000"`
  - Gray-coded 8PSK constellation (8 phases, 3 bits/symbol); Hann-windowed modulator; nearest-point IQ demodulator
  - HPX2300 waveform decision: single-carrier chosen over OFDM (lower PAPR, no cyclic prefix, simpler AFC — see `docs/dev/design/architecture.md`)
- `crates/openpulse-core/src/profile.rs`: `SessionProfile` struct mapping `SpeedLevel` → mode string
  - `SessionProfile::hpx500()`: SL2=BPSK31, SL3=BPSK63, SL4=BPSK250, SL5=QPSK250, SL6=QPSK500; initial=SL2
  - `SessionProfile::hpx2300()`: SL8=QPSK500, SL9=QPSK1000, SL11=8PSK1000; initial=SL8
- `crates/openpulse-modem/src/engine.rs`: `start_adaptive_session()`, `apply_ack()`, `current_adaptive_mode()` wired to `RateAdapter`
- Integration tests: `crates/openpulse-core/tests/session_profile.rs` (4 tests); `crates/openpulse-modem/tests/adaptive_profile_integration.rs` (8 tests); psk8 loopback (5 tests)

**2.3 — Signed handshake and manifest verification** ✅ Done
- `crates/openpulse-core/Cargo.toml`: added `ed25519-dalek 2.0` (signing/verification); `rand 0.8` in dev-deps
- `crates/openpulse-core/src/handshake.rs`: `ConReq`, `ConAck` wire frames with Ed25519 sign/verify; `TrustStore` trait; `InMemoryTrustStore`; `verify_conreq()`, `verify_conack()`
  - CONREQ: MAGIC "HSCQ" + VERSION + LENGTH + JSON; signature covers canonical JSON of body fields
  - CONACK: MAGIC "HSAK" + VERSION + LENGTH + JSON; echoes CONREQ session_id; signature covers canonical JSON
  - Trust evaluation wired to existing `evaluate_handshake()` / `classify_connection_trust()` in `trust.rs`
  - Revoked key → `TrustError::RejectedTrustLevel`; no mutual mode → `TrustError::NoMutualSigningMode`
- `crates/openpulse-core/src/manifest.rs`: `TransferManifest` with SHA-256 payload hash, sender ID, Ed25519 signature; `verify_manifest()` and `TransferManifest::sign()`
- Integration tests: `crates/openpulse-core/tests/handshake_integration.rs` (12 tests); `crates/openpulse-core/tests/manifest_integration.rs` (6 tests)
  - Tests cover: happy path, tampered signature, revoked key, session ID mismatch, no mutual mode, full round-trip

**2.4 — DCD and CSMA channel access** ✅ Done
- `crates/openpulse-core/src/dcd.rs`: `DcdState` struct — RMS energy threshold, configurable hold window (default 100 ms at 8 kHz), `update(samples)`, `is_busy()`, `energy()`, `force_busy()`
- `crates/openpulse-core/src/error.rs`: added `ModemError::ChannelBusy` variant
- `crates/openpulse-modem/src/engine.rs`: DCD update wired into `receive()` after sample capture; 0.3-persistence `csma_check()` called at the head of each transmit path (pre-encode, to avoid burning a sequence number on deferral — including `broadcast()` as of the G-1 fix); `enable_csma()`, `disable_csma()`, `is_channel_busy()`, `dcd_energy()` public API; `rand 0.8` added to `[dependencies]`
- Integration tests: `crates/openpulse-modem/tests/csma_loopback.rs` (4 tests)
  - DCD detects energy from received signal; CSMA blocks on busy channel; disabled CSMA ignores DCD; two-station deferral scenario

**2.5 — Peer cache and query subsystem** ✅ Done
- `crates/openpulse-core/src/peer_descriptor.rs`: `PeerDescriptor` self-authenticating signed identity descriptor
  - `peer_id` IS the Ed25519 verifying-key bytes; `verify_peer_descriptor()` needs no external key store
  - Signed fields: `peer_id`, `callsign`, `capability_mask`, `timestamp_ms` via canonical JSON (same pattern as CONREQ/CONACK)
  - `callsign_hash()` returns SHA-256 of callsign bytes for use in query response entries
- `crates/openpulse-core/src/peer_cache.rs`: extended `PeerRecord` with `capability_mask: u32`
  - Added `TrustFilter` enum (TrustedOnly, TrustedOrUnknown, Any) — wire codes per peer-query-relay-wire.md
  - Added `PeerCache::query(capability_mask, min_quality, trust_filter, max_results, now_ms)` — sorted by quality descending
- `crates/openpulse-core/src/wire_query.rs`: OPHF binary envelope + peer query payloads
  - `WireEnvelope`: encode/decode per docs/dev/peer-query-relay-wire.md; header 104 B + payload + auth_tag 16 B
  - `PeerQueryRequest` (msg_type 0x01): 17-byte fixed payload
  - `PeerQueryResponse` (msg_type 0x02): variable-length results with descriptor_signature
- Integration tests: `tests/peer_descriptor_integration.rs` (9 tests); `tests/wire_query_integration.rs` (9 tests)

**2.6 — Multi-hop relay forwarding** ✅ Done
- `crates/openpulse-core/src/relay.rs`: extended with `RelayForwarder`, `RelayEvent`, `RelayForwardError`, `score_route`, `select_best_scored_route`
  - `RelayForwarder`: stateful relay node — hop-limit enforcement, (session_id, nonce) duplicate suppression with TTL eviction, src_peer_id trust-policy check
  - On success: clones envelope with `hop_index += 1`; emits `RelayEvent::Forwarded`
  - On failure: returns typed `RelayForwardError`; emits corresponding `RelayEvent` for observability
  - `drain_events()` returns buffered events (Forwarded, HopLimitExceeded, DuplicateSuppressed, PolicyRejected)
- Trust-weighted path scoring: `score_route` uses bottleneck (min) hop score = `trust_weight × route_quality`
  - `trust_weight`: Verified=4, PskVerified=3, Unknown=2, Reduced=1
  - Direct routes (no intermediate hops) score `u32::MAX` — never penalized
  - `select_best_scored_route`: highest-score wins; ties broken by shorter path
- `crates/openpulse-core/src/peer_cache.rs`: added `peek(&self)` — read-only lookup for route scoring (no LRU update)
- `crates/openpulse-core/src/wire_query.rs`: extended `WireMsgType` (added 0x03–0x08); added `RelayDataChunk` (msg_type 0x05, 82-byte header + variable sig/data), `AckStatus` enum, `RelayHopAck` (msg_type 0x06, 49 bytes fixed)
- Integration tests: `tests/relay_integration.rs` (12 tests)
  - Path scoring, bottleneck scoring, best-route selection, policy denial
  - 3-hop hop_index increment across separate `RelayForwarder` nodes
  - Hop-limit drop, duplicate suppression, trust-policy rejection at hop
  - Event drain verification, `RelayDataChunk` round-trip, `RelayHopAck` round-trip

**3.2 — Network query propagation** ✅ Done
- `crates/openpulse-core/src/wire_query.rs`: added remaining wire payload codecs
  - `WireTrustState` enum (0x00–0x03); `RouteChangeReason` enum (u16, 5 variants); `RouteHop` (37 bytes per hop)
  - `RouteDiscoveryRequest` (msg_type 0x03, 47 bytes fixed): route_query_id, destination_peer_id, max_hops, required_capability_mask, policy_flags
  - `RouteDiscoveryResponse` (msg_type 0x04, variable): route_query_id, route_id, hops(Vec<RouteHop>), route_signature
  - `RelayRouteUpdate` (msg_type 0x07, variable): route_id, previous_hop_count, route_change_reason, replacement_hops, route_update_signature
  - `RelayRouteReject` (msg_type 0x08, 45 bytes fixed): route_id, reject_hop_peer_id, reason_code, trust_decision, policy_reference
  - Added `WireQueryError::HopCountExceeded` and `WireQueryError::SignatureTooLarge`
- `crates/openpulse-core/src/query_propagation.rs`: added `QueryForwarder` and events
  - `QueryForwarder`: stateful propagation node — msg-type check, hop-limit enforcement, trust-policy check, (src_peer_id, query_id) duplicate suppression via `QueryPropagationTracker`
  - On success: clones envelope with `hop_index += 1`; emits `QueryEvent::Propagated`
  - `QueryForwardError` (4 variants) and `QueryEvent` (4 variants) for observability
- Integration tests: `tests/query_propagation_integration.rs` (12 tests)
  - Wire codec round-trips: `RouteDiscoveryRequest`, `RouteDiscoveryResponse` (no-hop and multi-hop), `RelayRouteUpdate`, `RelayRouteReject`
  - `QueryForwarder`: basic propagation, hop-limit, duplicate suppression, trust-policy rejection, event drain
  - 3-node query chain verifying hop_index increment across separate `QueryForwarder` nodes

**2.7 — Compression (session layer)** ✅ Done
- `crates/openpulse-core/Cargo.toml`: added `lz4_flex 0.11` (pure-Rust LZ4 implementation, no C bindings)
- `crates/openpulse-core/src/compression.rs`: new module — `CompressionAlgorithm` enum (`None`, `Lz4`), `compress()`, `decompress()`, `compress_if_smaller()` (compress-then-compare; returns original bytes with `None` if LZ4 is not smaller)
- `crates/openpulse-core/src/handshake.rs`: added `supported_compression: Vec<CompressionAlgorithm>` to `ConReq` and `selected_compression: CompressionAlgorithm` to `ConAck`; both fields are included in the canonical JSON covered by the Ed25519 signature, so post-signing injection is detectable
- Integration tests: `tests/compression_integration.rs` (9 tests)
  - Codec round-trips (`None`, `Lz4`), decompression of garbage returns error
  - `compress_if_smaller` picks Lz4 for compressible data, keeps original for incompressible
  - `ConReq`/`ConAck` carry and verify compression fields; full negotiation round-trip; tampered compression field invalidates signature

**3.1 — Post-quantum in-band handshake** ✅ Done
- `crates/openpulse-core/Cargo.toml`: added `ml-dsa 0.1.0-rc.9` (ML-DSA-44, `rand_core` feature), `ml-kem 0.3` (`getrandom` feature), `rand 0.8` promoted from dev-deps to deps
- `crates/openpulse-core/src/trust.rs`: added `SigningMode::Pq` (ML-DSA-44 only, strength 4) and `SigningMode::Hybrid` (Ed25519 + ML-DSA-44, strength 5); updated `mode_strength()` and `allowed_signing_modes()` for all three policy profiles
- `crates/openpulse-core/src/pq_handshake.rs`: new module — ML-DSA-44/ML-KEM-768 post-quantum handshake
  - Size constants: `ML_DSA_44_PUBKEY_SIZE=1312`, `ML_DSA_44_SIG_SIZE=2420`, `ML_KEM_768_EK_SIZE=1184`, `ML_KEM_768_DK_SIZE=64` (d||z seed), `ML_KEM_768_CT_SIZE=1088`, `ML_KEM_768_SS_SIZE=32`
  - `PqConReq` / `PqConAck` wire frames with classical + PQ pubkeys, KEM pubkey/ciphertext, dual signatures
  - `generate_ml_dsa_44_keypair()`, `generate_ml_kem_768_keypair()` — OsRng-based key generation
  - `create_pq_conreq()`, `create_pq_conack()` — build and sign frames; `Hybrid` signs with both Ed25519 and ML-DSA-44; `Pq`-only mode leaves `classical_signature` empty
  - `kem_decapsulate(dk, ct)` — recovers 32-byte shared secret from `PqConAck.kem_ciphertext`
  - `verify_pq_conreq()`, `verify_pq_conack()` — verify both signatures then call `evaluate_handshake()`
  - `encode_pq_conreq/ack`, `decode_pq_conreq/ack` — JSON serde helpers for SAR transport
- `crates/openpulse-cli/src/output.rs`: added `Pq` and `Hybrid` arms to `signing_mode_to_str()`
- Integration tests: `tests/pq_handshake_integration.rs` (12 tests)
  - Key sizes, KEM shared-secret match, Hybrid and Pq-only round-trips, mode negotiation, tamper rejection (PQ sig, classical sig, session ID mismatch), SAR size gate, SAR encode→fragment→reassemble→decode round-trip

**PKI service-side trust-bundle signing** ✅ Done (PR #88)
- `pki-tooling/Cargo.toml`: added `rand = "0.8"` for explicit dev-only ephemeral key generation
- `pki-tooling/src/lib.rs`: `AppState` gains `signing_key: ed25519_dalek::SigningKey`; new route `GET /api/v1/signing-key`
- `pki-tooling/src/main.rs`: requires `PKI_SIGNING_KEY` (base64 32-byte seed) by default; explicit dev fallback requires `PKI_ALLOW_EPHEMERAL_KEY=true`; bind address is configurable with `PKI_BIND_ADDR`
- `pki-tooling/src/verification.rs`: added `bundle_canonical_body()` (with recursive key-sort for JSONB stability) and `verify_bundle_signature()`; 8 unit tests
- `pki-tooling/src/api/handlers.rs`: service computes Ed25519 signature at publish time; `bundle_signature` removed from request; `service_pubkey` persisted per-row and returned in `TrustBundleResponse`; `get_signing_key` handler
- `pki-tooling/migrations/0010_trust_bundle_service_pubkey.sql`: adds `service_pubkey TEXT NOT NULL DEFAULT ''` to `trust_bundles`

**Phase 3.5-substitute — sound-loopback channel simulation** ✅ Done (PR #89)
- `crates/openpulse-audio/src/loopback.rs`: added `drain_samples()` and `fill_samples()` test-utility methods to `LoopbackBackend`
- `crates/openpulse-modem/src/channel_sim.rs`: new `ChannelSimHarness` wiring two `ModemEngine` instances through `openpulse_channel::ChannelModel`; `route()` and `route_clean()` methods
- `crates/openpulse-modem/tests/channel_loopback.rs`: 6 integration tests (clean passthrough, AWGN 20 dB, Watterson F1, Watterson F2 negative, G-E light+FEC positive, G-E burst negative)
- `crates/openpulse-channel/src/watterson.rs`: fix `fading_coeff` bug — was passing loop-local index instead of absolute sample index, causing O(n) envelope FFT refills per `apply()` call; fixed to O(n/1024)

**Phase 3.2 — Convolutional FEC evaluation** ✅ Done
- `crates/openpulse-core/src/conv.rs`: `ConvCodec` — rate-1/2, K=3 (4-state), generators G={7,5} octal, hard-decision Viterbi decoder; same `encode/decode` interface as `FecCodec`
- Benchmark: at channel BER 1%, RS post-decode BER = 0.497 vs ConvCodec = 0.0004 (AWGN regime; RS fails because random errors exceed 16-byte/block capacity); CPU overhead 3.8×
- Decision: **ACCEPTED** — ConvCodec is an optional alternative FEC for AWGN-dominant paths; RS+interleaver remains default for HF burst-error profiles
- 6 integration tests in `crates/openpulse-core/tests/fec_comparison.rs`; decision documented in `docs/dev/research/vara-research.md`

**Phase 3.3 — GPU compute acceleration for BPSK DSP** ✅ Done (PR #90)
- `crates/openpulse-gpu/`: new crate — `GpuContext` (wgpu device + pre-compiled pipelines), WGSL kernels for BPSK modulation, IQ demodulation, and timing offset search
- `plugins/bpsk/`: `gpu` feature flag; `BpskPlugin::with_gpu(Arc<GpuContext>)`; GPU dispatch in modulate and demodulate paths; CPU fallback when GPU readback returns `None`
- All GPU functions return `Option<T>` so callers can detect failures rather than silently getting empty/zero data
- GPU-vs-CPU equivalence tests under `#[cfg(feature = "gpu")]`

**Phase 3.4 — ARDOP-compatible TCP interface** ✅ Done
- `crates/openpulse-ardop/`: new crate with library + `openpulse-tnc` binary
  - `state.rs`: `TncState` enum (Disc, Listen, Connecting, Connected, Disconnecting) with ARDOP state labels
  - `bridge.rs`: `ModemBridge` — shared state, `broadcast` event/data channels, sync TX queue, background worker thread
  - `command.rs`: ASCII line protocol — VERSION, MYID, LISTEN, CONNECT, DISCONNECT, ABORT, STATE, BUFFER, PTT, CLOSE
  - `data.rs`: `u16 BE` length-prefixed binary framing in both directions
  - `main.rs`: binary reads `ARDOP_CMD_PORT`, `ARDOP_DATA_PORT`, `ARDOP_MODE`, `ARDOP_BIND` env vars
  - Loopback mode (`ArdopConfig::loopback`) echoes TX data as RX data for protocol-level integration tests
- `crates/openpulse-ardop/tests/ardop_integration.rs`: 8 tests — VERSION, MYID, STATE, CONNECT/DISCONNECT, ABORT, BUFFER, data port single-frame and multi-frame loopback

**Phase 4.2 — Structured JSON event stream** ✅ Done
- `crates/openpulse-core/src/hpx.rs`: added `#[derive(Serialize, Deserialize)]` to `HpxState`, `HpxEvent`
- `crates/openpulse-core/src/rate.rs`: added `#[derive(Serialize, Deserialize)]` to `RateEvent`
- `crates/openpulse-modem/src/event.rs`: new `EngineEvent` enum (8 variants, NDJSON-ready); `SessionStarted.session_id` is `Option<String>`; `SessionStarted.peer_modes` (not `peer`)
- `crates/openpulse-modem/src/engine.rs`: added `broadcast::Sender<EngineEvent>` field; `subscribe()` method; events emitted at transmit/receive/apply_ack/hpx_apply_event/begin_secure_session/end_secure_session with DCD change detection; `RateChange` only emitted when adaptive session is active
- `crates/openpulse-cli/src/commands/monitor.rs`: `openpulse monitor --mode <MODE>` subcommand streaming NDJSON to stdout; fatal errors propagated; stdout flushed per event
- Integration tests: `crates/openpulse-modem/tests/engine_events.rs` (7 tests including DcdChange and AfcUpdate)

**Phase 4.1 — TUI frontend** ✅ Done
- `crates/openpulse-tui/`: new binary crate using ratatui 0.27 + crossterm
  - `src/app.rs`: `App` state struct updated by `EngineEvent`s; transitions ring buffer (last 50); pause/scroll support
  - `src/ui.rs`: three-panel layout — HPX state (colour-coded), AFC/rate meters + DCD energy bar, scrollable transitions log
  - `src/events.rs`: `spawn_worker()` runs engine receive loop in a background thread; `drain_worker()` applies events to `App`
  - `src/main.rs`: 100 ms tick loop; keyboard: `q`/Ctrl+C quit, `p` pause, ↑↓ scroll

**Phase 4.3 — KISS and AX.25 interface** ✅ Done
- `crates/openpulse-kiss/`: new binary crate with `openpulse-kisstnc` binary
  - `src/kiss.rs`: KISS frame encode/decode with full byte stuffing (FEND/FESC/TFEND/TFESC); `KISS_DATA=0x00` type constant
  - `src/ax25.rs`: AX.25 UI frame encode/decode — `Ax25Addr` (callsign + SSID), `Ax25UiFrame`; Control=0x03, PID=0xF0; callsign wire encoding via 1-bit left-shift
  - `src/bridge.rs`: `KissBridge` with `broadcast::Sender<Vec<u8>>` RX channel and `std::sync::mpsc::SyncSender<Vec<u8>>` TX queue; OS-thread worker loop mirrors `openpulse-ardop`
  - `src/server.rs`: single TCP listener; per-client task reads FEND-delimited KISS frames, KISS-encodes RX payloads back to clients
  - `src/main.rs`: reads `KISS_PORT` (default 8100), `KISS_BIND`, `KISS_MODE` env vars
- Integration tests: `tests/kiss_integration.rs` (8 tests)
  - KISS codec round-trip, FEND/FESC byte stuffing, AX.25 callsign parse and UI frame round-trip
  - TCP single-frame loopback, multi-frame loopback, byte-stuffed payload loopback

**Phase 4.4 — B2F protocol and Winlink gateway integration** ✅ Done
- `crates/openpulse-b2f/`: new pure-protocol library crate (no tokio, no modem engine dependency)
  - `src/banner.rs`: WL2K connection banner encode/decode — `[WL2K-3.0-B2FWINMOR-4.0-XXXXXXXX]`; FNV-1a session key
  - `src/frame.rs`: B2F control frame codec — `Fc`, `Fs`, `Ff`, `Fq`; `ProposalType` (C/D); `FsAnswer` (Accept/Reject/Defer); CR-terminated ASCII
  - `src/header.rs`: WL2K message header encode/decode; RFC-5322-like CRLF-terminated; `WlHeader`, `AttachmentInfo`
  - `src/compress.rs`: Gzip (type D) via `flate2`; LZHUF (type C) pass-through stubs
  - `src/session.rs`: `B2fSession` state machine; `SessionRole::Iss`/`Irs`; Handshake→ProposalExchange→Transfer→Done; handles ISS-immediate-proposal pattern
- `crates/openpulse-ardop/src/bridge.rs` + `command.rs`: Pat-compatible ARDOP commands — GRIDSQUARE, ARQBW, ARQTIMEOUT, CWID, SENDID, PING; `gridsquare/arq_bw/arq_timeout` fields with `Arc<RwLock<>>` sharing
- Integration tests: `crates/openpulse-b2f/tests/b2f_integration.rs` (9 tests); `ardop_integration.rs` extended to 11 tests

**Phase 4.5 — Signal-path testbench GUI** ✅ Done
- `apps/openpulse-testbench/`: new egui/eframe 0.29 binary crate
  - 4-column live view: TX (clean), Noise channel, Mixed (TX+noise), RX (decoded)
  - Per-tap: spectrum line plot (FFT dBFS) + plasma-colourmap waterfall texture
  - Toolbar: mode (BPSK31–QPSK500), noise model (7 models), SNR slider, FEC toggle, seed, dB range sliders
  - Stats bar: runs / OK / fail / BER / last event from rolling log
  - Signal thread uses `bpsk-plugin`/`qpsk-plugin` directly; `Arc<RwLock<TapData>>` shared with UI; `crossbeam_channel` stop signal
  - All 7 channel models wired through `build_channel()` factory from `openpulse-channel`

**Phase 5.1 — B2F session driver** ✅ Done (PR #98)
- `crates/openpulse-b2f-driver/`: new pure-std crate (no tokio); `B2fDriver`, `DecodedMessage`, `DriverError`
  - `src/cmd.rs`: `CmdPort` — BufReader<TcpStream> + write half; `TimedOut`/`WouldBlock` mapped to `DriverError::Timeout`
  - `src/data.rs`: `DataPort` — u16 BE length-prefixed frames; send validated against u16::MAX
  - `run_iss()`: MYID→CONNECT→recv banner→send FC+FF→recv FS→send blobs→DISCONNECT
  - `run_irs()`: MYID→LISTEN→wait CONNECTED (with timeout)→send banner→recv FC/FF→send FS→recv N blobs→DISCONNECT
- Integration tests: `tests/driver_integration.rs` (4 tests): `iss_sends_one_message`, `irs_receives_one_message`, `iss_irs_roundtrip`, `multi_message_roundtrip`

**Phase 5.2 — LZHUF codec** ✅ Done (PR #98)
- `crates/openpulse-b2f/src/compress.rs`: real LZHUF LH5 via `oxiarc-lzhuf = "0.2.7"`
  - 4-byte BE original-length prefix makes stream self-contained (known incompatibility with external Winlink Type C — deferred)
  - `compress_lzhuf`: validates payload fits `u32` before cast
  - `decompress_lzhuf`: caps `orig_len` at 16 MiB to prevent OOM from malformed frames
- `B2fSession::accepted_count()` added to `session.rs` — IRS driver uses this to know how many data frames to read
- Integration tests: `lzhuf_round_trip`, `lzhuf_bad_input_error`

**Phase 5.3 — TOML configuration management** ✅ Done (PR #102)
- `crates/openpulse-config/`: new crate with typed TOML schema covering station, modem, ARDOP, KISS, logging, relay, and trust-store settings
  - `load()` reads `~/.config/openpulse/config.toml`; propagates errors so misconfiguration is visible at startup
  - `init_template()` returns a fully-commented TOML template
  - Precedence: CLI flag > config file > built-in defaults
- `openpulse-tnc` and `openpulse-kisstnc` accept clap CLI flags (`--cmd-port`, `--data-port`, `--mode`, `--bind` / `--port`) that override config file values, replacing the previous env-var-only approach
- `openpulse config init` writes the commented template to stdout; short-circuits before any hardware/network setup
- Three tests: `load_defaults_when_no_file`, `cli_override_pattern`, `missing_fields_get_defaults`

**Phase 5.4 — End-to-end loopback integration test** ✅ Done (PR #100)
- `crates/openpulse-b2f-driver/tests/e2e_loopback.rs`: full-stack gate test (no hardware required)
  - Bidirectional modem relay chains two `B2fDriver` instances through `ChannelSimHarness` (BPSK250 encode → channel → BPSK250 decode)
  - `e2e_single_message_awgn_20db`: one message through AWGN 20 dB (seed 42, deterministic)
  - `e2e_multi_message_clean`: three messages through clean channel, all bodies verified in order
  - Shared test helpers extracted to `tests/common/mod.rs` (reused by `driver_integration.rs`)
- Phase 3.5 on-air validation is now unblocked

**Phase 5.5 — Direct TCP Winlink CMS gateway** ✅ Done
- `crates/openpulse-gateway/`: new binary crate (`openpulse-gateway`)
  - Phase 1 (ISS): connects to `cms.winlink.org:8772`, reads CMS banner, sends FC+FF proposals, reads FS, sends compressed blobs
  - Phase 2 (IRS): same TCP connection, fresh `B2fSession(Irs)`, reads CMS FC+FF proposals, sends FS, reads and decompresses reply blobs
  - `DataPort` wraps `TcpStream` directly — Winlink CMS TCP uses identical u16-BE framing as B2F driver
  - CLI: `openpulse-gateway [--host] [--port] [--callsign] send --to <CALL> [--subject] [--message | stdin]`
  - Callsign read from `~/.config/openpulse/config.toml`; `--callsign` overrides; bails on default `N0CALL`
  - `gateway_round_trip` unit test: mock CMS TCP server validates full ISS+IRS exchange without network access

**Phase 5.6 — CpalBackend wiring + TOML audio config + on-air test plan** ✅ Done (PR #105)
- `crates/openpulse-config/src/lib.rs`: `AudioConfig { backend: String }` (default `"default"`); no device-name fields
- `crates/openpulse-ardop/src/main.rs` + `crates/openpulse-kiss/src/main.rs`: `--backend` CLI flag; `#[cfg(feature = "cpal")]`/`#[cfg(not(feature = "cpal"))]` match arms; `"default"` silently falls back to loopback; `"cpal"` warns when feature absent
- Build with real audio: `cargo build --release -p openpulse-kiss --features cpal` / `--features cpal` for ardop
- `docs/on-air_testplan.md`: hardware prereqs, station config template, audio path verification (Python KISS frame sender), test matrix (BPSK250 exchange, rate adaptation, Winlink CMS via RF, multi-mode ladder, ID compliance), regulatory checklist, diagnostics table

**Phase 5.7 — Testbench live audio capture** ✅ Done (PR #108)
- `apps/openpulse-testbench/Cargo.toml`: `cpal` feature gates `openpulse-audio/cpal-backend`
- `AudioSource` enum (`Synthetic` / `LiveCapture`); `AppConfig::audio_source` field
- `run_live()`: opens default system input at 8 kHz mono, captures audio into tap[2], demodulates into tap[3], synthesized TX reference in tap[0]; failure propagated to stats event log
- Source combo (cpal only) disabled while simulation is running; panel labels update to match live mode
- `JoinHandle::is_finished()` check in `update()` auto-clears `running` when thread exits early
- Build: `cargo build --release -p openpulse-testbench --features cpal`

These must be confirmed by the user before the relevant implementation starts. Do not implement speculatively.

### SAR wire format — Resolved (Option B implemented)

Option B (full SAR sub-layer) was selected and implemented in `crates/openpulse-core/src/sar.rs`.  See Group 3 task entry for details.  Phase 3.1 (PQ handshake) is unblocked once any other remaining Phase 1 gates are closed.

### `PttController` trait location (blocks Phase 1.5)

Resolved and implemented: `crates/openpulse-radio` with `NoOpPtt`, `SerialRtsDtrPtt`, `VoxPtt`, `RigctldPtt`.

---

## Acceptance criteria

Each requirement below is done when the linked test passes. Add new links as tests are written.

| Requirement | Acceptance test |
|---|---|
| BPSK loopback correctness | `cargo test -p openpulse-modem --test bpsk_hardening` |
| QPSK loopback correctness | `cargo test -p openpulse-modem --test qpsk_hardening` |
| FEC RS encode/decode | `cargo test -p openpulse-modem --test fec_loopback` |
| HPX state machine transitions | `cargo test -p openpulse-modem --test hpx_conformance_integration` |
| Benchmark 100% pass, mean_transitions ≤ 20 | `cargo test -p openpulse-modem --test benchmark_integration` |
| Session persistence | `cargo test -p openpulse-cli --test local_state_integration` |
| Block interleaver round-trip | `cargo test -p openpulse-core` (add test in `fec.rs`) |
| Gilbert-Elliott bursts span whole symbols (mean 1/p_bg symbols) | `cargo test -p openpulse-channel --lib bursts_span_whole_symbols_with_mean_one_over_pbg` |
| Watterson fading envelope non-trivial | `cargo test -p openpulse-channel` (`f1_envelope_has_non_trivial_variation` in `watterson.rs`) |
| Watterson continuous fade correlates across `apply()` calls | `cargo test -p openpulse-channel --lib continuous_fade_correlates_across_calls` |
| SC-FDMA channel estimator vs. selective channels | `cargo test -p openpulse-modem --test scfdma_ce_sweep -- --ignored` (before/after harness) |
| SC-FDMA decodes a stronger delayed ray to a 2 ms (16-sample) spread inside the CP | `cargo test -p openpulse-modem --test scfdma_multipath_timing` |
| Symbol-domain SNR tracks true SNR past M2M4's ~15 dB saturation | `cargo test -p openpulse-modem --test symbol_domain_snr` + `--test symbol_snr_ladder_climb` |
| 64QAM soft LLRs are calibrated (worst-bin error ≤ 4× the promised rate) | `cargo test -p qam64-plugin --test llr_reliability` |
| OFDM soft LLRs are calibrated (worst-bin error ≤ 4× the promised rate) | `cargo test -p ofdm-plugin --test llr_reliability` |
| Pilot-plugin soft LLRs are calibrated (worst-bin error ≤ 4× the promised rate) | `cargo test -p pilot-plugin --test llr_reliability` |
| QPSK1000-HF-RRC forward-only LMS holds the good_f1 coded floor | `cargo test -p openpulse-modem --test qpsk_hf_rrc_forward_only` |
| CI goodput regression gate (linksim effective_bps ≥ 65 % of baseline) | `cargo test -p openpulse-linksim goodput_gate` |
| JS8 NORMAL native decode reaches the −18 dB weak-signal gate (FF-15 Phase-B go/no-go) | `cargo test -p js8-plugin --test snr_sweep gate_at_minus_18_db` |
| JS8 discovery MVP: the daemon rx-tick activates, dwells, decodes an injected heartbeat, caches the station + emits `StationHeard` | `cargo test -p openpulse-daemon --no-default-features discovery_tick` |
| File-transfer protocol edges (offer/accept/reject/timeout/cancel/verify/tamper) | `cargo test -p openpulse-filexfer` |
| File-transfer blocks survive the modem (loopback round-trip + tamper→verify-fail) | `cargo test -p openpulse-modem --test filexfer_loopback` |
| File-transfer multi-object >64 KB split/reassemble | `cargo test -p openpulse-filexfer --test blocks multi_object_over_64kb` |
| File transfer crosses two real daemons (twin round-trip) | `cargo test -p openpulse-daemon --test twin_daemon_bridge a_file_crosses` |
| PTT assert/release ≤ 50 ms | `cargo test -p openpulse-radio` (add timing test in `noop.rs`) |
| Periodic station ID at interval (REQ-REG-10) | `cargo test -p openpulse-core --lib station_id` + `cargo test -p openpulse-core --lib cw_id` + `cargo test -p openpulse-modem --test station_id_txcount` |
| MFSK16 sub-floor waveform: loopback + acquisition + calibrated LLRs (REQ-WSIG-01) | `cargo test -p mfsk16-plugin` + `cargo test -p openpulse-modem --test mfsk16_engine` |
| Receiver AGC: decode level-invariant on/off + AGC tracks level (REQ-AGC-01) | `cargo test -p openpulse-modem --test agc_amplitude_sweep` |
| Simultaneous multi-mode receive monitor (REQ-RX-01) | `cargo test -p openpulse-daemon --no-default-features monitor::` |
| Hotplug-safe audio device resolution (REQ-DEV-01) | `cargo test -p openpulse-core --no-default-features audio::tests` |
| CM108 / GPIO PTT backends (REQ-PTT-02/03) | `cargo test -p openpulse-radio --no-default-features cm108 gpio` |
| CI auto-runs on every PR (Linux gates + macOS build) | `.github/workflows/ci.yml` `on: pull_request`; Linux core/full/gpu/pi5 gates + a `macos-build` compile check |

For any new Phase 1 feature: write the test first, confirm it fails, implement until it passes. Do not mark a task done if its test does not exist.

---

## Coding conventions

### Rust style
- `thiserror` for error types in library crates; `anyhow` in CLI and test code
- No `unwrap()` or `expect()` in library crate production paths (`openpulse-core`, `openpulse-audio`, `openpulse-modem`, `openpulse-channel`, `openpulse-radio`). `expect()` is acceptable in tests and CLI.
- Derive `Debug`, `Clone`, `PartialEq` on config and result types
- Derive `serde::Serialize, Deserialize` on any type that crosses an API boundary or is emitted as JSON
- Use `tracing::{debug, info, warn, error}` for structured logging; no `println!` in library code
- Integer field sizes: use the smallest type that covers the domain (`u8` for counts ≤ 255, `u16` for sequence numbers, `f32` for audio samples and DSP)
- `Arc<RwLock<T>>` for shared state read by multiple threads; `crossbeam_channel` for inter-thread messaging

### Module organisation
- One concept per file; prefer small focused modules over large files
- Traits defined in `mod.rs` or a dedicated `traits.rs`; implementations in separate files named after the implementation
- Test modules inline for unit tests (`#[cfg(test)] mod tests { ... }`); integration tests in `tests/` directory

### Documentation
- All public types and functions get a one-line doc comment
- No multi-paragraph docstrings
- No comments explaining what the code does; only comments explaining why when the reason is non-obvious

### Commit style
- One logical change per commit
- Prefix: `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `ci:`
- Imperative mood: "add block interleaver" not "added block interleaver"

### PR hygiene
- Every PR must pass `cargo test --workspace --no-default-features` locally before opening
- Every PR that adds a feature must include at least one test
- Link the roadmap task in the PR description

### Traceability (required for substantive changes)
Carry the full chain in the commit message and PR body, and append an entry to
`docs/dev/project/traceability.md`:

**requirement/change → architecture/design decision (+ rationale) → implementation (files/functions) → tests → test results (actually run).**

- The "tests → results" link must be a real run (pass/fail counts), never "covered" asserted from a callers-grep.
- Keep the acceptance-criteria table above current (requirement ↔ acceptance test).
- Don't build a separate heavyweight matrix that rots — bake the chain into the artifacts that already travel with the change (commits, PRs, the `traceability.md` ledger, the acceptance table).

---

## Known sharp edges

**QPSK dependency scope mismatch (resolved).** `qpsk-plugin` is now in `[dependencies]` for both `openpulse-modem` and `openpulse-cli`, so production wiring can use QPSK paths without dependency-scope surprises.

**Watterson Doppler envelope resolution (resolved).** `WattersonChannel::make_envelope` now auto-sizes the shaping FFT so `σ_bins ≥ 2.0` even for low-Doppler profiles (e.g. Good F1 at 0.1 Hz), capped at 2^18 samples. The envelope shows meaningful temporal variation across a full call instead of collapsing to the 0.5 floor. Regression test: `f1_envelope_has_non_trivial_variation` in `crates/openpulse-channel/src/watterson.rs`.

**FEC short-payload waste (resolved for loopback / well-framed paths).** `FecCodec::encode` still emits multiples of 255 bytes; for ACK frames and small (≤ 213 B) **data** frames, callers can now select `FecMode::ShortRs` via `transmit_with_fec_mode` / `receive_with_fec_mode`. That path wraps the payload in the standard `Frame` envelope (10 B), then `ShortFecCodec::with_ecc_len(32)` (t = 16) appends 32 ECC bytes — so the wire carries `Frame(payload) + 32` bytes (≈ `payload + 42` total) instead of a 255-byte block. The receiver runs the normal `stage_decode_frame` + `HpxStateUpdate` routing after RS correction, so seq/CRC validation and HPX state updates work identically to other FEC modes. Only plugins whose demodulator emits the exact byte count of the transmitted frame are supported (loopback and well-framed half-duplex paths); OFDM/SC-FDMA padded modes are not. Regression tests: `short_fec_data_frame_engine_loopback`, `short_fec_data_frame_rejects_oversized_payload` in `crates/openpulse-modem/tests/fec_loopback.rs`.

**Audio backend opt-in (`--no-default-features` footgun).** All workspace tests and CI assume `--no-default-features` so the CPAL audio backend is **off**. Building the ARDOP TNC, KISS TNC, or testbench without their `cpal` feature produces a binary that silently falls back to `LoopbackBackend` regardless of any `[audio] backend = "cpal"` line in `config.toml`. To get real audio, build with `cargo build --release -p openpulse-kiss --features cpal` (or the equivalent for `openpulse-ardop` / `openpulse-testbench`). **`openpulse-cli` is the exception**: its audio feature is `cpal-backend` and it is **on by default**, so `cargo build -p openpulse-cli` already includes CPAL — pass `--features cpal-backend` only if you have disabled default features (`--features cpal` does not exist for the CLI and will error). The `--backend cpal` CLI flag will emit a warning at startup when the feature is absent.

**SAR is now implemented** (`crates/openpulse-core/src/sar.rs`). Objects up to 64 005 bytes can be segmented into 255-byte frame payloads and reassembled. PQ handshake (Phase 3.1) is unblocked.

**The rectangular QPSK/8PSK pulse is a crossfade — the one-slot demod loses ⅓ of the next symbol.** The "plain" modulator blends adjacent symbols with a raised cosine, so `demodulate_symbols` recovers `sym_k + ⅓·sym_{k+1}`. That `β²=−9.5 dB` ISI floor is invisible to any BER test (45° QPSK margin) but caps every soft consumer — it stalled `mean(|LLR|)` above ~12 dB and floored recovered-symbol EVM at −9.7 dB regardless of SNR. `cancel_crossfade_isi` removes it by stable backward substitution (PR #695; QPSK500 soft-FEC floor was stuck at 0.00 and now decodes). It is *anti-causal* ISI, so the DFE cannot reach it. **8PSK has the same defect and is now fixed too** (PR pending): its matched demod integrates against the *squared* window `w_tail²`, so `β = Σ w_head·w_tail² / Σ w_tail³` is **n-dependent** (0.182 at 16 sps, 0.167 at 8 sps) — computed from the window, not a constant — and 8PSK500 EVM cleared −13.7→−20.0 dB @40 dB. **The cancellation must be gated to the plain (crossfade) pulse only**: the `cosine_overlap`/`-HF` pulse is a per-symbol `sin²` bump with no inter-symbol overlap, so cancelling there injects ⅓ of the next symbol as *error* — the shipped QPSK #695 ran it unconditionally on the non-RRC path (latent soft-path corruption on `QPSK1000-HF`), fixed by the same `!cosine_overlap` guard.

**Soft combining does not dominate plain retry — take the union.** On a fading channel, summing HARQ attempts wins when every attempt is partially ruined and they carry complementary information, and *loses* when one attempt is simply clean and the sum dilutes it (measured on `moderate_f1`: SCFDMA52 @20 dB, plain retry 0.97, combining alone 0.95). `receive_with_llr_combining` therefore decodes each attempt standalone before falling back to the MAP sum — one extra RS decode over LLRs already in memory, and success becomes a strict superset of both (PR #694; SCFDMA52-16QAM @28 dB: 0.43 / 0.48 → **0.67**). Deep-fade outage is what limits SC-FDMA on HF, and diversity is the only thing that touches it.

**An uncoded-BER win is not a win.** SC-FDMA's IBDFE halved uncoded BER on a static notch and moved coded frame success by *zero*, because iterative feedback trades average residual for **confidently-wrong bits** — and soft FEC is destroyed by exactly those. Its own model noise variance was 90× optimistic (`v̄` comes from max-log LLRs, and the feedback error correlates with the noise it is subtracted from); the calibration-safe choice is to keep the pre-iteration variance and claim only a better symbol *estimate*. Measured, then reverted — see `docs/dev/research/scfdma-improvements.md` → *Rejected — P7*. Always take the **coded** number, and check `plugins/scfdma/tests/llr_reliability.rs`.

**Code rate is the last lever, not the first.** Higher-rate FEC buys throughput by *spending* SNR. Measured on SC-FDMA: `LdpcHighRate` (r≈8/9) costs +4…+8 dB of floor over `SoftConcatenated` (r≈0.437) for 2.03× the rate — a worse trade than climbing one modulation order (8PSK→16QAM: 1.33× for ~2 dB). So a rate swap on a rung that still has a denser constellation above it *loses* throughput at that rung's operating SNR. LDPC earns rungs only at the ladder's top, where 64QAM is already the densest constellation the plugin has (PR #692, SL16–SL19). Before proposing "stronger/faster FEC on rung X", measure the floor delta and compare it against the next modulation order.

**Test what an LLR *means*, not just its sign.** A true LLR `L` predicts `P(bit wrong) = 1/(1+e^{|L|})`. Bin the emitted LLRs by `|L|`, count actual errors, compare. SC-FDMA's `mmse_llr_noise_var` modelled only the additive noise — omitting channel-estimate error and the residual-ISI term `var(α_k)` — and bits with `|L| ≈ 12` were wrong **71× more often than promised**, on a *flat* channel. No frame-success metric in the repo could see it: soft Viterbi, min-sum LDPC and max-log turbo are all scale-invariant, and the missing terms were nearly a per-frame constant. Fixed in PR #690 with **no measured decode gain** — it matters for HARQ combining and for any iterative equalizer that derives feedback reliability from LLR posteriors. `plugins/scfdma/tests/llr_reliability.rs` is the gate.

**LLRs already carry `1/σ²` — do not weight them by it again.** `openpulse_dsp::constellation::symbol_llrs` divides every distance by `noise_var`, so a calibrated plugin (SC-FDMA, OFDM) emits true log-likelihood ratios whose magnitude is ∝ 1/σ². For repeated observations of the same bits, LLRs **add**: `combine_llrs_map` is the MAP combine and *is* inverse-noise weighting. The engine used to re-weight that sum by a `1 / mean(|LLR|)` proxy — a second 1/σ² — costing 0.75 dB on graded HARQ attempt sets (fixed in PR #686). `combine_llrs_weighted` is only for LLRs with a noise-blind scale (the ±1.0 trait default). Every shipped plugin is now calibrated (PR #687); `crates/openpulse-modem/tests/llr_calibration.rs` fails any plugin whose `mean(|LLR|)` stops growing with SNR. **Choosing the noise estimator is the hard part**: a demodulator's residual is not all thermal noise — pulse-shaping ISI and equalizer misadjustment vary the symbol *amplitude* with no SNR dependence, so a moment (M2/M4) or distance-to-nearest-point estimator stops tracking SNR entirely. Use the component *orthogonal* to the hard decision (`psk_symbol_noise_var`), or for a differential detector the quadrature companion, where the amplitude cancels exactly (`differential_llr_scale`).

**Acquire on the normalised correlation, not the unnormalised score.** `IqMatchedFilter::search`'s argmax favours high *energy*, on the reasoning that "a deep-fade low-energy window cannot win". When the **preamble** is the faded part that is exactly backwards — SC-FDMA lost frames to data-region windows 4896 samples later that merely shared the pilot comb (ρ = 0.994 at the true offset with energy 19.4, versus ρ = 0.657 with energy 83.0). Use `search_normalized` with an energy floor (ρ is meaningless on a silent window). This looked like "fade dynamics" for a release and was slated as a channel-estimate fix; deleting `smooth_ce` entirely left the numbers bit-identical (PR #689).

**Sync must lock ahead of the correlation peak, never on it.** A matched filter's argmax sits on whichever multipath ray is instantaneously strongest — the delayed one about half the time. A late FFT-window start pulls the next symbol in; the cyclic prefix only protects an **early** start (there the window begins inside the symbol's own prefix, a circular shift that `deramp_timing` removes). SC-FDMA locked on the argmax and lost half of all Watterson frames for it (PR #688; `good_f1` sum 9.19 → 29.57 of 42, AWGN bit-for-bit unchanged). OFDM already scanned back for the leading tap. Note the asymmetry that hid it: with the *direct* ray stronger the argmax is already right, so a **symmetric** static two-ray test passes either way — the reproduction needs `a_delayed > a_direct`.

**Delete the mechanism; if the number doesn't move, it was never the mechanism.** Three accepted explanations in this codebase were falsified in a row by removing the impairment they depend on. "Dense QAM can't hold coherence on HF" died against a noiseless in-CP two-ray channel (#685). "Notch smearing" died at 60 dB SNR — it is a noise-enhancement mechanism, and the selective-vs-flat gap was 0.50 at 32 dB and 0.51 at 60 dB (#688). "The CE lags a moving channel" died when `smooth_ce` was deleted and the flat-fade numbers came back bit-identical (#689). Run the ablation *before* building the fix the explanation implies.

**A modem that fails at *every* SNR has a bug, not a limitation.** The SC-FDMA `dft_ce_estimate` mis-reconstructed every frequency-selective channel (coarse 3.94-sample delay grid + negative taps read as large positive ones). Its signature was a *flat* 2–7% Watterson decode rate from 8 to 32 dB, and the tests recorded that as "correct and by design" for two releases. It was found by **taking the noise away**: a static two-ray FIR inside the cyclic prefix, no Doppler, 90 dB SNR — a receiver that cannot decode that has nowhere to hide. The same trick then falsified the *next* accepted explanation: "notch smearing" predicts the selective-vs-flat gap shrinks with SNR, but it measured 0.50 at 32 dB and 0.51 at 60 dB, which is how the sync bug above surfaced. Uncoded BER, flat-channel CE MSE, and all 58 unit tests were green throughout. Replacement is `channel::DelayCe` (physical delay basis, f64 normal equations, Wiener ridge with an exponential delay-power prior, and a σ² read off the pilot comb rather than a fit residual). Two lessons: (1) a metric reading "fails at all SNR" is a bug signature — write the noiseless test first; (2) when a DSP change regresses, swap **one** component behind a switch and hold the rest — that is what showed the AWGN loss belonged to the missing ridge, not to the delay basis.

**RX capture has two entry families — wire RX front-end DSP at the seam, not a caller.** Captured audio reaches demod by two distinct routes: the `receive*` family (`stage_capture_input` → `receive_from_samples`) and the **daemon's streaming path** (`accumulate_capture` → `accumulate_routed`, the one `server::run`'s `rx_ticker` actually uses). They both funnel through exactly one shared seam: **`route_audio_stage(PipelineStage::InputCapture)`** (~19 call sites). The receiver notch lives there so every path gets it by construction; `ModemEngine::notch_blocks_processed()` is a tripwire that stays 0 if an enabled feature never runs on a path. The original notch bug put the transform in `stage_capture_input` only, so it covered the `receive`-family tests but never ran in the daemon. Lesson: a receiver/transmitter front-end transform belongs at the single pipeline-stage seam, and must be tested through the **production entry function** (`accumulate_capture` / the `twin` harness), not only `ChannelSimHarness`/`receive()`.

**Cross-cutting RX/TX feature checklist (avoids the gap above).** When adding a feature that must run on *every* receive or transmit: (1) trace **top-down from the binary** — `server::run`'s `rx_ticker`/`tx` path, not just the engine API — to find what the running daemon actually calls; (2) place the transform at the single shared seam (`route_audio_stage(InputCapture)` for RX), never in one of the many caller functions; (3) never claim "covers all paths" from a callers-grep — prove it with a test that **fails without the wiring**; (4) add a runtime tripwire (a processed-block counter) and assert it increments on the production path; (5) add at least one test through the production entry (`accumulate_capture` or the `twin` daemon harness), not only the convenience seam.

---

## DSP acquisition & carrier-recovery playbook

Blind acquisition — recovering timing, frequency, **and** phase simultaneously from a 16-symbol preamble — is the single most-churned and most-misdiagnosed area of the modem (60+ AFC/carrier commits). These are hard-won, load-bearing practices; read them before touching any plugin's demod or the engine acquisition path.

1. **Diagnose an "AFC" failure with the swept-applied-correction experiment FIRST.** When a mode won't acquire through an offset, modulate at `fc+Δ`, then demodulate with a *manually swept* `afc_correction_hz` (and matching `center_frequency`). If it fails even at the exactly-correct Δ, the estimator/AFC is **innocent** — the bug is in timing, onset, or the carrier tracker. This one check relocated the 8PSK gap (PR #417) from "AFC precision" (where earlier sessions spent days on FLL / preamble-redesign / liquid-dsp ports) to a broken drift-fit branch in `carrier_phase_correct`.

2. **AFC is the usual suspect, rarely the culprit.** The acquisition chain is `energy gate → refine_onset → afc_mini_settle → decode → carrier tracker` (`crates/openpulse-modem/src/engine.rs`); a weakness in *any* link reads as "doesn't decode → must be AFC." Historically these were: onset landing (BPSK31 #406, QPSK500 #413), timing metric at 90° carrier phase (`5dded08`/`866b085`), sample-rate offset on the dual-clock rig (#391/#392/#397), and carrier tracking (8PSK #417) — **not** the AFC estimate.

3. **Settle AFC on the refined-onset window, never the coarse energy-gate window** (it may be mostly silence → a confident-but-bogus estimate, e.g. QPSK500's spurious ~257 Hz). And **don't apply sub-noise-floor (<2 Hz) settled corrections** (`AFC_SETTLE_DEADBAND_HZ` in `engine.rs`).

4. **Carrier recovery is acquire-then-track, not one loop.** A gentle (low-BW) loop holds lock but **cannot acquire** even a ~1 Hz residual over a short (~60–200 symbol) frame. Use two passes: pass 1 wide BW to acquire the frequency, pass 2 narrow BW *seeded* with it to track cleanly. 64QAM (`dd_carrier_track_2pass`) and 8PSK (`dd_track_seeded`, #417) both do this. A single high-BW loop fixes the offset but regresses clean/dense modes (8PSK9600) — the split keeps both.

5. **Don't try to extract sub-Hz CFO from the 16-symbol preamble by a magnitude-peak frequency search.** Its frequency resolution is only ~baud/16 (31–62 Hz) and the magnitude metric is sidelobe-ridden; a coarse scan locks to spurious peaks (−100…−256 Hz observed). Use a scan only for *coarse* acquisition; leave the fine residual to the 2-pass tracker. The data-aided mean-phase-increment estimator is the precise stage (ISI-biased ~0.9 Hz, which the tracker now absorbs).

6. **Dense constellations are the regression canaries.** 8PSK (±22.5° margin) and 64QAM surface every timing/phase/AFC weakness that BPSK/QPSK hide. Validate acquisition changes against them, not just BPSK.

7. **Rebuild BOTH ends for any loopback test** — the preamble sequence and frame geometry are shared protocol; a one-sided rebuild fails silently with "invalid magic."

8. **Test FEC-protected modes WITH their FEC.** Dense modes (SCFDMA-HOM, 64QAM) only ever run FEC-protected, so a no-FEC loopback is an unrealistic bar — use the loopback `FEC=` env / CLI `--fec`. Soft FEC (~+6 dB) was the bigger lever that the loopback had never exercised.

External modem/DSP references (gnuradio FLL band-edge, liquid-dsp framesync, daniestevez/qo100-modem) are catalogued in `docs/dev/research/references.md`. Recurring lesson: those references all use **RRC pulse shaping + a dedicated frequency-acquisition stage**; our rectangular single-Costas PSK is the outlier, which is why band-edge techniques don't drop in cleanly.

---

## Key documents by topic

| Topic | Document |
|---|---|
| Channel models (Watterson, Gilbert-Elliott) | `docs/dev/benchmark-harness.md` |
| Testbench design (channel models, DSP, UI) | `docs/dev/design/testbench-design.md` |
| WSJTX weak-signal techniques | `docs/dev/research/wsjtx-analysis.md` |
| JS8Call speed ladder and ARQ commands | `docs/dev/research/js8call-analysis.md` |
| VARA architecture and ACK taxonomy | `docs/dev/research/vara-research.md` |
| PACTOR Memory-ARQ, interleaver, FEC | `docs/dev/research/pactor-research.md` |
| ARDOP research | `docs/dev/research/ardop-research.md` |
| HPX waveform design | `docs/dev/design/hpx-waveform-design.md` |
| HPX state machine | `docs/dev/hpx-session-state-machine.md` |
| Protocol & handshake wire format (frame/SAR/CONREQ/CONACK/ACK/manifest) | `docs/dev/design/protocol-wire-spec.md` |
| Peer query and relay wire format | `docs/dev/peer-query-relay-wire.md` |
| Regulatory compliance | `docs/regulatory.md` |
| Roadmap and phase gates | `docs/dev/project/roadmap.md` |
| Requirements | `docs/dev/requirements.md` |
| Architecture | `docs/dev/design/architecture.md` |
| PKI tooling | `docs/dev/pki/pki-tooling-architecture.md` |
| CLI usage | `docs/cli-guide.md` |
| Benchmark harness spec | `docs/dev/benchmark-harness.md` |
| External modem/DSP references (FLL, liquid-dsp, qo100-modem) | `docs/dev/research/references.md` |
| JS8 discovery & rendezvous plan (D1–D7 locked; Phases A–G shipped — RX + beacon TX + rendezvous → HPX handoff; only H on-air remains) | `docs/dev/design/js8-discovery-rendezvous-plan.md` |
| Direct P2P file-transfer plan (D1–D5 locked; Phases A–E shipped, on-air deferred) | `docs/dev/design/file-transfer-plan.md` |
| VarAC feature-gap analysis (ideas we're missing; research, not scheduled) | `docs/dev/research/varac-feature-gap-analysis.md` |
| GPU LDPC BP prototype findings | `docs/dev/gpu-ldpc-prototype.md` |
| OTA adaptive rate-stepping hardware validation | `docs/dev/ota-hardware-validation.md` |
| On-air twin-OTA scenario (two daemons + twinview over RF) | `docs/dev/onair-twin-ota.md` |
| Loopback transports (virtual → hardware → on-air) | `docs/dev/virtual-loopback.md` |
| Agent safety rules | `AGENTS.md`, `docs/dev/AGENTS.md` |
