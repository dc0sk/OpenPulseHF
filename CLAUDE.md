---
project: openpulsehf
doc: CLAUDE.md
status: living
last_updated: 2026-05-03
---

# CLAUDE.md — OpenPulseHF Agent Contract

This file is the authoritative guide for any coding agent working in this repository. Read it before touching code. Mandatory agent safety rules are in `AGENTS.md` (root) and `docs/AGENTS.md`.

---

## Build and test commands

```bash
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
```

The `--no-default-features` flag disables the CPAL audio backend and is required for CI. All tests must pass with this flag. Never add tests that require real audio hardware.

---

## Crate map

| Crate | Path | Role |
|---|---|---|
| `openpulse-core` | `crates/openpulse-core` | Traits (`ModulationPlugin`, `AudioBackend`), frame format, CRC-16, `FecCodec` (RS), `HpxSession` state machine, plugin registry, trust/signing |
| `openpulse-audio` | `crates/openpulse-audio` | `LoopbackBackend` (testing) and `CpalBackend` (hardware, feature-gated) |
| `openpulse-modem` | `crates/openpulse-modem` | `ModemEngine`, `PipelineScheduler`, benchmark harness, diagnostics |
| `openpulse-cli` | `crates/openpulse-cli` | CLI binary; thin wrapper over modem engine |
| `bpsk-plugin` | `plugins/bpsk` | BPSK31/63/100/250 modulation plugin |
| `qpsk-plugin` | `plugins/qpsk` | QPSK125/250/500 modulation plugin |
| `pki-tooling` | `pki-tooling` | Key management, trust store, signing utilities |
| `openpulse-channel` | `crates/openpulse-channel` | Channel simulation (Watterson, Gilbert-Elliott, QRN/QRM/QSB/Chirp). Full spec in `docs/testbench-design.md` and `docs/benchmark-harness.md`. |
| `openpulse-radio` | `crates/openpulse-radio` | `PttController` trait + `NoOpPtt`, `SerialRtsDtrPtt`, `VoxPtt`, `RigctldPtt` backends. |
| `openpulse-testbench` | `apps/openpulse-testbench` | **Planned — post Phase A.** egui GUI for testing. Full spec in `docs/testbench-design.md`. |

---

## Current phase and execution order

**Active phase: Phase 2 — ACK Taxonomy and Rate Adaptation.** See `docs/roadmap.md` for the full gate criteria.

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
Full spec in `docs/testbench-design.md` and `docs/benchmark-harness.md`.

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
  - HPX2300 waveform decision: single-carrier chosen over OFDM (lower PAPR, no cyclic prefix, simpler AFC — see `docs/architecture.md`)
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
- `crates/openpulse-modem/src/engine.rs`: DCD update wired into `receive()` after sample capture; 0.3-persistence CSMA check in `stage_emit_output()` (applies to all transmit paths); `enable_csma()`, `disable_csma()`, `is_channel_busy()`, `dcd_energy()` public API; `rand 0.8` added to `[dependencies]`
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
  - `WireEnvelope`: encode/decode per docs/peer-query-relay-wire.md; header 104 B + payload + auth_tag 16 B
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
- `pki-tooling/Cargo.toml`: added `rand = "0.8"` for ephemeral key generation
- `pki-tooling/src/lib.rs`: `AppState` gains `signing_key: ed25519_dalek::SigningKey`; new route `GET /api/v1/signing-key`
- `pki-tooling/src/main.rs`: loads `PKI_SIGNING_KEY` env var (base64 32-byte seed); falls back to ephemeral key with warning
- `pki-tooling/src/verification.rs`: added `bundle_canonical_body()` (with recursive key-sort for JSONB stability) and `verify_bundle_signature()`; 8 unit tests
- `pki-tooling/src/api/handlers.rs`: service computes Ed25519 signature at publish time; `bundle_signature` removed from request; `service_pubkey` persisted per-row and returned in `TrustBundleResponse`; `get_signing_key` handler
- `pki-tooling/migrations/0010_trust_bundle_service_pubkey.sql`: adds `service_pubkey TEXT NOT NULL DEFAULT ''` to `trust_bundles`

