# Traceability ledger

Running record of substantive changes as a full chain:
**requirement/change → architecture/design decision → implementation → tests → test results.**

Newest first. See `CLAUDE.md` → *PR hygiene → Traceability* for the standing rule. The per-feature
acceptance gates live in `CLAUDE.md` → *Acceptance criteria*; this ledger adds the design rationale
and the actually-observed results per change.

---

## 2026-06-27 — Control-surface parity (CLI + panel)

- **Requirement/change:** the control-surface audit (`docs/dev/roadmap.md` → "Control-surface
  parity gaps") found `ControlCommand`s reachable from one surface but not another: CLI couldn't
  `SendMessage` / `SetMode` / PTT / accept-reject QSY; panel couldn't `SetDcdSquelch` / start-stop
  OTA. Close the real two-way-operability gaps.
- **Design decision:** mirror existing patterns rather than invent new surface plumbing. CLI →
  thin `simple()` wrappers over the existing `ControlCommand` (identical to `set-cessb`/`set-notch`).
  Panel → toolbar controls mirroring the TX-atten slider (squelch) and OTA lock/unlock block
  (start/stop). Keep the OTA hysteresis/aggressiveness/bounds CLI-only — intentional (panel offers
  the simplified lock/unlock).
- **Implementation:**
  - CLI (PR #559): `crates/openpulse-cli/src/cli.rs` (`DaemonCommands`: SetMode, SetFreq,
    PttAssert/Release, AcceptQsy/RejectQsy, SendMessage); `src/commands/daemon.rs` dispatch arms.
  - Panel (PR #560): `apps/openpulse-panel/src/app.rs` (Squelch slider → `SetDcdSquelch`; OTA
    Start/Stop + `ota_profile` field; new fields `dcd_squelch`, `ota_profile`).
- **Tests:** CLI subcommand parse + connection-stage reachability (manual invocations); daemon-side
  handlers for these commands are covered by `openpulse-daemon` lib tests; panel build + clippy/fmt
  (GUI confirmed visually before merge).
- **Test results:** CLI builds; all new subcommands parse and reach the connect stage;
  `openpulse-daemon` lib: **25 passed / 0 failed**; panel builds, **0 clippy errors**, fmt clean.
  CLI #559 merged; panel #560 merged after visual confirm.

## 2026-06-27 — Seam-gap audit fixes (RX/TX cross-cutting)

- **Requirement/change:** after the notch-on-daemon-path gap, audit every cross-cutting RX/TX
  behavior for the "wired at one entry, not the shared seam" pattern.
- **Design decision:** move each cross-cutting concern to its single shared seam; verify the rest
  are already uniform; record intentional exceptions.
- **Implementation (PR #557):** TX regulatory `log_frame` → `stage_emit_output` seam; RX SNR record
  added to `receive_from_samples_with_fec`; removed duplicate OTA `FrameReceived` emit
  (`crates/openpulse-modem/src/engine.rs`).
- **Tests:** `crates/openpulse-modem/tests/tx_logging_seam.rs` (plain/FEC/ACK paths log);
  existing `FrameReceived` tests use `.any()`.
- **Test results:** new tx_logging_seam tests pass; full modem + daemon suites pass; fmt/clippy
  clean. Verified-not-gaps: DCD unified, CSMA-broadcast intentional, FrameTransmitted on all data
  paths.

## 2026-06-27 — Single RX front-end seam + tripwire (notch gap structural fix)

- **Requirement/change:** the receiver notch ran only on the `receive()` family, not the daemon's
  `accumulate_capture` streaming path — a coverage gap invisible to the (wrong-seam) tests.
- **Design decision:** place the notch at the single convergence point all ~19 capture paths funnel
  through, `route_audio_stage(PipelineStage::InputCapture)`; add a tripwire counter so a feature
  that never runs on a path is visible; test through the production entry, not a convenience seam.
- **Implementation (PR #556):** `route_audio_stage` applies the notch for InputCapture keyed by a
  stored `rx_mode`; `notch_blocks_processed()` counter; removed the two duplicate call sites.
- **Tests:** `notch_runs_on_the_daemon_streaming_capture_path` (drives `accumulate_capture`, asserts
  the counter); auto-QSY daemon test asserts it too.
- **Test results:** notch + QSY + loopback suites pass; single-application preserved on both paths;
  fmt/clippy clean. Prevention checklist added to `CLAUDE.md` → *Known sharp edges*.
