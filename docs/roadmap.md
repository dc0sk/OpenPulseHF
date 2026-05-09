---
project: openpulsehf
doc: docs/roadmap.md
status: living
last_updated: 2026-05-09
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

### 7.1 — Hamlib full CAT control (`openpulse-radio` extension) ✅ Done

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

### 7.2 — Dual-rig support and cross-band repeater ✅ Done

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

### 7.3 — Daemon control protocol ✅ Done

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

### 7.4 — `openpulse-panel` operator UI ✅ Done

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

### 7.5 — Signed remote rig control (over-the-air) ✅ Done

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

### 7.6 — Full-duplex mode ✅ Done (PR #154)

Full-duplex cross-band repeater implemented. `run_full_duplex(stop)` asserts PTT once and
holds it for the session duration; `relay_one_frame()` skips per-frame PTT cycling when
`full_duplex=true`. PTT is guaranteed to release on error or stop. Config field
`[repeater] full_duplex = false` (default). 3 integration tests added.

---



---

## Phase 8 — Waveform Compliance and Pulse-Shaping Expansion

*Unlocked by the bandwidth audit that identified QPSK1000 and 8PSK1000 as exceeding the
2700 Hz HF channel-width limit used by IARU, FCC Part 97, and most national regulations.
Items 8.1 and 8.2 are naming/profile changes; 8.3 is a DSP change that closes the
underlying issue.*

### 8.1 — Rename wideband HPX profiles for non-HF use ✅ Done

The current `hpx2300()` profile reaches SL9=QPSK1000 and SL11=8PSK1000, both of which
occupy ~4000 Hz null-to-null bandwidth with Hann windowing.  These modes are illegal on
HF amateur allocations (2700 Hz hard ceiling) but are legitimate on FM, satellite, and
UHF/VHF links.  The rename makes the operating context explicit.

- Rename `SessionProfile::hpx2300()` → `SessionProfile::hpx_wideband()` in
  `crates/openpulse-core/src/profile.rs`.
- Update all call sites (testbench, testmatrix, integration tests, CLI).
- Add doc comment: *"Wideband profile (≤ 4000 Hz). Legal on FM, satellite, and
  UHF/VHF; exceeds 2700 Hz HF limit at SL9–SL11. Use `hpx_hf()` for HF operation."*
- No wire-protocol change; profile names are local to the session initialisation API.

**Acceptance**: `cargo test --workspace --no-default-features` passes; testbench and
testmatrix compile with the renamed symbol.

---

### 8.2 — HF-compliant capped adaptive profile (`hpx_hf`) ✅ Done

Add a new `SessionProfile::hpx_hf()` profile whose top speed level stays within the
2700 Hz HF channel-width limit.  Bandwidth budget: `4 × Rs ≤ 2700 Hz → Rs ≤ 675 baud`.

| Speed level | Mode | Gross bps | BW (Hann null-to-null) |
|---|---|---|---|
| SL2 (initial) | BPSK31 | 31 bps | 125 Hz |
| SL3 | BPSK63 | 63 bps | 250 Hz |
| SL4 | BPSK250 | 250 bps | 1000 Hz |
| SL5 | QPSK250 | 500 bps | 1000 Hz |
| SL6 (ceiling) | QPSK500 | 1000 bps | 2000 Hz |
| SL7 (ceiling alt) | 8PSK500 | 1500 bps | 2000 Hz |

`hpx_hf()` peaks at SL7 = 8PSK500 (1500 bps gross, 2000 Hz BW).  This is the
HF-legal upper limit with the current Hann-window pulse shaping.

- Implement `SessionProfile::hpx_hf()` in `profile.rs`.
- Add `Tier::Quick` testmatrix cases for the new profile.
- Document in `docs/architecture.md` alongside the existing profile table.
- Update testbench adaptive use-case to expose `HPX-HF` as a named profile choice.

**Acceptance**: adaptive profile integration tests pass for the new profile; testmatrix
quick tier includes at least one HPX-HF × AWGN 20 dB case.

**Dependencies**: none (profile is a pure data change; no new plugins required).

---

### 8.3 — PSK31-style cosine amplitude shaping ✅ Done

PSK31 achieves bandwidth ≈ symbol rate (vs. 4 × Rs for Hann isolated bursts) by applying
a continuous overlapping cosine amplitude envelope across symbol boundaries.  Phase
transitions occur at the zero-crossings of the amplitude, so adjacent symbols fade
smoothly through zero — eliminating the spectral splatter of hard transitions.