**Phase 3.5-substitute — sound-loopback channel simulation** ✅ Done (PR #89)
- `crates/openpulse-audio/src/loopback.rs`: added `drain_samples()` and `fill_samples()` test-utility methods to `LoopbackBackend`
- `crates/openpulse-modem/src/channel_sim.rs`: new `ChannelSimHarness` wiring two `ModemEngine` instances through `openpulse_channel::ChannelModel`; `route()` and `route_clean()` methods
- `crates/openpulse-modem/tests/channel_loopback.rs`: 6 integration tests (clean passthrough, AWGN 20 dB, Watterson F1, Watterson F2 negative, G-E light+FEC positive, G-E burst negative)
- `crates/openpulse-channel/src/watterson.rs`: fix `fading_coeff` bug — was passing loop-local index instead of absolute sample index, causing O(n) envelope FFT refills per `apply()` call; fixed to O(n/1024)

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

---

## Open design decisions

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
| Gilbert-Elliott mean burst length | `cargo test -p openpulse-channel` (add in `gilbert_elliott.rs`) |
| Watterson fading envelope non-trivial | `cargo test -p openpulse-channel` (add in `watterson.rs`) |
| PTT assert/release ≤ 50 ms | `cargo test -p openpulse-radio` (add timing test in `noop.rs`) |
| CI multi-platform green | ✅ Both jobs pass (PR #67 re-enabled) |

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

---

## Known sharp edges

**`qpsk-plugin` is in `[dev-dependencies]` in `openpulse-modem/Cargo.toml` but in `[dependencies]` in `openpulse-cli/Cargo.toml`.** This is inconsistent but not currently broken because QPSK is only used through the CLI path. Do not add production paths in `openpulse-modem` that depend on `qpsk-plugin` without moving it to `[dependencies]` first.

**Watterson Doppler envelope resolution at short block sizes.** For the Good F1 profile (Doppler spread = 0.1 Hz), the Doppler shaping filter is sub-bin at 1024-sample FFT size (7.8 Hz/bin at 8000 Hz). The envelope will be approximately constant-amplitude rather than truly diffuse fading. This is acceptable — document it in the implementation. Moderate and Poor profiles (≥ 1.0 Hz) are correctly represented.

**FEC with short payloads.** `FecCodec::encode` always produces a full RS block (255 bytes output for any input ≤ 223 bytes). A 16-byte payload produces 255 bytes. At BPSK31 this is ~65 000 samples = ~8 seconds of audio. Use BPSK250 or QPSK modes for any test that iterates FEC-encoded frames at speed.

**SAR is now implemented** (`crates/openpulse-core/src/sar.rs`). Objects up to 64 005 bytes can be segmented into 255-byte frame payloads and reassembled. PQ handshake (Phase 3.1) is unblocked.

---

## Key documents by topic

| Topic | Document |
|---|---|
| Channel models (Watterson, Gilbert-Elliott) | `docs/benchmark-harness.md` |
| Testbench design (channel models, DSP, UI) | `docs/testbench-design.md` |
| WSJTX weak-signal techniques | `docs/wsjtx-analysis.md` |
| JS8Call speed ladder and ARQ commands | `docs/js8call-analysis.md` |
| VARA architecture and ACK taxonomy | `docs/vara-research.md` |
| PACTOR Memory-ARQ, interleaver, FEC | `docs/pactor-research.md` |
| ARDOP research | `docs/ardop-research.md` |
| HPX waveform design | `docs/hpx-waveform-design.md` |
| HPX state machine | `docs/hpx-session-state-machine.md` |
| Peer query and relay wire format | `docs/peer-query-relay-wire.md` |
| Regulatory compliance | `docs/regulatory.md` |
| Roadmap and phase gates | `docs/roadmap.md` |
| Requirements | `docs/requirements.md` |
| Architecture | `docs/architecture.md` |
| PKI tooling | `docs/pki-tooling-architecture.md` |
| CLI usage | `docs/cli-guide.md` |
| Benchmark harness spec | `docs/benchmark-harness.md` |
| Agent safety rules | `AGENTS.md`, `docs/AGENTS.md` |
