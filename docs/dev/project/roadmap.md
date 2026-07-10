---
project: openpulsehf
doc: docs/dev/project/roadmap.md
status: living
last_updated: 2026-07-10
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

Most Phase 3 items shipped. Remaining: 3.5 on-air validation.

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

Remaining on-air items (**in active execution as of 2026-06** — see [onair-status.md](../onair-status.md)):
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
- Compression: Gzip (type D) via `flate2`; LZHUF LH5 (type C) implemented via `oxiarc-lzhuf`, including OpenPulse and Winlink-compatible length-prefix helpers.
- `B2fSession` state machine: ISS (Information Sending Station) and IRS (Information Receiving Station) roles; Handshake → ProposalExchange → Transfer → Done states; handles ISS-immediate-proposal pattern.
- Pat-client ARDOP compatibility: added GRIDSQUARE, ARQBW, ARQTIMEOUT, CWID, SENDID, PING commands to `openpulse-ardop`; 3 new integration tests (11 total).
- 9 integration tests in `crates/openpulse-b2f/tests/b2f_integration.rs`.

---

## Phase 5 — Integration and Release Readiness (Completed)

All Phase 5 items shipped. On-air regulatory validation (Phase 5.5-reg) is
**in active execution as of 2026-06** (see [onair-status.md](../onair-status.md)); no
hardware gate blocks further development.

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

### 5.5-reg — On-air regulatory validation *(in active execution, 2026-06 — see [onair-status.md](../onair-status.md))*
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

**New crate: `openpulse-repeater`** (or module in `openpulse-mesh`):
- `CrossBandRepeater`: holds two `RigctldController` instances and two `ModemEngine`
  instances (one per audio device); configurable from `[repeater]` TOML section.
- PTT sequencing: `rig_b.ptt_on()` before TX, `rig_b.ptt_off()` after; configurable
last_updated: 2026-05-17
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

## Phase 10 — Receiver-led OTA adaptation and twin-station validation (2026-06)

Over-the-air adaptive rate-stepping wired end to end through the daemon, plus a
full-stack, hardware-free validation rig and its real-radio scenario. This closes
the long-deferred "adaptive rate-stepping over the air (RX lockstep)" item.

