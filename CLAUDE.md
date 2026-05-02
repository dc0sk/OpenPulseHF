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

### Phase 2 — ✅ Partial (2.1, 2.2 complete)

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
