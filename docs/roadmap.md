---
project: openpulsehf
doc: docs/roadmap.md
status: living
last_updated: 2026-05-06
---

# Roadmap

## Scope policy

- Primary goal: build an independent and competing OpenPulse protocol from scratch.
- External/proprietary compatibility modes (for example VARA and PACTOR-4) are secondary and must not proceed without legal review and explicit approval.
- Regulatory compliance (FCC Part 97, CEPT/EU, UK Ofcom) is a hard requirement before any on-air transmission use. See docs/regulatory.md.

---

## Phase 0 — Foundation (Completed)

All Phase 0 work has shipped and merged.

### Shipped in PR #49
- ✅ HPX benchmark harness: inputs, scenarios, reproducible run procedure, JSON result schema.
- ✅ Signed transfer envelope format (header, payload_hash, signature_block).
- ✅ CI benchmark regression gates (100% pass rate, mean_transitions ≤ 20.0).
- ✅ HPX session persistence to `~/.config/openpulse/session-state.json`.
- ✅ Trust-store CLI commands: import, list, revoke.
- ✅ ARM64 cross-compile validation (aarch64-unknown-linux-gnu).
- ✅ Raspberry Pi 5 smoke-test profile (loopback + benchmark).
- ✅ CI auto-trigger on push and pull requests.

### Shipped in PR #50
- ✅ Hardened BPSK TX/RX under loopback with 56-scenario fixture matrix.
- ✅ Structured session diagnostics with transition event capture.
- ✅ `session state --diagnostics` JSON output.

### Shipped (multithreaded pipeline + session management)
- ✅ Explicit pipeline stage boundaries and bounded-channel scheduler.
- ✅ Per-stage scheduler metrics in diagnostics.
- ✅ `session list` and `session resume` commands.
- ✅ `session log --follow` streaming mode.

### Shipped (FEC phase 1)
- ✅ `FecCodec` in `openpulse-core` with GF(2^8) RS codec (ECC_LEN=32, corrects up to 16 byte errors/block).
- ✅ `ModemEngine::transmit_with_fec` and `receive_with_fec`.
- ✅ BER-injection correctness tests and 20-scenario loopback fixture matrix.

