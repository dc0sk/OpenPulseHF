---
project: openpulsehf
doc: CLAUDE.md
status: living
last_updated: 2026-05-02
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
| `openpulse-channel` | `crates/openpulse-channel` | **Planned — Phase 1.4.** Channel simulation (Watterson, Gilbert-Elliott, QRN/QRM/QSB/Chirp). Full spec in `docs/testbench-design.md` and `docs/benchmark-harness.md`. |
| `openpulse-radio` | `crates/openpulse-radio` | **Planned — Phase 1.5.** `PttController` trait + serial RTS/DTR, VOX, rigctld backends. |
| `openpulse-testbench` | `apps/openpulse-testbench` | **Planned — post Phase A.** egui GUI for testing. Full spec in `docs/testbench-design.md`. |

---

## Current phase and execution order

**Active phase: Phase 1 — Protocol Foundation.** See `docs/roadmap.md` for the full gate criteria.

Execute Phase 1 tasks in this order. Tasks within the same group are independent and may be parallelised.

### Group 1 — immediate, no design decisions required

**1.1a — Re-enable multi-platform CI**
- File: `.github/workflows/ci.yml` line 23
- Change: remove `if: false` from the `build-and-test` job
- Verify: push a branch and confirm the Ubuntu and macOS jobs run and pass
- Done when: both jobs are green and the `if: false` line is gone

**1.1b — Add block interleaver to `openpulse-core`** (Phase 1.3)
- Add `Interleaver` struct to `crates/openpulse-core/src/fec.rs` alongside `FecCodec`
- Algorithm: stride-based block interleaver from `docs/pactor-research.md` working conclusions
- Default depth: 5 × (expected max burst duration in symbols) — document as a named constant, not a magic number
- Pair depth rule: when used with a convolutional code of constraint length k, depth ≥ 2(k−1); document this in comments
- Tests: round-trip interleave/deinterleave; verify that a burst of length ≤ depth symbols spread across FEC blocks stays correctable
- Done when: `cargo test -p openpulse-core` passes and the interleaver depth is an explicit, documented parameter

**1.1c — Create `openpulse-channel` crate scaffold** (Phase 1.4 setup)
- Create `crates/openpulse-channel/Cargo.toml` and `src/lib.rs`
- Add `openpulse-channel` to workspace members in root `Cargo.toml`
- Add workspace dependency entry for the crate
- Implement `ChannelModel` trait and `ChannelError` type per `docs/testbench-design.md`
- No channel models yet — just the trait, error type, and `build_channel` stub
- Done when: `cargo check -p openpulse-channel` passes

### Group 2 — requires Group 1 complete

**1.4 — Implement channel models**
Full spec in `docs/testbench-design.md` (channel models section) and `docs/benchmark-harness.md`.

Implement in this order; each is independently testable:
1. `AwgnChannel` — Gaussian noise at configurable SNR; seeded `StdRng`
2. `GilbertElliottChannel` — two-state Markov burst model; four named profiles from `docs/benchmark-harness.md`
3. `WattersonChannel` — ITU-R F.1487 two-ray model; seven named profiles from `docs/benchmark-harness.md`; requires `rustfft` for Doppler envelope shaping
4. `QrnModel`, `QrmModel`, `QsbModel`, `ChirpModel` — named profiles in `docs/testbench-design.md`
5. `CompositeChannel` + `build_channel` factory + serde for all config types
6. `PowerSpectrum` + `WaterfallBuffer` in `src/dsp.rs` — spec in `docs/testbench-design.md` DSP section

New workspace dependencies needed: `rustfft = "6.2"`, `rand = { version = "0.8", features = ["std_rng"] }`, `rand_distr = "0.4"`.

Done when: unit tests pass — AWGN power at SNR=0 dB, Gilbert-Elliott mean burst length within 10% over 100 k symbols, Watterson output has non-trivial fading envelope, FFT of a 1500 Hz tone peaks at bin 192 at 8000 Hz sample rate.

**1.3 — Wire interleaver into `FecCodec` and `ModemEngine`**
- Add `InterleavedFecCodec` wrapper (or a flag on `FecCodec`) that applies interleave before encode and deinterleave after decode
- Add `transmit_with_fec_interleaved` and `receive_with_fec_interleaved` to `ModemEngine`
- Add Gilbert-Elliott burst test scenario to `crates/openpulse-modem/tests/fec_loopback.rs`
- Done when: fec_loopback tests pass with Gilbert-Elliott moderate burst injection

### Group 3 — requires resolved design decisions (see below)

**1.2 — SAR (Segmentation and Reassembly)**
- ⚠️ **Design decision is pending — do not implement until the SAR decision is confirmed by the user.** See "Open design decisions" section below.
- Once confirmed: implement SAR encoder and decoder in `openpulse-core`
- Integration tests: round-trip 256 bytes → 64 KB; missing fragment injection; reassembly timeout