Applying this to QPSK1000 reduces its null-to-null bandwidth from ~4000 Hz to ~2000 Hz,
making it legal on HF.  This is a TX-only pulse-shaping change; the existing
Goertzel/IQ-integration receiver does not need modification.

**Implementation**:
- Add `PulseShape` enum to `openpulse-core`: `Hann` (current), `CosineOverlap`.
- `CosineOverlap`: the amplitude envelope for symbol *n* is a half-cosine centred at
  the symbol's midpoint; adjacent symbols share the rising/falling edge so the combined
  envelope is continuous and never drops to zero between symbols (no inter-symbol gap).
- Expose `PulseShape` via `ModulationConfig`; plugins use it in the modulate path;
  default remains `Hann` for backward compatibility.
- Add `"QPSK1000-HF"`, `"8PSK1000-HF"` mode aliases that select `CosineOverlap` with
  the appropriate baud rate, documented as the HF-safe variants of QPSK1000/8PSK1000.
- Update testbench `gross_bps` / `mode_symbol_rate_hz` tables for the new aliases.
- Testmatrix quick tier: add QPSK1000-HF × clean and AWGN 20 dB cases.
- Measure the actual null-to-null bandwidth of QPSK1000-HF in the testbench and confirm
  it falls within 2700 Hz before marking done.

**What this does NOT do**: it does not implement a matched receiver filter or timing
recovery loop.  The receiver integrates over the full symbol period; the overlapping
envelope causes a mild SNR penalty (~1–2 dB) relative to a true matched filter, but
no ISI if the channel delay spread is short relative to the symbol period (acceptable
on HF for 500 baud; marginal at 1000 baud — see FF-3 for the full solution).

**Acceptance**: QPSK1000-HF spectrum in the testbench shows null-to-null BW ≤ 2700 Hz;
loopback round-trip passes on clean channel and AWGN 20 dB.

**Dependencies**: none beyond existing plugin infrastructure.

---

## Phase 9 — Diagnostics and Protocol Intelligence

*Inspired by analysis of FreeDV GUI (scatter plot, SNR trending, TX limiter) and
Mercury/Mercury-Qt (asymmetric rate adaptation, SNR-driven gear-shifting).  All items
are self-contained additions to existing subsystems; none require new crates.*

---

### 9.1 — Constellation / scatter plot in testbench ✅ Done (PR #138)

Add an IQ scatter plot panel to `apps/openpulse-testbench` alongside the existing
spectrum and waterfall views.

**Motivation**: FreeDV's scatter diagram is its most-used demodulation quality indicator.
The `-RRC` modes (FF-3) produce a clean QPSK or 8PSK IQ constellation at the matched-
filter output; the scatter plot makes ISI, phase noise, and timing jitter immediately
visible without needing to interpret BER numbers.

**Implementation**:
- Extend `TapData` with a `recent_symbols: VecDeque<(f32, f32)>` ring buffer (capacity
  ~2000 IQ pairs, ~10 s of symbols at 250 baud).
- Signal thread appends the raw IQ sample at the final decision point for each decoded
  symbol before the threshold comparison.  For `-RRC` modes this is after the matched
  filter and Gardner timing recovery; for Hann/CosineOverlap modes it is the integrated
  IQ pair.
- New `draw_scatter_panel()` in `apps/openpulse-testbench/src/ui.rs`: renders an
  `egui_plot::Points` scatter with a colour gradient fading from yellow (recent) to dark
  blue (oldest), matching FreeDV's visual idiom.  Panel height matches `SPECTRUM_H`
  (170 px); placed to the right of the spectrum plot in each tap column.
- No new dependencies; `egui_plot` is already in the workspace.

**Acceptance**: constellation is visible in the testbench for QPSK500-RRC on a clean
channel showing four tight clusters; on Watterson Good F1 the clusters visibly broaden.

**Dependencies**: Phase 4.5 (testbench), FF-3 (RRC modes produce cleaner constellation).

---

### 9.2 — Asymmetric per-direction rate adaptation ✅ Done (PR #138)

Mercury's most differentiating protocol feature: the A→B and B→A paths each select
their own speed level independently, since SNR is rarely symmetric on HF.