### Shipped (QPSK phase 1)
- ✅ `qpsk-plugin` with Gray-mapped QPSK modulate/demodulate (PR #56).
- ✅ CLI plugin registration for QPSK125, QPSK250, QPSK500.
- ✅ Loopback fixture matrix and spectral efficiency benchmarks.

---

## Phase 1 — Protocol Foundation (Completed)

All Phase 1 items shipped. See CLAUDE.md for per-PR completion records.

### 1.1 — CI and codebase health ✅ Done (PR #67, #68)
### 1.2 — Segmentation and reassembly (SAR) ✅ Done
### 1.3 — Interleaver integration ✅ Done (PR #70)
### 1.4 — Channel model implementation ✅ Done (PR #71)
### 1.5 — Radio interface layer ✅ Done (PTT, AFC, CLI wiring)
### 1.6 — Real-device validation *(deferred — replaced by Phase 3.5-substitute)*

---

## Phase 2 — HPX Protocol Completion (Completed)

All Phase 2 items shipped.

### 2.1 — ACK frame taxonomy and rate adaptation ✅ Done
- `AckType` (8 variants), `AckFrame` (5-byte codec, CRC-8/SMBUS, FNV-1a session hash).
- `SpeedLevel` (SL1–SL11), `RateAdapter` state machine; ACK-DOWN floors at SL2; ChirpFallback via 3 consecutive NACKs.
- `Fsk4Plugin` for FSK4-ACK: 4 tones, 100 baud, Hann-windowed, Goertzel demodulator.
- 11 rate adaptation tests; 3 FSK4 loopback tests.

### 2.2 — HPX500 and HPX2300 adaptive profiles ✅ Done
- `QPSK1000` mode added; `Psk8Plugin` for `8PSK500` and `8PSK1000` (Gray-coded, single-carrier).
- `SessionProfile::hpx500()` (SL2=BPSK31 → SL6=QPSK500) and `SessionProfile::hpx2300()` (SL8=QPSK500 → SL11=8PSK1000).
- HPX2300 waveform decision: single-carrier over OFDM (lower PAPR, simpler AFC — see docs/architecture.md).
- 4 session profile tests; 8 adaptive profile integration tests.

### 2.3 — Signed handshake and manifest verification ✅ Done
- `ConReq`/`ConAck` wire frames with Ed25519 sign/verify; trust evaluation via `evaluate_handshake()`.
- `TransferManifest` with SHA-256 payload hash, sender ID, Ed25519 signature; `verify_manifest()`.
- 12 handshake integration tests; 6 manifest integration tests.

### 2.4 — Channel access and shared-channel operation ✅ Done
- `DcdState`: RMS energy threshold, 100 ms hold window, `update()`/`is_busy()`/`energy()`.
- 0.3-persistence CSMA in `stage_emit_output()`; `ModemError::ChannelBusy` on blocked TX.
- 4 CSMA loopback integration tests.

### 2.5 — Peer cache and query subsystem ✅ Done
- `PeerDescriptor`: self-authenticating signed identity; `peer_id` IS the Ed25519 verifying-key bytes.
- `PeerCache::query()` with `TrustFilter` and capability mask; `PeerQueryRequest`/`PeerQueryResponse` wire envelopes.
- 9 peer descriptor tests; 9 wire query tests.

### 2.6 — Multi-hop relay ✅ Done
- `RelayForwarder`: hop-limit enforcement, duplicate suppression, trust-policy check.
- Trust-weighted path scoring; `score_route`/`select_best_scored_route`.
- `RelayDataChunk` (msg_type 0x05) and `RelayHopAck` (msg_type 0x06) wire codecs.
- 12 relay integration tests (path scoring, 3-hop chain, event drain, wire round-trips).

### 2.7 — Compression (session layer) ✅ Done
- `lz4_flex 0.11`; `compress_if_smaller()` — compress-then-compare, returns original if LZ4 not smaller.
- `ConReq`/`ConAck` carry `supported_compression`/`selected_compression`; covered by Ed25519 signature.
- 9 compression integration tests.

---

## Phase 3 — Advanced Signal Processing and Compliance (Partial)

Most Phase 3 items shipped. Remaining: 3.2 (Turbo FEC evaluation, deferred) and 3.5 on-air validation.

### 3.1 — Post-quantum in-band handshake ✅ Done
- `ml-dsa 0.1.0-rc.9` (ML-DSA-44) and `ml-kem 0.3` (ML-KEM-768) added to `openpulse-core`.
- `PqConReq`/`PqConAck` wire frames with classical + PQ pubkeys, KEM pubkey/ciphertext, dual signatures.
- Hybrid mode: Ed25519 + ML-DSA-44 dual signature; X25519 + ML-KEM-768 dual KEM.
- SAR encode→fragment→reassemble→decode round-trip validated for full PQ payload sizes.
- 12 PQ handshake integration tests.

### 3.2 — Convolutional FEC evaluation ✅ Done
- No pure-Rust Turbo (BCJR/MAP iterative) crate exists; proxy: rate-1/2 convolutional code (K=3, G={7,5} octal) with hard-decision Viterbi decoder.
- `crates/openpulse-core/src/conv.rs`: `ConvCodec` — same interface as `FecCodec` (encode/decode with 4-byte length prefix).
- Benchmark (`crates/openpulse-core/tests/fec_comparison.rs`, 6 tests): at channel BER 0.01 (AWGN), RS post-decode BER = 0.497 vs ConvCodec post-decode BER = 0.0004; CPU overhead 3.8×.
- **Decision: ACCEPTED** — ConvCodec added as an optional alternative FEC for AWGN-dominant paths (e.g., VHF line-of-sight). RS+interleaver remains default for HF burst-error profiles. Result documented in `docs/vara-research.md`.

### 3.3 — GPU acceleration ✅ Done (PR #90)
- `crates/openpulse-gpu`: `GpuContext` (wgpu device + pre-compiled pipelines), WGSL kernels for BPSK modulation, IQ demodulation, and timing offset search.
- `bpsk-plugin` gains optional `gpu` feature; CPU fallback when GPU returns `None`.
- All GPU functions return `Option<T>` to surface failures rather than silently returning empty/zero data.
- GPU-vs-CPU equivalence tests under `#[cfg(feature = "gpu")]`.

### 3.4 — ARDOP-compatible TCP interface ✅ Done (PR #91)
- ARDOP-compatible TCP command/data port interface (`crates/openpulse-ardop`).
- Command port (default 8515): ASCII line protocol — VERSION, MYID, LISTEN, CONNECT, DISCONNECT, ABORT, STATE, BUFFER, PTT, CLOSE.
- Data port (default 8516): `u16 BE` length-prefixed binary framing.
- `openpulse-tnc` binary; loopback mode for protocol-level integration tests.
- 8 integration tests covering all major ARDOP commands and data port framing.
- Legal review still required before labelling as "ARDOP-compatible" in public documentation.

### 3.5 — Regulatory on-air validation *(Phase 3.5-substitute shipped)*
Phase 3.5-substitute (sound-loopback channel simulation) is done (PR #89):
- `ChannelSimHarness` wires two `ModemEngine` instances through `openpulse-channel` models.
- 6 channel-loopback integration tests (clean, AWGN 20 dB, Watterson F1/F2, Gilbert-Elliott with/without FEC).
- These replace on-air validation as the CI gate for loopback correctness.

Remaining on-air items:
- Conduct on-air tests on IARU-aligned frequencies for each supported bandwidth class.
- Verify station identification behaviour at 10-minute intervals under long sessions.
- Test relay node automatic control point interface.
- Publish compliance test report as a release artefact.

---

## Phase 4 — Ecosystem and Long-term (Active)

**Active phase.** Phase 4.2, 4.1, 4.3, 4.4, and 4.5 shipped.

### 4.2 — Observability and automation ✅ Done (PR #92)
- `EngineEvent` broadcast channel in `ModemEngine` — 8 event variants (AFC update, rate change, DCD flip, HPX transition, frame TX/RX, session start/end).
- `engine.subscribe()` returns a `broadcast::Receiver<EngineEvent>` for real-time event consumption.
- `HpxState`, `HpxEvent`, `RateEvent` all derive `Serialize/Deserialize`.
- `openpulse monitor --mode <MODE>` CLI subcommand streams all engine events as NDJSON to stdout; pipeable to `jq` or scripts.
- 7 integration tests in `engine_events.rs` (including DcdChange and AfcUpdate coverage).

### 4.1 — TUI frontend ✅ Done
- New `crates/openpulse-tui` binary crate using ratatui 0.27 + crossterm.
- Background worker thread drives `ModemEngine::receive()` and forwards events to TUI via `std::sync::mpsc`.
- Live panels: HPX state (colour-coded), AFC offset + rate/mode, DCD energy bar (Gauge), scrollable transitions log (last 50).
- Keyboard: `q`/Ctrl+C to quit, `p` to pause updates, `↑↓` to scroll transition log.

### 4.3 — KISS and AX.25 interface ✅ Done
- New `crates/openpulse-kiss` crate with `openpulse-kisstnc` binary.
- KISS frame encode/decode with full byte stuffing (FEND/FESC).
- AX.25 UI frame encode/decode (callsign, SSID, Control=0x03, PID=0xF0).
- TCP listener (default port 8100) using the same `broadcast` + `std::sync::mpsc` bridge pattern as `openpulse-ardop`.
- 0.3-persistence CSMA (Phase 2.4) already in `ModemEngine`; honoured on TX path.
- Target: APRS clients (APRSdroid, PinPoint, Xastir) via KISS over TCP.
- 8 integration tests (KISS codec round-trips, byte stuffing, AX.25 frame round-trip, TCP loopback with multi-frame and special bytes).

### 4.5 — Signal-path testbench GUI ✅ Done
- New `apps/openpulse-testbench` binary crate using egui/eframe 0.29.
- 4-column live signal-path view: TX (clean), Noise channel, Mixed (TX+noise), RX (decoded).
- Per-tap spectrum plot (FFT, dBFS, configurable dB range) and plasma-colourmap waterfall display.
- Toolbar: mode selector (BPSK31–QPSK500), noise model (AWGN/GE/Watterson/QRN/QRM/QSB/Chirp), SNR slider, FEC toggle, RNG seed, dB range sliders.
- Stats bar: runs, OK, fail, BER, last event from the rolling event log.
- Signal thread driven by `bpsk-plugin` / `qpsk-plugin` directly (no full `ModemEngine`); communicates with UI via `Arc<RwLock<TapData>>` and `crossbeam_channel` stop signal.
- All 7 supported channel models wired; `build_channel()` factory from `openpulse-channel`.

### 4.4 — B2F protocol and Winlink gateway integration ✅ Done
- New `crates/openpulse-b2f` pure-protocol library (no tokio, no modem engine dependency).
- Banner encode/decode: `[WL2K-3.0-B2FWINMOR-4.0-XXXXXXXX]` format with FNV-1a session key.
- B2F control frame codec: FC (file check), FS (file select), FF (finished), FQ (quit); CR-terminated ASCII.
- WL2K message header encode/decode (RFC-5322-like, CRLF-terminated; Mid, Date, From, To, Subject, Body, File, Mbo).
- Compression: Gzip (type D) via `flate2`; LZHUF (type C) stub preserving API surface for future implementation.
- `B2fSession` state machine: ISS (Information Sending Station) and IRS (Information Receiving Station) roles; Handshake → ProposalExchange → Transfer → Done states; handles ISS-immediate-proposal pattern.
- Pat-client ARDOP compatibility: added GRIDSQUARE, ARQBW, ARQTIMEOUT, CWID, SENDID, PING commands to `openpulse-ardop`; 3 new integration tests (11 total).
- 9 integration tests in `crates/openpulse-b2f/tests/b2f_integration.rs`.

---

## Phase 5 — Integration and Release Readiness (Completed)

All Phase 5 items shipped. On-air regulatory validation (Phase 3.5) is
explicitly postponed — no hardware gate blocks further development.

### 5.1 — B2F session driver ✅ Done (PR #98)
- `crates/openpulse-b2f-driver`: `B2fDriver`, `DataPort`, `CmdPort`; `run_iss()` / `run_irs()` lifecycle.
- 4 driver integration tests; shared test helpers in `tests/common/mod.rs`.

### 5.2 — LZHUF codec (Type C) ✅ Done (PR #98)
- Real LH5 via `oxiarc-lzhuf 0.2.7`; 4-byte BE length prefix; 16 MiB decompression cap.
- `B2fSession::accepted_count()` added to drive IRS data-read loop.

### 5.3 — TOML configuration management ✅ Done (PR #102)
- `crates/openpulse-config`: typed schema, `load()`, `init_template()`; CLI precedence over config.
- `openpulse-tnc` and `openpulse-kisstnc` accept clap CLI flags overriding config file.

### 5.4 — End-to-end loopback integration test ✅ Done (PR #100)
- `crates/openpulse-b2f-driver/tests/e2e_loopback.rs`: bidirectional modem relay through `ChannelSimHarness`.
- `e2e_single_message_awgn_20db` and `e2e_multi_message_clean` — no hardware required.

### 5.5 — Direct TCP Winlink CMS gateway ✅ Done
- `crates/openpulse-gateway`: ISS send + IRS receive over a single `TcpStream` to `cms.winlink.org:8772`.
- `gateway_round_trip` unit test: mock CMS TCP server validates full exchange without network access.

### 5.6 — CpalBackend wiring and on-air test plan ✅ Done (PR #105)
- `AudioConfig` in `openpulse-config`; `--backend` CLI flag; cpal feature gate.
- `docs/on-air_testplan.md`: hardware prereqs, test matrix, regulatory checklist, diagnostics table.

### 5.7 — Testbench live audio capture ✅ Done (PR #108)
- `AudioSource` enum (`Synthetic` / `LiveCapture`); `run_live()` opens system input at 8 kHz mono.
- Source combo disabled while running; panel labels reflect live mode.

### 5.5-reg — Phase 3.5 on-air regulatory validation *(postponed — no target date)*
Conduct on-air tests on IARU-aligned frequencies, verify station ID at 10-minute intervals,
test relay automatic control point interface, publish compliance report as release artefact.

---

## Phase 6 — AFC, Interoperability, and Network ✅ Done

### 6.1 — AFC correction loop ✅ Done (PR #116)

Close the automatic frequency control feedback path.  The IQ-squaring estimator
(`estimate_frequency_offset` / `afc_estimate_hz`) already runs after every receive call
and its result is exposed via `ModemEngine::last_afc_offset_hz()`.  What is missing is
the correction step.

- Add `afc_correction_hz: f32` field to `ModemEngine`; accumulated from `last_afc_offset_hz`
  with a configurable step size (default 0.1 × estimated offset per frame — slow loop).
- Pass corrected carrier frequency `fc + afc_correction_hz` into each demodulate call.
- Expose `enable_afc()`, `disable_afc()`, `reset_afc()`, `set_center_frequency()` on `ModemEngine`; default enabled.
- `AfcUpdate` event extended with `correction_hz` field so TUI and NDJSON consumers see both
  the residual estimate and the accumulated correction; `#[serde(default)]` for backward compat.
- Integration tests: loopback with a 15 Hz TX/RX carrier offset; asserts AFC converges to
  within ±2 Hz within 25 frames at BPSK100.

### 6.2 — pat / Winlink interoperability ✅ Done (PR #118)

Verify an end-to-end Winlink round-trip driven by `pat` connecting to `openpulse-tnc` via
its ARDOP interface.

- Document and fix any wire-level incompatibilities found during `pat connect` against the TNC.
- Add `FECSEND` and `FECRCV` ARDOP commands (used by pat for FEC-framed transfers).
- Verify `BUFFER` polling behaviour matches pat's expectations during TX drain.
- Packaging: produce a reproducible `.deb` for Raspberry Pi OS (aarch64) and a static
  Linux x86-64 binary; both published as GitHub release assets.
- Acceptance: `pat` can send a message via `openpulse-tnc` and retrieve it back without
  any manual intervention beyond normal `pat` UI operation.

### 6.3 — Network mesh layer ✅ Done (PR #120)

Promote the relay, peer-cache, and query-propagation modules from library code to a running
network service.

- New binary `openpulse-mesh` (or `openpulse-node`) that runs the full HPX relay stack as
  a daemon: peer discovery beacons, query forwarding, store-and-forward relay.
- `CONNECT_MESH` extension to the ARDOP command port: directs `ModemEngine` to enter mesh
  mode, accepting relay frames alongside direct-addressed frames.
- Config: `[mesh]` section in `config.toml` — `enabled`, `max_hops`, `relay_policy`
  (trust-level minimum), `store_forward_ttl_s`.
- Integration tests: 3-node loopback mesh (`ChannelSimHarness` × 2 hops); verify
  a frame addressed to node C arrives via relay through B from A.

### 6.4 — Peer cache wired into mesh daemon ✅ Done (PR #121)

Wire `PeerCache` into `MeshDaemon` so beacon responses populate the local peer table and
nodes can answer peer-query requests from their cached knowledge.

- `MeshDaemon` gains a `PeerCache` field (capacity and TTL from `[mesh]` config).
- Self-seeded at construction so every node always includes itself in query responses.
- `WireMsgType::PeerQueryRequest` dispatch: query local cache, broadcast
  `PeerQueryResponse`, then propagate the request for multi-hop discovery.
- `WireMsgType::PeerQueryResponse` dispatch: upsert results into cache; emit
  `MeshEvent::PeerDiscovered` for new entries; `route_quality` derived from
  `envelope.hop_index`.
- New `MeshEvent` variants: `PeerQueried` and `PeerDiscovered`.
- Integration test: `peer_discovery_via_beacon` — A beacons → B responds with self →
  A caches B and emits `PeerDiscovered`.

---

## Phase 7 — Operator Panel and Dual-Rig Control

### 7.1 — Hamlib full CAT control (`openpulse-radio` extension)

Extend `openpulse-radio` beyond PTT to full rig CAT control via the existing `rigctld`
TCP interface.  The existing `RigctldPtt` already holds a TCP connection; this task
promotes it to a general-purpose `RigctldController`.

**New API surface (`openpulse-radio`)**
- `RigctldController`: wraps a `TcpStream` to `rigctld`; exposes:
  - `set_frequency(hz: u64)` / `get_frequency() -> u64`
  - `set_mode(mode: RigMode)` / `get_mode() -> RigMode`  (`RigMode` enum: USB, LSB, FM, AM, CW, …)
  - `get_signal_strength() -> i32` — S-meter reading in dBm
  - `get_power_out() -> f32` — forward power in watts
  - `get_alc() -> f32` — ALC level (0.0–1.0)
  - `get_swr() -> f32` — SWR reading
  - `ptt_on()` / `ptt_off()` — replaces `RigctldPtt`
- All methods return `Result<T, RadioError>`; `RadioError` gains a `RigctldIo` variant.
- `[radio]` TOML section: `rigctld_addr` (default `"127.0.0.1:4532"`); no breaking change
  to existing PTT-only config.

**Integration tests** (`crates/openpulse-radio/tests/rigctld_integration.rs`):
- Mock `rigctld` TCP server responding to `\get_freq`, `\set_freq`, `\get_level STRENGTH`, etc.
- Round-trip: set frequency → read back; set mode → read back; S-meter parse.

**Dependencies**: none (rigctld TCP protocol is already understood from `RigctldPtt`).

---

### 7.2 — Dual-rig support and cross-band repeater

Add support for a second transceiver and wire both into a cross-band repeater mode.

**Config**

```toml
[radio.rig_a]
rigctld_addr = "127.0.0.1:4532"   # primary rig (RX/TX for normal operation)

[radio.rig_b]
rigctld_addr = "127.0.0.1:4533"   # secondary rig (TX for cross-band repeater)
```

**New crate: `openpulse-repeater`** (or module in `openpulse-mesh`):
- `CrossBandRepeater`: holds two `RigctldController` instances and two `ModemEngine`
  instances (one per audio device); configurable from `[repeater]` TOML section.
- Repeater loop: `rig_a` engine receives a decoded frame → re-encode → `rig_b` transmits.
- PTT sequencing: `rig_b.ptt_on()` before TX, `rig_b.ptt_off()` after; configurable
  TX hang timer (`tx_hang_ms`, default 500 ms) before PTT release.
- `[repeater] enabled = false` — opt-in; disabled by default.
- Operator must configure both rigs and TOML explicitly; no auto-activation.

**Integration tests**: loopback cross-band relay — two `LoopbackBackend` + mock
`rigctld` servers; verify decoded frame on rig_a is re-transmitted on rig_b.

**Dependencies**: Phase 7.1 (`RigctldController`).

---

### 7.3 — Daemon control protocol

Add a structured NDJSON-over-TCP control port to the server daemon, enabling a thin
client (Phase 7.4) to display real-time status and send operator commands.

**Why NDJSON, not protobuf**: `EngineEvent`, `HpxState`, and `RateEvent` already
derive `serde::Serialize/Deserialize` and are streamed as NDJSON by `openpulse monitor`.
The control port is an extension of that existing stream.  Protobuf would require
maintaining a parallel `.proto` schema alongside the Rust type system with no benefit on
a local control channel; it also adds a `protoc` build-time dependency that complicates
cross-compilation to aarch64.  If a future web client materialises, NDJSON over WebSocket
requires no protocol change.

**Protocol** (newline-delimited JSON over TCP, default port 9000):

*Server → client* (event stream, unsolicited):
```json
{"type": "EngineEvent", "event": { ... }}           // existing EngineEvent variants
{"type": "Metrics", "effective_bps": 245, "ecc_rate": 0.03, "compress_ratio": 1.8,
 "afc_correction_hz": -3.2, "signal_strength_dbm": -87}
{"type": "RigStatus", "rig": "a", "freq_hz": 14074000, "mode": "USB",
 "power_w": 50.0, "alc": 0.12, "swr": 1.4}
```

*Client → server* (request/response, each on one line):
```json
{"cmd": "set_mode",    "mode": "BPSK250"}
{"cmd": "set_freq",    "rig": "a", "freq_hz": 14074000}
{"cmd": "accept_qsy",  "token": "abc123"}
{"cmd": "reject_qsy",  "token": "abc123"}
{"cmd": "enable_repeater"}
{"cmd": "disable_repeater"}
```

Server responds with `{"ok": true}` or `{"ok": false, "error": "..."}` on the same
connection.

**New crate: `openpulse-server`** (thin binary; library lives in `openpulse-daemon`):
- Wraps the modem engine, ARDOP bridge, KISS bridge, and mesh daemon into one process.
- Spawns the control port listener alongside the existing service ports.
- `Metrics` events emitted on a timer (default 1 Hz) from a background task reading
  `engine.last_afc_offset_hz()`, `engine.dcd_energy()`, and session stats.
- `RigStatus` events emitted at 2 Hz when rig CAT is configured.

**Integration tests** (`tests/control_port.rs`):
- Connect TCP client, verify `EngineEvent` stream flows.
- Send `set_mode` command; verify engine mode changes.
- Send `set_freq`; verify mock rigctld receives the frequency command.

**Dependencies**: Phase 7.1 (rig status), Phase 4.2 (EngineEvent already exists).

---

### 7.4 — `openpulse-panel` operator UI

A native egui desktop application that connects to the `openpulse-server` control port
and provides the full operator experience.

**Layout** (four-region egui window):

```
┌─────────────────────────────────────────────────┐
│  Rig A: 14.074 MHz USB  50W  SWR 1.4  [QSY ▼]  │  ← rig status bar
│  Rig B: 145.500 MHz FM  25W  SWR 1.2  [XBAND]  │
├────────────────────┬────────────────────────────┤
│  Waterfall / FFT   │  Session status             │
│  (egui_plot line)  │  Mode:     BPSK250          │
│                    │  Speed:    SL5 / 245 bps    │
│                    │  ECC:      3.1 %            │
│                    │  Compress: 1.8×             │
│                    │  AFC:      −3.2 Hz          │
│                    │  DCD:      ████░░  −92 dBm  │
├────────────────────┴────────────────────────────┤
│  Event log (scrollable, last 100 events)        │
└─────────────────────────────────────────────────┘
```

**Controls**: mode selector combo, connect/disconnect button, repeater toggle, QSY
accept/reject buttons (appear only when a QSY proposal is pending), server address field.

**Waterfall**: `egui_plot` line plot of the last FFT snapshot from `Metrics` events;
plasma colourmap waterfall texture (same approach as `openpulse-testbench`).

**Connection model**: panel connects to `HOST:9000` (default `127.0.0.1:9000`);
reconnects automatically on drop; server address editable in the toolbar.

**Dependencies**: Phase 7.3 (control protocol), `egui`/`eframe` (already in workspace).

---

### 7.5 — Signed remote rig control (over-the-air)

Allow a trusted peer to send signed rig-control commands over the air, enabling remote
operation of the second transceiver without an internet link.

**Wire format**: new `WireMsgType::RigCtrlCmd` (msg_type 0x09) carrying a CBOR/JSON
payload signed with the sender's Ed25519 key:
```json
{"cmd": "set_freq", "rig": "b", "freq_hz": 14074000, "ts_ms": 1234567890}
```
Signature covers `cmd + rig + freq_hz + ts_ms`; replay window 30 s.

**Trust policy**: `[remote_control] allow_trustlevels = ["verified"]`; Reduced and Unknown
peers are always rejected; operator must opt in explicitly.

**Acceptance**: `MeshDaemon` dispatches `RigCtrlCmd` frames to a new
`RemoteControlHandler` that validates the signature, checks trust level, enforces the
replay window, and calls `RigctldController` if all checks pass.

**Integration tests**: signed command accepted from Verified peer; rejected from Unknown
peer; replayed command rejected; tampered signature rejected.

**Dependencies**: Phase 7.1 (rig controller), Phase 7.2 (dual-rig), Phase 6.3 (mesh
daemon dispatch), Phase 2.3 (Ed25519 signing infrastructure).

---

### 7.6 — Full-duplex mode *(stretch — requires dual audio hardware)*

When two `CpalBackend` instances are available (one per rig), enable simultaneous TX on
rig_b while receiving on rig_a.  This requires the two audio streams to run on separate
hardware devices to avoid TX self-interference.

**Gating conditions**:
- Dual-rig (Phase 7.2) operational.
- TX and RX on physically separate audio devices (enforced at config parse time;
  same device name for both rigs → error at startup).
- Operator explicitly sets `[repeater] full_duplex = true`; default false.

**What changes**: the cross-band repeater's TX-hang timer is removed; `rig_b` PTT stays
asserted continuously while relay traffic is flowing; DCD on rig_a drives the decode
path regardless of rig_b TX state.

**Dependencies**: Phase 7.2, Phase 5.6 (CpalBackend wiring), hardware availability.

---



Features deliberately deferred beyond Phase 6.  Each item requires significant design
work, hardware availability, or explicit operator configuration that is not yet in scope.

### FF-1 — Operator-initiated QSY (frequency change) negotiation

Allows two stations to collaboratively move to a better channel when the current
frequency is impaired by QRM, QSB, or QRN.  The procedure is explicit and
operator-enabled; it is never triggered automatically.

**Operator prerequisites**
- QSY must be explicitly enabled per trust level in `config.toml`
  (`[qsy] allow_trustlevels = ["verified", "psk_verified"]` — untrusted is off by default
  but can be enabled; the operator takes responsibility).
- CAT control via hamlib must be configured and active (`[hamlib]` section).

**Procedure**

1. Either station may initiate.  The requesting station sends a `QSY_REQ` frame
   containing a request token and the number of candidate frequencies it will scan.
2. The requesting station uses hamlib to scan a configurable set of candidate frequencies
   (S-meter / noise-floor measurement), then returns to the current operating frequency.
3. It sends a `QSY_LIST` frame with an ordered list of candidate `(frequency_hz, snr_db)`
   tuples (best-first, as measured locally).
4. The partner station receives `QSY_LIST`, scans each candidate locally (hamlib), returns
   to the current frequency, and responds with a `QSY_VOTE` frame containing its own
   `(frequency_hz, snr_db)` assessments.
5. The requesting station picks the channel with the best combined score (sum of both
   stations' SNR readings), sends `QSY_ACK` naming the agreed frequency and a
   switchover time offset (seconds from now, default 5 s).
6. Both stations wait for the switchover time, command hamlib to QSY, and resume the
   session on the new frequency from where it was interrupted.

**Trust-level policy**
- `QSY_REQ` is accepted from any trust level that has `allow_trustlevels` covering the
  peer's current trust classification.
- A station may reject `QSY_REQ` with `QSY_REJECT` if QSY is disabled or hamlib is
  unavailable; the session continues on the original frequency.
- All QSY wire frames are signed with the session Ed25519 key to prevent spoofing.

**New wire frames** (all CR-terminated, carried over the existing B2F data channel)
- `QSY_REQ <token> <n_candidates>` — initiate QSY scan
- `QSY_LIST <token> <freq1>,<snr1> [<freq2>,<snr2> …]` — candidate list from requester
- `QSY_VOTE <token> <freq1>,<snr1> [<freq2>,<snr2> …]` — partner's assessment
- `QSY_ACK <token> <agreed_freq_hz> <switchover_offset_s>` — confirmed channel + timing
- `QSY_REJECT <token> <reason>` — decline (hamlib unavailable, policy, etc.)

**Dependencies**: hamlib integration (new `openpulse-hamlib` crate wrapping `rigctld` TCP
interface), Phase 6.3 mesh layer (for relay-assisted QSY coordination).

### FF-2 — I/Q output mode for direct SDR integration

Allows OpenPulseHF to drive SDR radios (QMX, HermesLite 2, ADALM-Pluto, RTL-SDR TX,
etc.) directly with complex baseband I/Q audio, achieving single-signal generation
comparable to what the QMX does internally: zero unwanted sideband, zero residual carrier,
no reliance on the transceiver's SSB filter for image suppression.

**Motivation**: The current architecture generates a real-valued audio signal at ~1500 Hz
and feeds it to a conventional SSB transceiver.  The radio's SSB filter provides sideband
and carrier suppression, but with finite roll-off.  I/Q output bypasses this entirely:
the modem generates complex baseband (I on left channel, Q on right channel of a stereo
stream), and the SDR upconverts directly to RF with mathematically exact suppression.

**Required changes**

- `ModulationPlugin::modulate_iq()` — new optional trait method returning `(Vec<f32>,
  Vec<f32>)` I/Q baseband samples at a configurable baseband rate (default 0 Hz centre,
  i.e. DC-centred); default implementation wraps the existing real `modulate()` via
  analytic signal (Hilbert transform).
- `AudioBackend::open_iq_output()` — new optional trait method opening a stereo output
  stream where left = I, right = Q; `CpalBackend` implements this when the device
  supports stereo output; `LoopbackBackend` stores interleaved I/Q for testing.
- `ModemEngine::transmit_iq()` — dispatches via `modulate_iq()` and `open_iq_output()`;
  falls back to real `transmit()` if either is unavailable.
- `openpulse-config`: `[audio] iq_output = true` flag; `iq_device` name override.
- Native I/Q implementations in `bpsk-plugin` and `qpsk-plugin` (trivial — the
  modulators already compute I/Q internally; the real output is just `I·cos - Q·sin`).

**What this does NOT change**: the demodulator path, the wire protocol, FEC, framing,
or any higher-layer logic.  It is purely a signal-generation and audio-output concern.

**Dependencies**: Phase 6.2 (CpalBackend is already wired; stereo output needs testing
on real hardware), SDR radio with stereo line-in capability (QMX, HermesLite, etc.).

---

## Dependency ordering summary

The following dependencies constrain the execution sequence:

```
Phase 1.2 (SAR)
    └─> Phase 3.1 (PQ in-band handshake)

Phase 1.3 (Interleaver)
    └─> Phase 2 (HPX profiles can claim burst-error resilience)

Phase 1.5 (Radio interface / PTT)
    └─> Phase 1.6 (Real-device validation)
    └─> Phase 3.5 (Regulatory on-air validation)

Phase 2.1 (ACK taxonomy)
    └─> Phase 2.2 (HPX500/HPX2300 adaptive profiles)

Phase 2.5 (Peer cache)
    └─> Phase 2.6 (Multi-hop relay)

Phase 2.4 (CSMA / DCD)
    └─> Phase 4.3 (KISS broadcast mode)

Phase 7.1 (RigctldController)
    └─> Phase 7.2 (dual-rig / cross-band repeater)
    └─> Phase 7.5 (signed remote rig control)

Phase 7.2 (dual-rig)
    └─> Phase 7.6 (full-duplex, stretch)

Phase 4.2 (EngineEvent)
    └─> Phase 7.3 (daemon control protocol)

Phase 7.3 (control protocol)
    └─> Phase 7.4 (openpulse-panel UI)
```

Items within the same phase may proceed in parallel unless a dependency within the phase is listed above.