**1.5 — Radio interface layer (`openpulse-radio` crate)**
- Create `crates/openpulse-radio/`; add to workspace
- `PttController` trait with: `fn assert_ptt(&mut self) -> Result<()>`, `fn release_ptt(&mut self) -> Result<()>`, `fn is_asserted(&self) -> bool`
- Implementations: `NoOpPtt` (loopback), `SerialRtsDtrPtt` (Linux, `serialport` crate), `VoxPtt` (audio-triggered, no external dep), `RigctldPtt` (TCP to hamlib daemon)
- AFC loop addition: add frequency-offset tracking (±50 Hz) to the BPSK demodulator in `plugins/bpsk`; expose estimated offset in session diagnostics
- CLI additions: `--ptt <none|rts|dtr|vox|rigctld>` and `--rig <address:port>` options in `openpulse-cli`
- Done when: `cargo test -p openpulse-radio` passes with NoOpPtt; serial and rigctld backends compile; PTT assert/release round-trips under 50 ms in loopback timing test

**1.1d — Refactor `openpulse-cli/src/main.rs`**
- Current: 2592-line monolith; not unit-testable
- Target: `src/commands/` module with one file per subcommand: `transmit.rs`, `receive.rs`, `session.rs`, `benchmark.rs`, `devices.rs`, `modes.rs`, `trust.rs`
- Each subcommand module exports a `run(args: &Subcommand) -> anyhow::Result<()>` function
- `main.rs` becomes a thin dispatcher (~100 lines)
- Do not change any CLI behavior — this is a refactor only
- Done when: `cargo test -p openpulse-cli` passes and `main.rs` is under 150 lines

---

## Open design decisions

These must be confirmed by the user before the relevant implementation starts. Do not implement speculatively.

### SAR wire format (blocks Phase 1.2 and Phase 3.1)

Two options are on the table per `docs/roadmap.md` section 1.2:

**Option A — extend `length` field to `u16`**
- Frame format version bump to 0x02
- Payload capacity increases from 255 to 65535 bytes
- ML-DSA-44 (2420 bytes) and ML-KEM-768 (1184 bytes) both fit in a single frame
- Simpler implementation; no reassembly state
- Downside: a 2420-byte frame at BPSK31 takes approximately 620 seconds to transmit — impractical on air; useful only at higher baud rates

**Option B — full SAR sub-layer (recommended)**
- Keep the current 255-byte frame payload limit
- Add a SAR header inside the payload: `segment_id (u16) | fragment_index (u8) | fragment_total (u8) | data`
- Effective maximum data per fragment: 251 bytes
- Reassembly buffer keyed on `(session_id, segment_id)`; reassembly timeout is configurable
- ML-DSA-44 (2420 bytes) needs 10 fragments; ML-KEM-768 (1184 bytes) needs 5
- Enables ARQ per-fragment: only failed fragments are retransmitted
- Recommended because large frames are impractical at low baud rates regardless of the length field width

**Ask the user to choose before starting Phase 1.2.**

### `PttController` trait location (blocks Phase 1.5)

Resolved: implement in a new `crates/openpulse-radio` crate. This keeps OS-specific code (serial port, rigctld TCP) out of `openpulse-core`. `openpulse-modem` gains a `PttController` field in `ModemEngine` via a `Box<dyn PttController>` parameter.

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
| CI multi-platform green | All jobs pass on PR (once `if: false` removed) |

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

**CI is disabled.** `.github/workflows/ci.yml` has `if: false` on the `build-and-test` job since PR #50. Only `cross-aarch64-linux` and `pi5-smoke-loopback` jobs run. Re-enabling is the first task (1.1a).

**`openpulse-cli/src/main.rs` is 2592 lines.** Any CLI work before the Phase 1.1d refactor means touching a file that can't be unit-tested. Keep CLI changes minimal until after the refactor.

**`qpsk-plugin` is in `[dev-dependencies]` in `openpulse-modem/Cargo.toml` but in `[dependencies]` in `openpulse-cli/Cargo.toml`.** This is inconsistent but not currently broken because QPSK is only used through the CLI path. Do not add production paths in `openpulse-modem` that depend on `qpsk-plugin` without moving it to `[dependencies]` first.

**`PttController` trait does not exist yet.** Phase 1.5 cannot start until the trait is defined in `openpulse-radio`. The trait definition is unambiguous (see Phase 1.5 description above) and can be stubbed without implementing any backends.

**Watterson Doppler envelope resolution at short block sizes.** For the Good F1 profile (Doppler spread = 0.1 Hz), the Doppler shaping filter is sub-bin at 1024-sample FFT size (7.8 Hz/bin at 8000 Hz). The envelope will be approximately constant-amplitude rather than truly diffuse fading. This is acceptable — document it in the implementation. Moderate and Poor profiles (≥ 1.0 Hz) are correctly represented.

**FEC with short payloads.** `FecCodec::encode` always produces a full RS block (255 bytes output for any input ≤ 223 bytes). A 16-byte payload produces 255 bytes. At BPSK31 this is ~65 000 samples = ~8 seconds of audio. Use BPSK250 or QPSK modes for any test that iterates FEC-encoded frames at speed.

**SAR is unimplemented.** Any object larger than 255 bytes cannot be transported in a single frame. The HPX session protocol handles multi-frame transfers at the application layer, but there is no SAR sub-layer. Do not implement PQ handshake (Phase 3.1) before SAR is complete (Phase 1.2).

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