**Current state**: `RateAdapter` is stateless per-direction — it applies ACK feedback to
a single shared `SpeedLevel`.  When node A sends at SL8 but the A→B path is marginal,
NACKs force both directions down even though B→A may be excellent.

**Implementation**:
- Add `tx_level: SpeedLevel` and `rx_level: SpeedLevel` fields to `RateAdapter` (or
  create `BiDirRateAdapter` wrapping two independent `RateAdapter` instances).
- `apply_ack()` updates only `tx_level` (our outgoing path quality as reported by the
  peer); a new `apply_remote_ack(ack: AckType)` updates `rx_level` when the peer's ACK
  includes a reverse-direction quality report.
- Extend `AckFrame` with a 1-byte `reverse_ack: AckType` field (the sender's assessment
  of the *incoming* path quality); backward-compatible via a version nibble already in
  the frame header.
- `ModemEngine::current_adaptive_mode()` returns `(tx_mode, rx_mode)` as a tuple;
  callers that assumed symmetric modes need updating.
- `RigStatus` and `EngineEvent::RateChange` gain an optional `direction` field.

**Acceptance**: integration test: two `ChannelSimHarness` engines with different SNR in
each direction converge to different speed levels per direction within 30 frames.

**Dependencies**: Phase 2.1 (`RateAdapter`, `AckFrame`), Phase 4.2 (`EngineEvent`).

---

### 9.3 — SNR trend plot in testbench ✅ Done (PR #138)

Add a rolling SNR history chart to the testbench stats panel, inspired by FreeDV's
180-second SNR plot.

**Implementation**:
- Add `snr_history: VecDeque<(f64 /*timestamp*/, f32 /*snr_db*/)>` (capacity 1800
  samples = 180 s at 10 Hz update) to `AppStats`.
- Signal thread estimates per-frame SNR as `signal_power / noise_power` where
  `noise_power` is sampled from the `channel.generate_noise()` RMS; emits to stats ring.
- New `draw_snr_plot()` renders an `egui_plot::Line` showing SNR (dBFS) vs. time
  (seconds ago); x-axis inverted so newest is on the right.  Range: −10 to +35 dB.
  Placed below the existing stats bar.
- Stats bar gains a live `SNR: XX.X dB` label updated at the same 10 Hz cadence.

**Acceptance**: SNR plot is visible in the testbench; SNR drops visibly when noise model
is switched from Clean to AWGN 10 dB.

**Dependencies**: Phase 4.5 (testbench).

---

### 9.4 — SNR as secondary rate adapter input ✅ Done (PR #138)