### 10.1 — Receiver-led OTA rate-stepping ✅ Done
- `openpulse-core::ota_rate::OtaRateController` — per-direction lockstep: the data
  receiver leads, shipping an **absolute** `recommended_level` in the ACK; the
  sender follows. Lockstep invariant: the receiver advances at most one mapped
  step above the highest level it actually decoded, so a lost ACK never desyncs
  (PRs #489, #490).
- `AckFrame.recommended_level` packed into free bits; `SpeedLevel::as_u8/from_u8/name/from_name`.
- Engine: `start_ota_session`/`respond_arq_ota`/`apply_ota_ack`/`transmit_arq_ota`;
  AFC-isolated 2-mode candidate fallback; MODCOD profile `hpx_modcod` with per-level FEC.
- Operator controls (#492): level bounds, lock/unlock, A2 backlog gate, A3 re-upgrade hold.
- M2M4 RX SNR estimator (#494) + silence VAD gating (#499) replacing the weak LLR proxy.

### 10.2 — Daemon OTA wiring ✅ Done
- Config `[modem] ota_*`; control protocol `StartOtaSession`/`StopOtaSession`/
  `OtaSetLevelBounds`/`OtaLockLevel`/`OtaUnlock`/`OtaSetHysteresis` + `OtaStatus`
  events; CLI `openpulse daemon ota-*`; panel toolbar OTA line + lock (#495–#500, #497).
- RX tick routes to `poll_ota_rx` (idle-gated, PTT-sequenced ACK) when a session
  is active; `SendMessage` drives `transmit_arq_ota_within` with the real-radio
  **ISS PTT turnaround** (key TX → release → listen for ACK) so the ladder steps
  under traffic (#501, #512, #513).
- Aggressiveness preset (`conservative`/`balanced`/`aggressive`) bundling the
  A2/A3 hysteresis gates into one operator control — config `[modem]
  ota_aggressiveness`, `OtaSetAggressiveness` command, CLI (#517).

### 10.3 — Twin-station validation rig ✅ Done
- `server::run` extracted from the binary with an injectable audio backend (#507).
- `openpulse_daemon::twin::spawn_bridged_pair` — two REAL daemons bridged through a
  channel model in one process; `LoopbackBackend::new_split()` (no self-loop);
  headless deterministic test + `examples/twin_station.rs` (#508).
- Real-audio substrate: `[audio] device` + `ModemEngine::set_default_device`;
  `scripts/setup-twin-loopback.sh` + `run-twin-station-audio.sh` (snd-aloop) (#509).
- `scripts/twin-traffic.sh` random-data traffic generator (#510).
- Spectrum-tap fix: the daemon now feeds real audio to the spectrum broadcast, so
  panels show live spectrum/waterfall instead of an FFT of silence (#511).
- `apps/openpulse-twinview` — combined both-directions viewer over two daemons (#514).

### 10.4 — Engine AGC (data-aided) ✅ Done
- 64QAM corner-preamble data-aided AGC for input-level robustness (no-op at unity
  level); the streaming `dsp::Agc` PSK-ladder rollout remains a follow-on (#503).

### 10.5 — On-air twin-OTA scenario ✅ Done (operator-run)
- `docs/config/onair-twin-ota.example.sh` + `scripts/run-onair-twin-ota.sh` +
  runbook `docs/dev/onair-twin-ota.md`: two real daemons over rigctld CAT+PTT and
  cpal, OTA rate-stepping over the air, observed in `openpulse-twinview` (#515).
  On-air execution is the operator's step (no radios/cpal in CI).

### 10.7 — Live-audio operability hardening ✅ Done
- Burst capture: the daemon RX tick now accumulates audio across ticks while a
  carrier is present and decodes the whole burst on carrier-drop
  (`ModemEngine::capture_burst` + `decode_burst`/`ota_decode_burst`), so a frame
  spanning many tick windows on a streaming cpal backend is assembled correctly
  (the in-process bridge delivered frames atomically and hid this) (#518).
- DCD squelch exposed: `[modem] dcd_squelch` default + `[modem.dcd_squelch_bands]`
  per-band map, resolved on retune via `bandplan::band_label_for_hz`, plus a
  `SetDcdSquelch` runtime command/CLI — so a band's noise floor doesn't read as a
  permanent carrier and stall the burst flush (#519).

### 10.8 — CE-SSB TX envelope conditioning ✅ Done
- Controlled-Envelope SSB conditioner (Hershberger W9GR, QEX 2014; public domain):
  `openpulse_dsp::cessb` look-ahead peak-stretcher raises average TX power at fixed
  PEP without overshoot. Prototyped + measured in channel-sim (#521):
  **OFDM52 +1.6/+2.7/+3.8 dB at 2.5/2.0/1.5×rms clip, ZERO BER cost**; ~0 dB and real
  EVM on single-carrier 64QAM; 0 dB on near-constant-envelope BPSK.
- Wired as an optional, **default-on, per-mode** TX conditioner gated to high-PAPR
  multicarrier (`cessb_benefits`: OFDM*/SCFDMA* only; no-op elsewhere): applied in
  `stage_emit_output` after attenuation, before the tanh limiter, with a peak-restore
  rescale. `[modem] cessb_enabled` config, `ControlCommand::SetCessb` + `openpulse
  daemon set-cessb` CLI, panel "CE-SSB: ON/OFF" toolbar toggle (#522, #523).
- OFDM-HOM gate confirmed: the dense-subcarrier variants
  `OFDM52-{8PSK,16QAM,32QAM,64QAM}` stay high-PAPR multicarrier, so the
  average-power gain holds (unlike single-carrier QAM, ~0 dB); they add a small EVM
  cost pure-QPSK OFDM does not (raw BER ≈ 0.0007–0.0039 at the 2.0×rms operating
  point) that stays FEC-absorbable. `cessb_benefits` enabling all OFDM*/SCFDMA* is
  therefore correct as-is — no narrowing. Locked by `cessb_benefits_hold_on_ofdm_hom`
  in `tests/cessb_power_evm.rs`.
- Verification note: virtual/channel-sim covers decode integrity. The average-power
  gain at fixed PEP is a PA-domain effect no audio loopback can show — **now confirmed
  on real RF** (rpi53 + FT-991A, 20 W via a 20 dB/20 W attenuator on 144.6 MHz):
  interleaved OFF/ON A/B of gapless OFDM52 measured **+1.18 dB** average-power gain on
  the rig's PO meter, matching the channel-sim prediction (+1.2 dB for this payload at
  the 2.0×rms operating point), with ALC unchanged between ON/OFF (gain at equal peak
  = the CE-SSB signature).
- Spectral regrowth — **conditioner DSP part DONE in software** (`cessb_acpr_spectral_regrowth`
  in `tests/cessb_power_evm.rs`): Welch PSD of CE-SSB on vs off OFDM52 shows the
  conditioner is spectrally benign at the 2.0×rms operating ratio (out-of-band
  regrowth −0.46 dB, no 99% OBW widening, shoulder −0.85 dB), self-validated against a
  naive hard clip that splatters clearly more (+5 dB OOB, OBW 2133→3094 Hz).
- PA-compression spectral mask — **DONE on real RF** (SDRplay RSP2pro, off-air capture
  of the FT-991A's 144.6 MHz OFDM52, CE-SSB OFF/ON A/B, 3 bursts each at 20 W).
  **CE-SSB ON does not worsen the spectral mask**: ACPR lower Δ−1.0 dB (cleaner), upper
  Δ−0.1 dB, 99% OBW Δ−46 Hz — all negligible/benign. Front-end linearity validated
  across a 32 dB level change (shoulders-dBc constant ⇒ real TX, not SDR IMD). With the
  +1.18 dB average-power gain, this confirms on QPSK OFDM **CE-SSB raises average power
  without increasing PA splatter** — the ideal outcome. (Absolute ACPR includes the OFDM
  signal's own skirts; the meaningful result is the OFF-vs-ON delta. Measurement floor
  −34 dBc.)
- OFDM-HOM on-air nuance — the dense variants `OFDM52-{8PSK,16QAM,32QAM,64QAM}` DO show
  on-air regrowth with CE-SSB ON (ACPR-lower Δ +1.2/+2.8/+4.2/+0.7 dB), but it is **PA
  compression, not the conditioner** (its digital output is clean on all HOM modes,
  ±0.7 dB). CE-SSB raises the peaky HOM signals' average power a lot (+2.4…+7 dB), so at
  *full* drive the PA overdrives. A 32QAM-ON drive sweep: ACPR −21.7 dBc @ALC 61 (full)
  → **−29.1 dBc @ALC ~31 (~35% drive)**, with output power essentially unchanged
  (PO 36→32) — i.e. the extra drive above proper level is pure splatter, no power.
  **Operator guidance: with CE-SSB on dense HOM, set audio drive for moderate ALC
  (~30–40, standard data-mode practice); don't slam the ALC.** Then CE-SSB on HOM is
  clean *and* keeps its power.

### 10.6 — Remaining follow-ons (deferred)
- Dual-station hardware validation of the OTA ladder (rpi51↔rpi52) per the runbooks.
- Streaming-`Agc` rollout to the PSK ladder with active-span gating — ✅ **Done (2026-06-28)**.
  Opt-in receiver AGC at the single `route_audio_stage(InputCapture)` seam (default off, after the
  notch), active-span gated via `Agc::lock`/`unlock` on the DCD squelch so leading silence can't
  ramp the gain. Engine API `enable_agc`/`disable_agc`/`configure_agc`/`agc_gain_db` +
  `agc_blocks_processed` tripwire; daemon `SetAgc` control + CLI `daemon set-agc`. Tests:
  `crates/openpulse-modem/tests/agc_loopback.rs` (4). Panel toggle parity ✅ **Done (2026-06-29)** —
  `apps/openpulse-panel/src/app.rs` adds an `AGC: ON/OFF` button in the controls column (mirrors the
  Notch/CE-SSB toggles, default off) that sends `ControlCommand::SetAgc`.
- (CE-SSB on-air validation is now complete — average-power gain *and* spectral mask
  both confirmed on real RF; see §10.8.)

---

## Phase 11 — Signal-chain audit hardening (Fable full-chain audit, 2026-07) ✅ Done

A five-stream model-driven audit of the whole signal chain (single-carrier PSK / multicarrier /
FEC+ladder / DSP primitives / CE-SSB + measurement), each finding **independently reproduced** before
any fix, in the repo's ablation-first, measure-noiselessly-first culture. Seven Tier-1 correctness bugs,
the measurement-layer biases that had been flattering past conclusions, and a ranked improvement backlog —
all shipped. Per-change chains are in `docs/dev/project/traceability.md`; the audit notes live in the
session memory. Ordered execution: Tier-1 bugs → OTA-ladder re-seat → measurement fidelity → trivial wins.

### 11.1 — Tier-1 correctness bugs ✅ Done
- **DFE feedback-update sign inverted** — `LmsEqualizer` was anti-adaptive; flipped the sign (PR #697).
- **HARQ soft-LLR combining wired nowhere** — combining now engages in the daemon OTA path across
  retransmissions (`decode_combined_llrs`, additive design, superset of standalone) (PR #702).
- **M2M4 SNR estimator waveform-blind, capping the ladder ~SL8** — replaced by per-plugin symbol-domain
  SNR: PSK (PR #703), then OFDM + SC-FDMA (PR #705). M2M4 saturates ~15 dB; the plugins track to the EVM
  floor, so the OTA ladder now climbs into the dense rungs (to SL17).
- **CE-SSB crushed low-entropy OFDM frames on a clean channel** — TX bit-stream whitening scrambler (PR #698).
- **AGC/DCD seam-ordering deadlock** — carrier-detect moved before the AGC so its boost can't wedge the
  squelch (PR #699).
- **No-AGC × Costas coupling** — level-normalise symbols before PSK carrier recovery (PR #700).
- **SC-FDMA delay-cliff** — the wideband rungs cliffed at a ~10-sample delay *inside* the 32-sample CP.
  Root cause was the **sync back-off** (`SYNC_EARLY_BIAS`), not the CE reach the audit had guessed;
  raising it 8→16 extends the reach to a 2 ms (CCIR-poor) spread. A CE-basis widen was tried and reverted
  (it over-fit pilot noise and broke `llr_reliability`); `deramp_timing`'s centroid re-centring makes the
  existing ±10 basis sufficient (PR #717).

### 11.2 — OTA-ladder re-seat (OFDM beats SC-FDMA on selective fading) ✅ Done
- Bake-off confirmed OFDM ≫ SC-FDMA on selective fading at matched rate/geometry (moderate_f1 @20 dB:
  0.88 vs 0.35). Re-seated the `hpx_hf` dense rungs SL11–SL19 from SC-FDMA to OFDM (PR #704), re-indexed to
  drop the P4-duplicate rungs (PR #707), and ablated the residual moderate_f1 plateau to Doppler (not
  outage or delay-cliff) so it's recorded, not mis-fixed (PR #706). `hpx_ofdm_hf` made a complete robust
  all-OFDM dispersive-HF ladder; linksim now drives the ladder from the daemon's real `rx_snr_db` (PR #708).

### 11.3 — Measurement fidelity ✅ Done
- **Watterson +3 dB hot** path power normalised to unity (PR #701).
- **Gilbert-Elliott** stepped per-sample (sub-symbol "bursts" ≈ elevated-variance AWGN) → now steps
  per-symbol, so Bad runs span whole symbols with mean 1/p_bg symbols — a valid burst channel (PR #714).
- **Watterson re-randomised the fade every `apply()`** → opt-in continuous phase-persistent fade
  (sum-of-sinusoids), default-off so existing thresholds stay bit-identical; linksim opts in (PR #716).
- **CI goodput regression gate** — real-modem `run_link` gates that catch DSP regressions the HPX
  event-replay benchmark can't see (PR #710).
- (`estimate_additive_snr_db` multi-tap was assessed **moot** — zero consumers after linksim adopted
  `rx_snr_db` in #708 — and intentionally not built.)

### 11.4 — Improvement backlog ✅ Done
- Byte interleaver on the `SoftConcatenated` wire — burst-fade tolerance, multi-block gated (PR #709).
- LDPC rate-1/2 PEG graph replacing the random xorshift `H` (PR #711).
- RRC filter span 8→12 on the dense-constellation RRC rungs (PR #712).
- Pilot-plugin soft-LLR calibration + normalised-correlation onset (PR #713).
- QPSK1000-HF-RRC forward-only LMS — a coded Watterson sweep showed the DFE *loses* to forward-only on
  fading (error propagation), confirming the #697 note (PR #715).
- (The QPSK "window-derived β" port and wiring `freq_acquire::acquire` were assessed and **skipped** —
  wrong-premise and risky-for-no-clear-benefit respectively; recorded so they aren't re-opened.)

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
- `docs/dev/archive/backlog-fec-improvements.md` — FEC backlog items (BL-FEC-1 through BL-FEC-6) from RS research session

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
radio constraints was conducted 2026-05-09.  See `docs/dev/archive/backlog-waveforms.md` for the full
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

### FF-14 — SC-FDMA PAPR: non-interleaved pilot redesign *(SUPERSEDED by the OFDM HOM ladder, PR #407)*

**Dropped 2026-06-16.** The premise — that de-interleaving the pilots would recover ~6–8 dB
of PAPR and make SC-FDMA the dense-mode workhorse — did not hold. A prototype measured the
*realized* gain from contiguous pilots at only **~3.8 dB** (12.7 → ~8.9 dB): OpenPulseHF's
SC-FDMA is a **real-valued passband** signal (Hermitian symmetry, 1500 Hz centre), and the
~3 dB real-bandpass penalty floors it well above textbook complex-baseband SC-FDMA. Single-
carrier `64QAM2000-RRC` already has lower PAPR (~6.6 dB) *and* out-throughputs
`SCFDMA52-64QAM`, so SC-FDMA is **dominated** — no PAPR edge, and worse frequency-selective
fade handling (DFT-despread noise enhancement) than OFDM.

**Replacement — the OFDM higher-order ladder** (PR #407, `docs/mode-fec-ladder.md` §4/§7):
OFDM's CP + per-subcarrier equalization handle HF multipath natively (no despread penalty),
the industry choice for HF data (VARA HF, Mercury, ARDOP). Shipped: shared
`openpulse-dsp::constellation` (Gray map / hard-demap / max-log-MAP soft-LLR, QPSK..64QAM);
OFDM modes `OFDM52-{8PSK,16QAM,32QAM,64QAM}`; `hpx_ofdm_hf` extended SL5→SL10. PAPR is
managed by **TX leveling, not clipping** — clipping is QPSK-only (it shreds dense
constellations) and HOM frames are **peak-normalized to a DAC-safe 0.9**. Validated on the
rpi51↔rpi52 hardware loopback: OFDM52-16QAM (uncoded + soft FEC) and OFDM52-64QAM all decode;
OFDM52-16QAM + soft FEC decodes a Watterson Good-F1 channel through the engine.

**SC-FDMA stays as-is** — a working, hardware-validated dense-multicarrier path and the
source the OFDM HOM ladder reused for its constellation code; kept, not retired, not invested
in further.

**Other audit notes** (no action, documented for completeness): SCFDMA52-64QAM-P4 trades
throughput for dense pilots (high-Doppler use, not dominated); plain rectangular 2000-baud
single-carrier modes remain superseded by their `-RRC` variants.

---

## BL-FEC series — FEC codec improvements

Incremental FEC improvements tracked in [`docs/dev/archive/backlog-fec-improvements.md`](../archive/backlog-fec-improvements.md).

| Item | Description | Status |
|---|---|---|
| BL-FEC-1 | Concatenated Conv+RS session mode | ✅ Done (PR #169) |
| BL-FEC-2 | RS t=32 strong codec (RS(255,191), 25% overhead) | ✅ Done (PR #171) |
| BL-FEC-3 | Short-block RS for ACK/control frames (5 B → 13 B) | ✅ Done (PR #170) |
| BL-FEC-4 | Memory-ARQ soft combining (element-wise sample averaging) | ✅ Done (PR #171) |
| BL-FEC-5 | Soft-decision K=7 Viterbi | ✅ Done (PR #177) |
| BL-FEC-6 | Turbo / LDPC codes — `IterativeDecoder` trait + CPU LDPC min-sum implementation (GPU acceleration reserved) | ✅ Done (PR #176) |

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

---

## BL-TP series — Baseline throughput and modulation hardening

These tasks establish the empirical baseline and close the gap between the simulator and a real HF link. All tasks are independent unless noted.

| ID | Title | Status | PR |
|---|---|---|---|
| BL-TP-1 | SNR sweep extended to 0–10 dB (AWGN + Watterson) | Done | #215 |
| BL-TP-2 | Benchmark 20-frame → 50-frame for statistical stability | Done | #213 |
| BL-TP-3 | SC-FDMA block interleaver wired into SC-FDMA encoder/decoder | Done | #213 |
| BL-TP-4 | SC-FDMA 16QAM and 64QAM modes + MMSE equalization | Done | #210 |
| BL-TP-5 | Memory-ARQ end-to-end in SC-FDMA session (soft combining) | Done | #213 |
| BL-TP-6 | Preamble / sync sequence for carrier and timing acquisition | Done | #213 |
| BL-TP-7 | SC-FDMA pilot density review vs measured Doppler spread | Done | #278 |

BL-TP-7 validation is now covered by `plugins/scfdma/tests/pilot_density_review.rs`,
which compares sparse-pilot (`SCFDMA52-64QAM`) and dense-pilot (`SCFDMA52-64QAM-P4`)
profiles across low and high Doppler Watterson settings and gates that dense-pilot
absolute bit agreement is not worse than sparse-pilot at either Doppler point.

---

## RF series — HF robustness and fading fixes

These tasks address the failure modes identified in Watterson channel benchmarking.

| ID | Title | Status | PR |
|---|---|---|---|
| RF-1 | Diagnose and fix BPSK250 100% failure under Watterson Good F2 | Done | #212 |
| RF-2 | SC-FDMA block interleaver (cross-reference BL-TP-3) | Done | #213 |
| RF-3 | Extend SNR sweep to 0–10 dB in benchmark harness (cross-reference BL-TP-1) | Done | #215 |
| RF-4 | Memory-ARQ end-to-end in SC-FDMA HPX session (cross-reference BL-TP-5) | Done | #213 |
| RF-5 | Preamble / acquisition sequence for HF channel entry (cross-reference BL-TP-6) | Done | #213 |

### RF-1 confirmed root cause (PR #212)

Two bugs in the Watterson model caused the 0/20 failure — not a demodulator or timing issue:

**Bug 1 — Independent fading per 1024-sample block.** The model regenerated completely new random envelopes every 1024 samples, creating 65 discontinuous channel jumps within a single RS-encoded frame (~66 k samples). Fix: generate one FFT-shaped envelope per `apply()` call, sized to `next_power_of_two(signal_len)`.

**Bug 2 — Magnitude-only fading creates a static frequency-domain null.** Using `h.norm()` discards the random ionospheric phase, leaving the combining sign set purely by geometry:
- Good F2 (delay 1.0 ms) at fc=1500 Hz: `cos(2π·1500·0.001) = cos(3π) = −1` → permanent destructive null → 0/20
- Good F1 (delay 0.5 ms) at fc=1500 Hz: `cos(3π/2) = 0` → delayed ray orthogonal to I channel → harmless

Fix: use `h.re × √2` so the complex Gaussian's random phase randomises the combining sign per frame, breaking static nulls while preserving unit mean-square amplitude.

After the fix, BPSK250 + RS FEC + block interleaver correctly decodes through Good F2 (seed 5). Differential detection handles frames where the fading sign is consistent throughout; FEC+interleaver corrects the burst errors at sign-transition boundaries.

---

## Profile scope decisions — active vs deferred modes (updated 2026-05-19)

### Current SessionProfile table (profile.rs HEAD)

| Profile | SL range | Initial | Top mode |
|---|---|---|---|
| `hpx500` | SL2–SL6 | SL2 | QPSK500 |
| `hpx_hf` | SL2–SL11 | SL2 | SCFDMA52-64QAM |
| `hpx_ofdm_hf` | SL5–SL10 | SL5 | OFDM52-64QAM |
| `hpx_pilot` | SL2–SL5 | SL2 | PILOT-32APSK500 |
| `hpx_pilot_rrc` | SL2–SL5 | SL2 | PILOT-32APSK500-RRC |
| `hpx_pilot_fast` | SL2–SL5 | SL2 | PILOT-32APSK1000 |
| `hpx_pilot_fast_rrc` | SL2–SL5 | SL2 | PILOT-32APSK1000-RRC |
| `hpx_wideband` | SL8–SL11 | SL8 | 8PSK1000 |
| `hpx_narrowband` | SL8–SL11 | SL8 | 8PSK2000-RRC |
| `hpx_narrowband_hd` | SL8–SL9 | SL8 | 8PSK9600-RRC |
| `hpx_wideband_hd` | SL9–SL15 | SL12 | 64QAM2000-RRC |

### Mode-to-plugin mapping

| Mode | Plugin | Profile slot |
|---|---|---|
| BPSK31/63/250 | bpsk-plugin | hpx_hf SL2–SL4 |
| QPSK250/500 | qpsk-plugin | hpx_hf SL5–SL6 |
| 8PSK500 | psk8-plugin | hpx_hf SL7 |
| SCFDMA52-8PSK/16QAM/32QAM/64QAM | scfdma-plugin | hpx_hf SL8–SL11 |
| QPSK500/1000 | qpsk-plugin | hpx_narrowband SL8–SL9, hpx_wideband SL8–SL9 |
| QPSK2000-RRC | qpsk-plugin | hpx_narrowband SL10 |
| 8PSK2000-RRC | psk8-plugin | hpx_narrowband SL11 |
| 8PSK1000 | psk8-plugin | hpx_wideband SL11 |
| OFDM16/OFDM52/OFDM52-8PSK/16QAM/32QAM/64QAM | ofdm-plugin | hpx_ofdm_hf SL5–SL10 |
| PILOT-QPSK/8PSK/16QAM/32APSK500(/1000)(-RRC) | pilot-plugin | hpx_pilot / _rrc / _fast / _fast_rrc SL2–SL5 |
| QPSK9600-RRC | qpsk-plugin | hpx_narrowband_hd SL8 |
| 8PSK9600-RRC | psk8-plugin | hpx_narrowband_hd SL9 |
| SCFDMA52-16QAM | scfdma-plugin | hpx_wideband_hd SL12 |
| SCFDMA52-32QAM | scfdma-plugin | hpx_wideband_hd SL13 |
| SCFDMA52-64QAM | scfdma-plugin | hpx_wideband_hd SL14 |
| 64QAM2000-RRC | 64qam-plugin | hpx_wideband_hd SL15 |

### Modes in plugins but not in any profile (no planned home)

| Mode | Plugin | Note |
|---|---|---|
| BPSK100 | bpsk-plugin | Low throughput; not competitive vs QPSK |
| SCFDMA52-64QAM-P4 | scfdma-plugin | Dense-pilot research variant; no profile slot |
| 64QAM500 / 64QAM1000 | 64qam-plugin | No profile home |

### Daemon-level implementation gaps (2026-05-19 — both resolved 2026, PR #321)

Found during implementation audit; both have since been wired.

| Gap | Location | Status |
|---|---|---|
| `accept_qsy` records decision but does not invoke `QsySession` or transmit RF QSY frames | `crates/openpulse-daemon/src/lib.rs` | ✅ Done (PR #321) — `QsySession` wired into `AcceptQsy`; QSY_REQ/QSY_LIST transmitted via the modem engine |
| `enable_repeater` sets flag and emits event but does not spawn CrossBandRepeater audio routing thread | `crates/openpulse-daemon/src/lib.rs` | ✅ Done (PR #321) — `EnableRepeater` spawns the repeater thread via `run_full_duplex`; `DisableRepeater` stops and joins it |

### Control-surface parity gaps (audit 2026-06-27)

Audited every `ControlCommand` variant for reachability across the three surfaces (daemon
handler / CLI `daemon` subcommand / panel GUI button) — the "wired at one seam, not all" lens
applied to the control surface. All 28 variants are handled by the daemon (no dead commands).
The reachability gaps:

Original audit (2026-06-27) gap rows, all since **closed** (status as of 2026-06-29):

| Command | Daemon | CLI | Panel | Status |
|---|---|---|---|---|
| `SendMessage` | ✓ | ✓ | ✓ | ✅ closed — `daemon send-message` (`cli.rs`) |
| `SetMode` | ✓ | ✓ | ✓ | ✅ closed — `daemon set-mode` |
| `PttAssert` / `PttRelease` | ✓ | ✓ | ✓ | ✅ closed — `daemon ptt-assert` / `ptt-release` |
| `AcceptQsy` / `RejectQsy` | ✓ | ✓ | ✓ | ✅ closed — `daemon accept-qsy` / `reject-qsy` |
| `SetFreq` | ✓ | ✓ | ✗ | ✅ CLI-reachable — `daemon set-freq` (panel via config) |
| `SetDcdSquelch` | ✓ | ✓ | ✓ | ✅ closed — panel Squelch slider |
| `SetAgc` | ✓ | ✓ | ✓ | ✅ closed — panel `AGC: ON/OFF` toggle (2026-06-29) |
| `SetTxAttenuation` | ✓ | ✓ | ✓ | ✅ closed — CLI `daemon set-tx-attenuation` (2026-06-29; a wiring-gap audit found it panel-only) |
| `StartOtaSession` / `StopOtaSession` | ✓ | ✓ | ✗ | panel only locks/unlocks an active session |
| `OtaSetLevelBounds` / `OtaSetHysteresis` / `OtaSetAggressiveness` | ✓ | ✓ | ✗ | expert OTA tuning, CLI-only by design |

All the reachability gaps the audit flagged are now closed: the CLI gained `send-message`,
`set-mode`, `ptt-assert`/`ptt-release`, `accept-qsy`/`reject-qsy`; the panel gained the live Squelch
slider and (2026-06-29) the `AGC: ON/OFF` toggle. The OTA hysteresis/aggressiveness/bounds CLI-only
split is intentional (the panel offers the simplified lock/unlock instead). No control-surface
parity gaps remain.

### TNC command-surface audit (2026-06-27)

Audited the ARDOP + KISS TNCs for the "accepted/advertised but not applied" gap class.

**ARDOP** (`crates/openpulse-ardop/src/command.rs`): 15 commands are fully implemented; 3 are
accepted + validated + echoed but **not applied** to the modem — `GRIDSQUARE` (informational),
`ARQBW` and `ARQTIMEOUT` (stored, never read by the engine: OpenPulseHF self-manages bandwidth and
session timeout via its adaptive rate ladder, not a host hint). `CWID` / `SENDID` are honest stubs
(warn-logged). Now documented as such (code comment + `docs/non-gpl-interfacing.md`). Real fix, if
host-driven ARQ control is ever wanted: wire `ARQBW`/`ARQTIMEOUT` into `transmit_arq`.

**KISS** (`crates/openpulse-kiss/src/server.rs`): only `KISS_DATA` (0x00) is applied; the control
frames (TXDELAY 0x01, P 0x02, SlotTime 0x03, TXtail 0x04, FullDuplex 0x05, SetHardware 0x06) were
**silently** dropped. They're advisory PTT/CSMA-timing hints and this TNC manages PTT/channel access
itself, so dropping is acceptable per the KISS spec — now logged (`debug!`) instead of silent. Real
fix, if host TX-timing control is wanted: honor TXDELAY/TXtail/P/SlotTime.

#### ARQBW/ARQTIMEOUT now real ✅ Done (2026-06-29)

**Resolved** by building the feature the blocked-finding below called for:
1. **Opt-in adaptive ARQ session** — `[ardop] enable_adaptive_arq` (+ `adaptive_profile`); `main.rs`
   calls `start_adaptive_session`, so `current_tx_level()` is `Some` and the worker takes the
   adaptive `transmit_arq` / `receive_with_ack_hint` branch instead of fixed-mode.
2. **rate_policy bandwidth cap** — `RateAdapter::clamp_to` / `BiDirRateAdapter::clamp_to`;
   `RateAdaptationPolicy::set_max_tx_level` (clamps now + enforces on AckUp);
   `ModemEngine::set_arq_max_tx_level` + `adaptive_profile_modes`;
   `openpulse_qsy::bandplan::max_speed_level_for_bandwidth` maps a Hz cap to a max level.
3. **ARQBW → cap, ARQTIMEOUT → disconnect** — the `bridge.rs` worker applies the ARQBW cap when it
   changes and drops an idle connection after ARQTIMEOUT seconds of no successful exchange.

With adaptive ARQ **off** (the default), there is no ladder/connection to bound, so the host hints
stay accepted-and-echoed no-ops (not a "defined-but-not-consumed" gap — they act the moment a
session is enabled). Tests: `arq_max_tx_level_caps_the_adaptive_ladder` +
`arq_max_tx_level_clamps_an_already_high_session` (modem), `max_speed_level_for_bandwidth_maps_hz_cap_to_a_level`
(qsy). Original blocked-finding (2026-06-28) kept below for history.

#### Why "wire ARQBW/ARQTIMEOUT for real" was blocked (deeper investigation 2026-06-28)

Attempting the real wiring surfaced that it's the same class of gap as the signed-handshake one: the
**ARDOP TNC never drives the adaptive session the host hints would bound.**
- `crates/openpulse-ardop/src/main.rs` builds a `ModemEngine` and registers plugins but never calls
  `start_adaptive_session` / `start_ota_session`. So `current_tx_level()` is always `None` and
  `worker_loop` (`bridge.rs`) always takes the **fixed-`mode` path** (`transmit`/`receive`), never
  the adaptive `transmit_arq` / `receive_with_ack_hint` branch. The rate ladder is dormant.
- So `ARQBW` (a Hz cap on the ladder) has no ladder to cap, and `ARQTIMEOUT` (an ARQ connection
  timeout) has no ARQ session/connection to time out — the worker does single-shot per-frame
  `receive(mode, None)`.
- Worse, the only existing bandwidth-cap lever, `ModemEngine::ota_set_level_bounds`, targets the
  **OTA** controller (`self.ota`), whereas the worker's `current_tx_level()` reads the **rate_policy**
  controller (`start_adaptive_session`) — different mechanisms. There is no rate_policy bandwidth cap
  to wire `ARQBW` to today.

Wiring a no-op into these dead fields would just re-create the "defined-but-not-consumed" gap the
2026-06-27 audit removed, so it was deliberately NOT done. **Real fix (a feature, not a wire):**
(1) make the ARDOP TNC optionally run an adaptive ARQ session (opt-in config → `start_adaptive_session`,
activating the worker's existing adaptive branch); (2) add a rate_policy bandwidth cap
(`SessionProfile` mode→occupied-bandwidth via `openpulse-qsy::bandplan::occupied_bandwidth_hz`) and a
connection-timeout loop; (3) then `ARQBW`→cap and `ARQTIMEOUT`→timeout become real. Tracked here; not
scheduled.

### Config/feature gaps — defined but not consumed (audit 2026-06-27)

Audited every `OpenpulseConfig` field for a reader (the "defined but not consumed" gap). 72 of 79
fields are consumed; the recently-added notch/QSY/logbook fields are all wired. The dead ones —
config you can set that does nothing — all sit in `[radio]`:

| Field(s) | Why dead | Resolution |
|---|---|---|
| `[radio.rig_a]` (whole `RigConfig`) | The daemon configures the primary rig via the **top-level** `[radio]`; `cfg.radio.rig_a` is never read. | Still reserved for the planned multi-rig refactor; docs/template mark it accurately. |
| `RigConfig.backend` / `serial_port` / `rig_file` (the `rig_a` copies) | Per-rig copies, unread; the daemon reads the top-level `[radio]` equivalents. | ✅ Generic CAT now wired at the **top level** — see below. The `rig_a` copies stay reserved for multi-rig. |

**Generic serial CAT backend wired ✅ Done (2026-06-29).** `cat_backend = "generic"` (top-level
`[radio]`, with `serial_port` + `rig_file`) now drives the previously-unreachable `GenericSerialCat`
(FF-13). `RigctldController` gained a real `CatController` impl (the trait doc had claimed it); the
daemon holds a `CatBackend` enum (rigctld | generic) behind `Option`, selected by
`server::build_cat_controller`, gated by the daemon `generic-serial` feature (Unix). Meter polling
stays rigctld-only (separate connection). Tests: `cat_backend_tests` (none → no controller; generic
without a rig file → no controller, no panic).

The field docs + TOML template now mark these accurately so the config no longer looks wired when
it isn't; the underlying multi-rig refactor remains tracked here.

### Signed handshake wired into the daemon connect ✅ Done (2026-06-29)

**Resolved.** The daemon now exchanges the Ed25519 signed handshake over RF on connect:
`ConnectPeer` signs a `ConReq` (with the station's grid) and transmits it SAR-fragmented (the
~530 B frame exceeds the 255 B modem-frame cap); the responder reassembles it in
`process_received_bytes`, verifies it, replies with a signed `ConAck` (also SAR-fragmented), and
records the proven peer identity; the initiator verifies the `ConAck` against its in-flight
`ConReq` and records the verified peer. The verified grid is stamped onto the in-flight ADIF QSO
(taking precedence over the `[logbook.peer_grids]` config fallback), so **logbook item B (peer
GRIDSQUARE from the handshake) is now delivered**. Station key from `[station] identity_key_path`
(default `~/.config/openpulse/identity.key`, auto-generated). New `ControlEvent::PeerVerified`;
30 s CONACK timeout. Grid is a `skip_serializing_if`-empty field on `ConReq`/`ConAck` so legacy
zero-grid frames stay byte-identical. Tests: `handshake_rf_tests` (6, daemon) +
`handshake::tests` grid coverage (3, core). The exchange is **additive** to the existing local
trust eval — `begin_secure_session` still runs; the connection upgrades to "verified" on CONACK.

Original finding (2026-06-27), for history:

Investigating "peer GRIDSQUARE from the handshake" surfaced a larger gap: the Ed25519 signed
handshake (`ConReq`/`ConAck` in `openpulse-core/src/handshake.rs`) is a **tested library primitive
that the daemon never exchanges**. `ModemEngine::begin_secure_session` (the `ConnectPeer` path) is a
**local trust evaluation** (`evaluate_handshake` over the provided params) — it does not send a
`ConReq` or verify a peer's `ConAck` over RF. `ConReq`/`ConAck` are referenced only by the
handshake library + its tests, nowhere in the daemon.

Consequences:
- No authenticated peer identity (or grid) is exchanged on air in normal daemon operation — the
  peer callsign comes from the local `ConnectPeer` argument, not a verified handshake.
- "Peer GRIDSQUARE from the handshake" (logbook richer-fields item B) is therefore **blocked** on
  first wiring the signed `ConReq`/`ConAck` exchange into the daemon connect flow — a substantial
  feature, not a field add. Adding a grid field to the unused primitive now would be a new
  "defined-but-not-consumed" gap, so it was deliberately NOT done.
- Interim for the logbook grid: the `[logbook.peer_grids]` config map (shipped, item A).

Real fix (no target date): wire the over-the-air signed handshake (initiator `ConReq` → responder
verify + `ConAck` → initiator verify) into the daemon's `ConnectPeer`/RF path, storing the verified
peer identity (and an optional grid field) on the engine; then the logbook reads the verified grid.

### Automatic ADIF logbook (opt-in) ✅ Shipped

**Done.** `crates/openpulse-core/src/adif.rs` (`AdifRecord` + ADIF 3.1.4 rendering, band mapping)
and `crates/openpulse-daemon/src/logbook.rs` (`Logbook` — `begin_qso` on connect, `end_qso`
appends on disconnect) with `[logbook]` config (`enabled`, `adif_path`, `peer_grids`) and runtime
`SetLogbook` parity across daemon/CLI/panel. The worked-station `GRIDSQUARE` comes from the verified
handshake when available (see the handshake item above), falling back to the `[logbook.peer_grids]`
config map. Original design notes below for history.

A station logbook that records each contact in **ADIF** (Amateur Data Interchange Format) so logs
import into standard logging software / LoTW / eQSL. Opt-in via a new `[logbook]` config section
(`enabled = false` default, `adif_path = "~/.local/share/openpulse/openpulse.adi"`).

- **QSO source:** derive a record per completed contact, not per frame — keyed on the
  handshake / `ConnectPeer` peer callsign (CONREQ identity), with frequency/band from CAT
  (`RigctldController`), `mode` from the active modem mode, UTC start/end timestamps, and
  RST/SNR from the rate-policy SNR estimate. Distinct from `TxSessionLog` (per-frame regulatory
  audit) — the logbook is per-QSO.
- **Fields:** `CALL`, `QSO_DATE` / `TIME_ON` / `TIME_OFF`, `BAND` / `FREQ`, `MODE` / `SUBMODE`,
  `RST_SENT` / `RST_RCVD`, `STATION_CALLSIGN`, `MY_GRIDSQUARE` / `GRIDSQUARE`, `COMMENT`
  (effective rate / final speed level).
- **Surface:** `[logbook]` config toggle + a control command with CLI/panel parity (per the audit
  above). Append-on-QSO-complete; must never block the modem path.
- **Open questions:** ADIF version (3.1.x), how to map HPX modes to ADIF `MODE` / `SUBMODE`
  (a custom `DYNAMIC` vs. per-mode), and whether to also log FreeDV / Winlink sessions.

### Weak-signal symbol-diversity mode (deferred — from SSB reference mining 2026-07-01)

Flagged from the FreeDV/codec2 reference pass (see `docs/dev/research/references.md` →
*CE-SSB and polar-SSB transmit conditioning*). FreeDV **700D** transmits each carrier's
symbol **twice** across the band ("frequency diversity"), so the receiver combines two
independent fading realisations before slicing. This is a distinct SNR lever from FEC
(it buys diversity, not coding gain) and would give the ladder a rung **below** its
current SL floor for deep-fade / very-weak-signal HF.

- **Idea:** an optional lowest-rate mode (candidate `SL1`-adjacent) that repeats each
  data symbol on a second, band-separated carrier and does maximal-ratio (or equal-gain)
  combining at the receiver before demodulation — trading throughput for a few dB of
  fading margin the FEC alone doesn't provide.
- **Fit:** slots naturally under the OFDM path (per-subcarrier repetition + combine) or
  as a dedicated diversity waveform; complements, does not replace, the RS/LDPC/soft-FEC
  stack.
- **Open questions:** carrier spacing for decorrelated fading vs. occupied bandwidth;
  combine metric (MRC needs per-carrier SNR estimates from the pilots); where it sits on
  the `RateAdapter` ladder (a true sub-floor rung vs. a separate "weak-signal" profile);
  and whether the gain over just dropping to BPSK31 + soft-FEC is worth the complexity.
- **Status:** unscheduled; no target date. Parked here so the diversity lever isn't lost.

### Features shipped (no longer deferred)

- LDPC: real rate-1/2 min-sum belief propagation shipped in PR #187. The "LDPC stub" entry below is obsolete.
- SCFDMA52-16QAM through SCFDMA52-64QAM: all assigned to `hpx_wideband_hd` (PRs #316, #320).
- 64QAM2000-RRC: assigned to `hpx_wideband_hd` SL15 (PR #320).

### Features that remain fully in scope regardless of mode deferral

All of the following improve SNR, PAPR, or link reliability and are never deferred:
- SC-FDMA DFT spreading (3–4 dB PAPR improvement over OFDM)
- MMSE equalization (SNR improvement at weak subcarriers vs ZF)
- Reed-Solomon + block interleaving (burst-error resilience)
- Soft Viterbi (convolutional FEC via `ConvCodec`)
- LMS/DFE adaptive equalizer (`crates/openpulse-dsp`)
- Memory-ARQ infrastructure (`BL-FEC-4`, soft combining)
- LZ4 session compression
- tanh TX limiter (PAPR clipping control)
- Per-band TX attenuation
- Ed25519 + ML-DSA-44 handshake signing
- LDPC rate-1/2 min-sum belief propagation (shipped PR #187; not a stub)
- Full-duplex dual-rig path
- WASM panel WebSocket transport
