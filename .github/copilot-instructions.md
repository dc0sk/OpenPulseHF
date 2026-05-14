# Copilot Cloud Agent Onboarding — OpenPulseHF

This repository is a large Rust workspace for HF modem, protocol bridges, and tooling. To work efficiently, follow this order.

## 1) Read these first (mandatory)

1. `AGENTS.md` — non-negotiable safety and git rules.
2. `docs/AGENTS.md` — recovery/merge safeguards.
3. `CLAUDE.md` — authoritative build/test commands, crate map, active roadmap status, coding conventions, and known sharp edges.
4. `README.md` — current feature surface and operator-facing behavior.

## 2) Workspace quick map

- Core DSP/protocol crates: `crates/openpulse-core`, `crates/openpulse-dsp`, `crates/openpulse-modem`, `crates/openpulse-channel`, `crates/openpulse-audio`, `crates/openpulse-radio`.
- Protocol/service crates: `crates/openpulse-ardop`, `crates/openpulse-kiss`, `crates/openpulse-b2f`, `crates/openpulse-b2f-driver`, `crates/openpulse-gateway`, `crates/openpulse-qsy`, `crates/openpulse-daemon`, `crates/openpulse-mesh`, `crates/openpulse-repeater`.
- UI/tools/apps: `crates/openpulse-cli`, `crates/openpulse-tui`, `apps/openpulse-testbench`, `apps/openpulse-testmatrix`, `apps/openpulse-panel`, `pki-tooling`.
- Modulation plugins: `plugins/bpsk`, `plugins/qpsk`, `plugins/psk8`, `plugins/64qam`, `plugins/fsk4`, `plugins/ofdm`, `plugins/scfdma`.

## 3) Default validation flow

Use CI-compatible flags unless a task explicitly requires hardware-backed audio:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --no-default-features -- -D warnings
cargo test --workspace --no-default-features
```

Then run targeted crate tests for touched areas to iterate faster.

## 4) Feature-flag and environment rules

- `--no-default-features` is the default for CI and most development loops.
- CPAL/audio-hardware paths are optional and feature-gated; do not assume real audio hardware in tests.
- `openpulse-audio` is used workspace-wide with `default-features = false`; crates opt into `cpal-backend` explicitly.
- Prefer loopback/simulated paths for reproducible tests.

## 5) Coding and PR conventions

- Library crates: no `unwrap()`/`expect()` in production paths.
- Error style: `thiserror` in libraries, `anyhow` in CLI/tests.
- Keep commits small and single-purpose.
- Every behavior change should include/adjust tests.
- Validate changes locally before requesting review.

## 6) Known sharp edges (high impact)

- `qpsk-plugin` dependency placement differs between `openpulse-modem` and `openpulse-cli`; avoid adding new modem production dependencies on it without fixing dependency scope first.
- Watterson Good F1 profile has sub-bin Doppler at current FFT sizing; behavior is expected to be near-constant envelope at short block sizes.
- RS FEC on short payloads expands to full 255-byte blocks; choose faster modes (e.g., BPSK250/QPSK) for iterative tests.

## 7) Error log from onboarding run (and workarounds)

1. **Error:** `cargo clippy --workspace --no-default-features -- -D warnings` failed in pre-existing code (`crates/openpulse-dsp/src/preamble.rs`) with:
   - `clippy::len_without_is_empty`
   - `clippy::manual_is_multiple_of`
   **Workaround used:** treat as a pre-existing baseline issue for this docs-only task; still ran formatting and full workspace tests to validate no regressions from onboarding documentation changes.

2. **Error:** Large file read of `CLAUDE.md` exceeded single-read tool limits.
   **Workaround used:** read in sections using ranged reads (`view_range`) and continue from truncation boundaries.

## 8) Practical execution tips for first-time agents

- Start from `CLAUDE.md` command list instead of inventing commands.
- Use `rg`/`glob` to scope impact before editing.
- Avoid touching unrelated crates in this multi-crate workspace.
- For CI/build/test failures, inspect exact failing crate/file first and confirm whether failure is pre-existing or introduced by your diff.