Supplement ACK-only rate decisions with a raw SNR estimate, closing the feedback loop
faster on rapidly degrading channels (Mercury's "hybrid SNR + delivery-feedback").

**Current state**: `RateAdapter::apply_ack()` only acts on `AckType`; it cannot react
until a full frame has been sent and acknowledged.  On a channel that drops 3 dB in a
single propagation skip, the engine sends several frames at the wrong rate before the
NACKs arrive.

**Implementation**:
- Add `apply_snr_hint(snr_db: f32)` to `RateAdapter`; called after every receive frame
  (SNR derived from the modem's `estimate_afc_hz` signal-strength side-channel or a
  separate RMS estimator in `ModemEngine`).
- If `snr_db` drops below `profile.snr_floor_for_level(current_level)` (a per-level
  SNR threshold table added to `SessionProfile`), immediately step down one level without
  waiting for a NACK.  If `snr_db` rises above `snr_ceiling_for_level`, candidate an
  upgrade (confirmed only after the next ACK-UP).
- Thresholds are conservative (3 dB headroom above the Eb/N₀ required for 10⁻³ BER at
  each modulation order) so the SNR hint only acts in unambiguous cases; normal ACK
  feedback remains the primary driver.
- `RateChange` engine event gains an optional `trigger: RateTrigger` field
  (`AckUp`, `AckDown`, `NackDecrement`, `ChirpFallback`, `SnrFloor`, `SnrCeiling`).

**Acceptance**: integration test: engine running at SL8 on a clean channel; inject a
large noise burst (SNR drops below SL8 floor); assert engine steps down within 3 frames
before any NACK has been processed.

**Dependencies**: Phase 2.1 (`RateAdapter`), Phase 9.2 (asymmetric adaptation; shares
the `SessionProfile` SNR threshold table).

---

### 9.5 — Broadcast / beacon mode alongside ARQ ✅ Done (PR #139)

Mercury runs a broadcast mode in parallel to its ARQ sessions, enabling one-to-many
unacknowledged transmissions (beacons, network announcements, position reports).

**Implementation**:
- New `WireMsgType::BroadcastFrame` (msg_type 0x0A): fixed 4-byte header
  `(callsign_hash: u32, seq: u16, ttl: u8, flags: u8)` followed by variable payload.
  No ACK expected; no session state required.
- `ModemEngine::broadcast(payload: &[u8])` — encodes a `BroadcastFrame`, skips CSMA
  persistence check (broadcasts are short; sender takes responsibility), and transmits
  immediately.
- `MeshDaemon` re-broadcasts received `BroadcastFrame`s with `ttl -= 1` until `ttl == 0`
  (store-and-forward propagation limited to TTL hops).
- `openpulse-cli broadcast --payload <hex|text> --ttl <n>` subcommand.
- Beacon mode: `openpulse-cli beacon --interval 600s --callsign KX0ABC` sends a minimal
  `BroadcastFrame` every interval (station ID compliance for long sessions).

**Acceptance**: two `ChannelSimHarness` nodes: node A broadcasts; node B receives and
emits `EngineEvent::FrameRx`; relay node C re-broadcasts with `ttl - 1`.

**Dependencies**: Phase 2.4 (CSMA/DCD), Phase 6.3 (mesh daemon for multi-hop relay).

---

## Phase 9 dependency summary

```
Phase 4.5 (testbench)
    └─> Phase 9.1 (scatter plot)
    └─> Phase 9.3 (SNR trend plot)

FF-3 (RRC modes)
    └─> Phase 9.1 (RRC produces clean constellation; scatter plot is most useful)

Phase 2.1 (RateAdapter / AckFrame)
    └─> Phase 9.2 (asymmetric per-direction adaptation)
    └─> Phase 9.4 (SNR secondary input)

Phase 9.2 (asymmetric adaptation — adds SNR threshold table)
    └─> Phase 9.4 (reuses SNR threshold table)

Phase 4.2 (EngineEvent)
    └─> Phase 9.2 (RateChange event gains direction field)
    └─> Phase 9.4 (RateChange event gains trigger field)

Phase 2.4 (CSMA/DCD)
    └─> Phase 9.5 (broadcast mode bypasses CSMA)

Phase 6.3 (mesh daemon)
    └─> Phase 9.5 (TTL-limited re-broadcast)
```

---

## Far-future items

Features deliberately deferred beyond Phase 9.  Each item requires significant design
work, hardware availability, or explicit operator configuration that is not yet in scope.

### FF-3 — Root-raised-cosine matched filtering ✅ Done (PR #158)

RRC reduces the occupied bandwidth to `(1 + α) × Rs` Hz.  At α = 0.35 this is 1350 Hz
for 1000 baud — well within 2700 Hz and comparable to VARA 500 Hz mode spectral
efficiency.

**What was delivered**:
- TX: RRC FIR TX filter (span = 8 symbols, α = 0.35) replacing cosine/Hann amplitude
  shaping for all `-RRC` mode variants.
- RX: matched RRC filter + Gardner timing error detector for adaptive symbol timing
  recovery across BPSK-RRC, QPSK-RRC, and 8PSK-RRC.  `GardnerDetector::pre_arm()`
  bridges brute-force preamble acquisition to the adaptive tracking loop.
- Costas PLL (decision-directed, psk_order=2 for QPSK, psk_order=3 for 8PSK) for
  coherent carrier recovery on QPSK-RRC and 8PSK-RRC.  BPSK keeps differential
  detection (NRZI-encoded; coherent detection would break the protocol).
- Integration tests: `rrc_channel_loopback.rs` — 5 tests covering BPSK250-RRC,
  QPSK500-RRC, and 8PSK500-RRC on clean channel and AWGN 20 dB.

**Note**: adaptive equalizer (LMS/DFE) deferred — at 1000 baud on HF the symbol period
(1 ms) is comparable to multipath delay spread, but FEC already provides adequate
protection on Good F1 channels.  Equalizer remains a future option.

---

### FF-4 — OFDM wideband HF profile ✅ Done (PR #167)

OFDM multi-carrier modes for HF use, with LS channel estimation and ZF equalization to handle
frequency-selective ionospheric fading.  Two sub-profiles are delivered:

| Mode | SCs | BW | Gross bps | Session SL |
|---|---|---|---|---|
| OFDM16 | 16 data + 4 pilot, SCs 38–57 | ≈ 625 Hz | ≈ 889 | SL5 |
| OFDM52 | 52 data + 13 pilot, SCs 16–80 | ≈ 2031 Hz | ≈ 2889 | SL6 |

Both modes: FFT=256, CP=32, QPSK per-subcarrier, centre 1500 Hz, iterative PAPR clipping
(target 6 dB), 2-byte LE length prefix.

**What was delivered**:
- `plugins/ofdm/` — new `ofdm-plugin` crate: `OfdmPlugin`, modulate, demodulate, params, channel equalization modules; 15 tests
- `crates/openpulse-core/src/profile.rs` — `hpx_ofdm_hf()` session profile (SL5=OFDM16, SL6=OFDM52)
- `crates/openpulse-core/tests/session_profile.rs` — 3 new profile tests
- `apps/openpulse-testbench/` — OFDM16 and OFDM52 modes wired into the testbench GUI with correct bandwidth markers and gross bps display
- `docs/backlog-fec-improvements.md` — FEC backlog items (BL-FEC-1 through BL-FEC-6) from RS research session

**Previous research** (PR #135, `crates/openpulse-modem/src/ofdm_sim.rs`):
The research showed PAPR reduction via iterative clipping degrades BER.  The production
plugin resolves this by adding per-subcarrier LS+ZF equalization, which compensates for
the amplitude distortion introduced by clipping.

---

### FF-5 — UHF/VHF wideband modes ✅ Done (PR #159)

On UHF (430 MHz) and VHF (144 MHz), the 2700 Hz bandwidth restriction does not apply.
FM voice allocations are typically 12.5–25 kHz wide.  FF-5 added two tiers of higher-
baud modes and two new `SessionProfile` constructors targeting PMR/LMR narrowband channels.

**What was delivered**:

Standard tier (8 kHz audio, fits within 12.5 kHz channel, ~2700 Hz occupied BW with α=0.35):

| Mode | Baud | n (sps) | Notes |
|---|---|---|---|
| QPSK2000 | 2000 | 4 | Hann crossfade |
| QPSK2000-RRC | 2000 | 4 | RRC + Gardner + Costas |
| 8PSK2000 | 2000 | 4 | CosineOverlap + fc=2×baud required at n=4 |
| 8PSK2000-RRC | 2000 | 4 | RRC + Gardner + Costas |

HD tier (48 kHz audio required, fills 12.5 kHz channel, ~13 kHz BW):

| Mode | Baud | n (sps) | Notes |
|---|---|---|---|
| QPSK9600 | 9600 | 5 | Hann; fc must be ≥ baud (use 12000 Hz) |
| QPSK9600-RRC | 9600 | 5 | RRC + Gardner + Costas |
| 8PSK9600 | 9600 | 5 | Hann; fc = 12000 Hz |
| 8PSK9600-RRC | 9600 | 5 | RRC + Gardner + Costas |

New session profiles:
- `hpx_narrowband()`: SL8=QPSK500, SL9=QPSK1000, SL10=QPSK2000-RRC, SL11=8PSK2000-RRC
- `hpx_narrowband_hd()`: SL8=QPSK9600-RRC, SL9=8PSK9600-RRC

**Note on 8PSK2000 non-RRC**: at n=4 the Hann crossfade ISI exceeds 8PSK's 22.5°
decision margins (QPSK passes at n=4 because its margins are 45°).  The mode requires
`CosineOverlap` (sin² shaping, zeros at boundaries) and fc = integer multiple of baud
for adequate I/Q orthogonality.  The `-RRC` variant has no such constraint.

**Note on 64QAM at 3000–12500 baud**: original FF-5 scope included 64QAM; deferred
pending FF-4 OFDM research and FF-3 full-equalizer work.  SL12–SL20 SpeedLevel
extensions are not yet implemented.

---

### FF-1 — QSY frequency-agility protocol ✅ Done (PR #140 / PR #141)

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

### FF-2 — I/Q output mode for direct SDR integration ✅ Done (PR #150)

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

### FF-6 — Binary WebSocket spectrum channel ✅ Done (PR #157)

Extend the daemon control protocol (Phase 7.3) with a binary frame channel for spectrum
data, eliminating JSON serialisation overhead on high-frequency FFT updates.

**Motivation**: Mercury-Qt uses a hybrid protocol — JSON for control commands, binary
frames (magic header + float32 array) for spectrum data.  At 20 Hz waterfall updates
with 512 bins, JSON encoding wastes ~8 KB/s per client vs. ~4 KB/s for raw float32.

**Implementation**:
- Binary frame format: `[0x4F 0x50 0x53 0x50] (magic "OPSP") | fft_size: u16 LE |
  sample_rate: u32 LE | bins: [f32 LE × fft_size]`.
- Control port (Phase 7.3) gains a `Content-Type: application/octet-stream` upgrade
  path: client sends `{"cmd": "subscribe_spectrum", "fps": 20}` → server begins
  interleaving binary frames into the connection alongside NDJSON events.
- `openpulse-panel` (Phase 7.4) subscribes at startup and feeds the float32 array
  directly into its waterfall texture, bypassing JSON parse and float conversion.

**Acceptance**: panel waterfall updates at 20 fps with < 5 ms latency on loopback;
existing NDJSON event consumers are unaffected.

**Dependencies**: Phase 7.3 (control protocol), Phase 7.4 (panel consumes spectrum).

---

### FF-7 — Tanh TX limiter ✅ Done (PR #149)

Apply a soft-limiting (tanh) compressor on the modulated audio output immediately before
the audio backend, preventing ADC clipping and reducing PA non-linearity on hot signals.

**Motivation**: FreeDV applies tanh limiting for full-duplex mode, but it is useful
generally: at the top of the HPX rate ladder (8PSK1000-RRC), the RRC filter produces
occasional amplitude peaks ~3 dB above RMS.  A soft limiter with a threshold at +2 dB
above RMS absorbs these without audible distortion, reducing the PA back-off requirement.

**Implementation**:
- `pub fn tanh_limit(samples: &mut [f32], threshold: f32)` in `openpulse-audio` (or
  `openpulse-core`): apply `threshold * tanh(s / threshold)` per sample.
- `ModemEngine::transmit()` applies `tanh_limit` with a configurable threshold
  (default `1.5 × RMS`; `[audio] tx_limiter_threshold` config key; 0.0 = disabled).
- No changes to any plugin or demodulator.

**Acceptance**: peak amplitude of QPSK1000-RRC frame after limiting does not exceed
`threshold`; BER on clean loopback is unchanged.

**Dependencies**: Phase 5.6 (CpalBackend audio path), FF-3 (RRC modes benefit most).

---

### FF-8 — Per-band TX attenuation memory ✅ Done (PR #148)

Store the operator's last TX gain setting per amateur band segment and restore it
automatically when the rig tunes to that band.

**Motivation**: FreeDV stores per-band TX attenuation because operators use different
power levels on different bands (40 m full power, 30 m QRP-only, etc.).  Manual
re-adjustment on every band change is error-prone.

**Implementation**:
- `[tx_levels]` TOML section: `"40m" = -3.0`, `"30m" = -10.0`, etc.; band names follow
  IARU Region 1 edge frequencies; `"default" = 0.0` for unrecognised frequencies.
- After each `RigctldController::get_frequency()` call (Phase 7.1), map the result to
  a band segment; scale TX samples by `10^(attenuation_db / 20)`.
- `openpulse-panel` toolbar gains a `TX atten: X.X dB` slider that writes to the in-
  memory value and persists to `[tx_levels]` on change.

**Acceptance**: operator sets -6 dB on 40 m; tunes to 20 m (0 dB); tunes back to 40 m
— attenuation returns to -6 dB without operator action; persists across restart.

**Dependencies**: Phase 7.1 (RigctldController for frequency readback), Phase 7.4 (panel).

---

### FF-9 — Reactor pattern for HPX state machine ✅ Done (PR #151)

Replace the monolithic `HpxSession` state machine with an event-driven reactor pattern,
decoupling protocol events from state transitions.

**Motivation**: Mercury replaced its monolithic ARQ state machine with a modular reactor
because concurrent protocol states became unmanageable in a single match block.
OpenPulseHF's `HpxSession` is straightforward today, but as relay (Phase 6.3),
asymmetric rate adaptation (Phase 9.2), broadcast (Phase 9.5), and QSY (FF-1) are all
wired in, the number of concurrent state combinations will grow.

**What changes**:
- `HpxSession` becomes `HpxReactor` holding a `HashMap<SessionId, SessionState>` and a
  bounded event queue.
- `HpxReactor::dispatch(event: HpxEvent)` routes events to per-state handler functions;
  no direct state mutation outside handlers.
- Existing `HpxState` and `HpxEvent` enums are unchanged; HPX conformance tests serve as
  the acceptance gate.
- Wire protocol and all wire frame types are unaffected.

**Timing**: defer until at least Phase 9.5 (broadcast) is implemented; only undertake
the refactor if the state machine branch count exceeds ~50 arms.

**Acceptance**: all existing `hpx_conformance_integration` tests pass unchanged.

**Dependencies**: Phase 9.5, FF-1 — implement after at least two of these add concurrent
states to `HpxSession`.

---

### FF-10 — zstd dictionary compression ✅ Done (PR #156)

Added `CompressionAlgorithm::Zstd` backed by a pre-trained shared dictionary built from
typical HPX/Winlink message content (structured headers, callsigns, common phrases).

- `zstd` crate dependency added to `openpulse-core`; `train_from_buffer` API used by `openpulse-dict-trainer` offline tool.
- Dictionary artifact embedded at compile time; falls back to no-dictionary if absent.
- `ConReq`/`ConAck` negotiation extended: `Zstd` variant carries a 4-byte dictionary ID so both sides confirm they are using the same dictionary version before enabling it.
- `CompressionAlgorithm::Zstd` in `openpulse-core/src/compression.rs`; `compress_if_smaller` selects Zstd when it beats LZ4 and the plain payload.
- `openpulse-dict-trainer` binary collects loopback payloads as a training corpus.

**Dependencies**: Phase 2.7 (compression layer) ✅.

---

### FF-11 — Authenticated voice shim for FreeDV ✅ Done (PR #162)

FreeDV transmits codec2-compressed voice digitally, but provides no cryptographic
guarantee that a received frame was actually produced by the claimed operator.  Replay
attacks and synthetic voice injection are undetectable at the FreeDV layer.

**Implemented** in `crates/openpulse-freedv-auth`:

- **Authentication model**: station-identity signing — `{callsign, timestamp_utc,
  session_nonce, freq_hz, mode, pubkey}` as canonical JSON, signed with Ed25519.
  Satisfies FCC Part 97 ID without per-frame audio signing overhead.
- **Interface**: FreeDV Qt-GUI UDP data port (`127.0.0.1:10001`).  Pure-Rust, no
  C FFI, fully compatible with `--no-default-features`.
- **`AuthBeacon`**: Ed25519-signed, length-prefixed JSON wire format (≈144 bytes).
  `sign()`, `verify()`, `encode()`, `decode()` API.
- **`BeaconScheduler`**: sends a beacon immediately then every configured interval.
- **`FreeDvDataPort`**: async tokio UDP wrapper for beacon injection and receive.
- **`TrustVerdict`**: `Verified / Unverified / Invalid` with Unix-socket server for
  companion UI polling.
- `sign_bytes` / `verify_bytes` primitives exposed from `openpulse-core/src/signing.rs`.
- 13 tests (8 unit + 5 integration) all passing.

**Dependencies**: Phase 2.3 (Ed25519 signing) ✅, Phase 3.1 (optional PQ hybrid) ✅.

---

### FF-12 — SC-FDMA waveform ✅ Done (PR #175)

Analysis of five 5G-era multi-carrier schemes (FBMC, UFMC, GFDM, SC-FDMA, OFDMA) against HF
radio constraints was conducted 2026-05-09.  See `docs/backlog-waveforms.md` for the full
analysis.

**Conclusion**: SC-FDMA (DFT-spread OFDM) is the only scheme with a clear HF benefit:
3–4 dB lower PAPR than OFDM without iterative clipping, with identical one-tap ZF channel
equalization. All other schemes either add receiver complexity with no HF gain (FBMC, UFMC,
GFDM) or are inapplicable to peer-to-peer links (OFDMA).

The OOB distortion gate was removed: localized SC-FDMA (52/256 subcarriers) achieves
~8–11 dB PAPR naturally without hard clipping. The benefit over OFDM is the absence of
clipping-induced OOB spectral regrowth, not that PAPR is numerically below any fixed target.

**Delivered**: `plugins/scfdma/` — `SCFDMA16` (625 Hz BW, ~889 bps) and `SCFDMA52`
(2031 Hz BW, ~2889 bps) modes; DFT-spread TX, IDFT de-spread RX, shared LS/ZF channel
estimation with OFDM; registered in CLI and testbench.

**Acceptance criterion**: SC-FDMA52 loopback at Watterson F1 SNR=15 dB achieves equal or
better BER than OFDM52, with ≥ 3 dB lower measured PAPR.

**Dependencies**: FF-4 (OFDM plugin) ✅.

---

### FF-13 — Generic serial CAT for rigs not in hamlib ✅ Done (PR #173)

Current rig control is fully dependent on hamlib (`rigctld`) for frequency/mode/level
commands.  Rigs that are unsupported or only partially supported by hamlib cannot use
CAT-dependent features (QSY, per-band TX attenuation, remote rig control) and must fall
back to `SerialRtsDtrPtt` (PTT only) or `VoxPtt`.

**Goal**: add a `GenericSerialCat` backend in `crates/openpulse-radio/` that lets users
define a rig's CAT protocol via a small TOML script (command templates + response parsers),
without requiring a hamlib driver.

**Scope**:
- `crates/openpulse-radio/src/generic_cat.rs`: `GenericSerialCat` implementing `PttController`
  and a new `CatController` trait (subset: `set_frequency`, `get_frequency`, `set_mode`, PTT).
- TOML rig definition format: one file per rig model, listing baud rate, byte framing, and
  per-command byte sequences with optional regex response parser.
- `crates/openpulse-config/`: `[rig]` section gains `backend = "generic"` and `rig_file = "path/to/rig.toml"`.
- Ship at least two reference rig files: Icom CI-V (e.g. IC-7300) and Yaesu FT-8x7 binary CAT.

**Limitations accepted at this stage**: features that require real-time level polling
(S-meter-driven SNR, ALC feedback, power-out reporting) will be unavailable or return
`RadioError::Unsupported` unless the rig definition provides the relevant command.  The
QSY and per-band-attenuation features will work as long as `set_frequency` is defined.

**Acceptance criterion**: IC-7300 or FT-817 loopback test (using a mock serial port) sets
frequency, mode, and PTT via `GenericSerialCat`; `RigctldController`-dependent paths
degrade gracefully to `RadioError::Unsupported` when the rig file omits the command.

**Dependencies**: Phase 1.5 (`openpulse-radio` crate) ✅, Phase 5.3 (TOML config) ✅.

---

## BL-FEC series — FEC codec improvements

Incremental FEC improvements tracked in [`docs/backlog-fec-improvements.md`](backlog-fec-improvements.md).

| Item | Description | Status |
|---|---|---|
| BL-FEC-1 | Concatenated Conv+RS session mode | ✅ Done (PR #169) |
| BL-FEC-2 | RS t=32 strong codec (RS(255,191), 25% overhead) | ✅ Done (PR #171) |
| BL-FEC-3 | Short-block RS for ACK/control frames (5 B → 13 B) | ✅ Done (PR #170) |
| BL-FEC-4 | Memory-ARQ soft combining (element-wise sample averaging) | ✅ Done (PR #171) |
| BL-FEC-5 | Soft-decision K=7 Viterbi | In progress — bl-fec-5-soft-viterbi |
| BL-FEC-6 | Turbo / LDPC codes — `IterativeDecoder` trait + stub | In progress — bl-fec-6-ldpc-prep |

---

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

Phase 8.1 (rename hpx_wideband)
    └─> Phase 8.2 (hpx_hf profile — references renamed symbol)
    └─> Phase 8.3 (cosine pulse shaping — QPSK1000-HF uses it)

Phase 8.3 (cosine pulse shaping)
    └─> FF-3 (RRC builds on pulse-shaping infrastructure)
    └─> FF-4 (OFDM research uses channel simulation tooling)

FF-3 (RRC matched filtering)
    └─> FF-5 (UHF/VHF ultra-high-speed modes require RRC)

FF-4 (OFDM research)
    └─> FF-5 (OFDM may be preferred at very wide channels)

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
