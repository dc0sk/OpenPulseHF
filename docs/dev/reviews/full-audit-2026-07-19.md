---
project: openpulsehf
doc: full-audit-2026-07-19
status: living
last_updated: 2026-07-19
---

# Full Codebase Audit — 2026-07-19

**Audit scope**: Complete workspace — 44 crates, 10 plugins, all `.rs` source.
**Method**: 6 parallel finder agents (stubs-deadcode, panics-in-libs, test-gaps, security-safety, correctness, docs-drift), each reading actual source. Findings adversarially verified against the code before inclusion.
**Baseline**: `cargo fmt` clean, `cargo clippy --workspace --no-default-features --all-targets -- -D warnings` zero warnings. Test suite requires extended timeout (large workspace).

---

## Executive Summary

The codebase is broadly solid: the FEC, SAR, HPX state machine, handshake, and relay paths are thoroughly defended with audit annotations. The worst findings are in **test integrity** — several acceptance-cited tests are vacuous (always pass) — and in **dead configuration wiring** that erodes operator trust. The DSP/protocol correctness findings are real but mostly affect edge cases or future maintenance, not current production paths. Security is well-hardened; the gaps are defense-in-depth (Argon2 params, zeroize) rather than exploitable vulnerabilities.

---

## Ranked Top Findings

| # | Severity | Area | Finding | Fix Sketch |
|---|----------|------|---------|------------|
| 1 | **Critical** | Test integrity | `bpsk_hardening.rs` has 3 vacuous tests (`assert!(is_ok \|\| is_err)`) — acceptance-cited BPSK loopback correctness gate catches zero regressions | Replace with real assertions: `assert!(result.is_ok())` or assert specific error variant |
| 2 | **Critical** | Test integrity | `qpsk_hardening.rs` has 2 tests with `let _ = r.is_ok() \|\| r.is_err()` — no assertion at all; acceptance-cited QPSK gate catches zero regressions | Add real assertions checking decode output or specific error |
| 3 | **High** | Dead wiring | `DiscoveryConfig.query_new_stations` and `max_queries_per_10min` defined in config but never read by any runtime code | Wire into `DiscoveryRuntime` or remove from config schema |
| 4 | **High** | Dead wiring | `ModemBridge.mesh_mode` set by `CONNECT_MESH` command but never read in bridge worker loop | Wire into TX/RX behavior or remove the command |
| 5 | **High** | Panics | 7 `unwrap()` calls on `raw_wire`/`llrs` in modem FEC decode path — a future FecMode mismatch panics every received frame | Replace with `.ok_or(ModemError::Internal)?` or add a static assertion |
| 6 | **High** | Panics | `scfdma_modulate()` calls `params_for_mode(mode).expect(...)` — invalid mode string panics the daemon at TX time | Return `Result` instead of panicking |
| 7 | **High** | Correctness | CE-SSB peak stretcher is O(window) in a blocking loop — blows RPi4 real-time frame budget | Move to async chunked processing or pre-compute peak table |
| 8 | **Medium** | Panics | ARDOP bridge `thread::spawn().expect(...)` panics on resource exhaustion | Return error; let caller handle gracefully |
| 9 | **Medium** | Panics | KISS TNC `run_with_listener()` double-call panics via `.take().expect(...)` | Return error on second call instead of panicking |
| 10 | **Medium** | Panics | Channel estimation `known.last().unwrap()` guarded only by early return — fragile to refactoring | Use `if let Some(last) = known.last()` pattern |
| 11 | **Medium** | Security | Argon2 KDF uses default params (19 MiB) — at OWASP floor, below production threshold | Tune to m_cost=65536 (64 MiB) for master password |
| 12 | **Medium** | Security | No `ZeroizeOnDrop` for `FileKeystore` master password and secrets — heap persists after drop | Add `zeroize` crate dependency and derive `ZeroizeOnDrop` |
| 13 | **Medium** | Dead wiring | `ControlEvent::Metrics.ecc_rate` hardcoded to `None` in every tick — never wired to engine diagnostics | Wire from `ModemEngine::last_diagnostics()` |
| 14 | **Medium** | Dead wiring | `ControlEvent::Metrics.signal_strength_dbm` hardcoded to `None` — panel shows permanently empty | Wire from radio backend RSSI if available |
| 15 | **Medium** | Dead wiring | `ControlEvent::MonitorFrame` emitted by daemon but never consumed by panel, CLI, or TUI | Add handler in at least one client |
| 16 | **Medium** | Test integrity | 4 tests in `bpsk_hardening.rs` have empty bodies (frame_loss, timeout, retransmit, hw_fallback) — test nothing | Write real error injection or remove |
| 17 | **Medium** | Test integrity | CLAUDE.md acceptance command `-- cm108 gpio` matches zero tests (Cargo AND-filters) | Change to two separate `cargo test` invocations |
| 18 | **Medium** | Docs drift | README Plugins table omits `mfsk16-plugin` (3 of 10 plugins missing) | Add mfsk16, pilot, js8 to table |
| 19 | **Medium** | Docs drift | `.github/copilot-instructions.md` lists only 7 of 10 plugins | Update plugin list |
| 20 | **Medium** | Correctness | Costas PLL unit mismatch risk (omega_rel in unnormalized vs normalized) — requires deeper DSP review | Verify against reference implementation; add unit test with known-frequency tone |
| 21 | **Low** | Security | Non-Unix keystore file permissions are no-ops — Windows files may be group-readable | Add Windows ACL enforcement |
| 22 | **Low** | Security | TOCTOU between `FileStore::open()` exists check and `create()` — symlink race | Use `O_CREAT \| O_EXCL` atomically |
| 23 | **Low** | Panics | `assert_benchmark_regression()` is `pub` and panics — any production caller crashes | Make private or return Result |
| 24 | **Low** | Panics | HKDF expand `expect()` — currently infallible but violates library convention | Convert to `?` |
| 25 | **Low** | Panics | Benchmark `run_scenario` panics on plugin registration failure | Return structured error |
| 26 | **Low** | Panics | Frame encode `payload.len() as u8` truncates silently | Use `u8::try_from()` |
| 27 | **Low** | Correctness | HPX state machine has no timeout out of `Disconnecting`/`Waiting` — potential resource leak | Add configurable timeout with cleanup |
| 28 | **Low** | Correctness | Rate policy u64 frame counter wraps — breaks anti-oscillation hold-off | Saturate or use wrapping_add with check |
| 29 | **Low** | Docs drift | CLAUDE.md Phase 5.2 heading still says Done but LZHUF was removed in PR #948 | Update heading to Removed |
| 30 | **Low** | Docs drift | `release-1.0-criteria.md` version is stale (v0.15.0 vs v0.16.0) | Update version string |
| 31 | **Low** | Docs drift | `osi-layer-map.md` omits Zstd from compression row | Add Zstd |
| 32 | **Low** | Dead code | `QsySession::State` has `#[allow(dead_code)]` — some variants may be unreachable | Audit state reachability |
| 33 | **Low** | Dead code | `Panel transport.rs send_binary` marked `#[allow(dead_code)]` — unused trait method | Remove if truly unused |
| 34 | **Low** | Dead code | `MeshConfig.relay_policy` doc says "reserved" but IS wired — stale comment | Update doc comment |

---

## Detailed Findings by Dimension

### 1. Test Integrity (6 findings)

The most impactful dimension. Multiple acceptance-cited tests are vacuous, meaning regressions in core BPSK/QPSK paths would go undetected.

**Critical:**
- `crates/openpulse-modem/tests/bpsk_hardening.rs:242-243,253-254,266-267` — Three tests (`bpsk_empty_payload_handled`, `bpsk_large_payload_handled`, `bpsk_recovery_exhaustion_transitions_to_failed`) use `assert!(is_ok() || is_err())` which is `assert!(true)`. Cited in CLAUDE.md line 481 as the "BPSK loopback correctness" acceptance gate.
- `crates/openpulse-modem/tests/qpsk_hardening.rs:61,69` — Two tests (`qpsk_empty_payload_handled`, `qpsk_large_payload_handled`) use `let _ = r.is_ok() || r.is_err()` with no assertion. Cited in CLAUDE.md line 482.

**High:**
- `crates/openpulse-modem/tests/bpsk_hardening.rs:174-205` — Four tests (`bpsk_frame_loss_recovery`, `bpsk_timeout_recovery`, `bpsk_retransmit_logic`, `bpsk_hardware_detection_graceful_fallback`) have empty or near-empty bodies.

**Medium:**
- `CLAUDE.md:521` — `cargo test -p openpulse-radio --no-default-features -- cm108 gpio` uses two positional filter args. Cargo AND-filters them; no test name contains both `cm108` AND `gpio`, so zero tests match and the gate passes vacuously.

### 2. Stubs and Dead Code (10 findings)

Two high-severity config fields are defined but never consumed, creating a silent failure mode for operators.

**High:**
- `crates/openpulse-config/src/lib.rs:91,93` — `query_new_stations` and `max_queries_per_10min` are declared, defaulted, and documented in TOML but never read by any runtime code in `openpulse-discovery`.
- `crates/openpulse-ardop/src/bridge.rs:41` — `mesh_mode` is set by `CONNECT_MESH` command (command.rs:330) but never read in the bridge worker loop.

**Medium:**
- `crates/openpulse-daemon/src/lib.rs:678` — `ecc_rate: None` hardcoded in every Metrics tick.
- `crates/openpulse-daemon/src/lib.rs:681` — `signal_strength_dbm: None` hardcoded in every Metrics tick.
- `crates/openpulse-daemon/src/server.rs:812` — `MonitorFrame` emitted but never consumed by any client.

**Low:**
- `crates/openpulse-qsy/src/session.rs:115` — `#[allow(dead_code)]` on State enum.
- `apps/openpulse-panel/src/transport.rs:37` — `send_binary` marked dead code.
- `crates/openpulse-config/src/lib.rs:597` — Stale doc on `relay_policy` (says "reserved" but IS wired).

### 3. Panics in Library Paths (10 findings)

The modem's FEC decode path has 7 unwrap() calls that currently hold an internal invariant but have no compile-time enforcement.

**High:**
- `crates/openpulse-modem/src/engine.rs:2849-2887` — 7 `unwrap()` calls on `raw_wire`/`llrs` in FEC decode. A new `FecMode` that mismatches the soft/hard pairing panics every received frame.
- `plugins/scfdma/src/modulate.rs:18` — `params_for_mode(mode).expect(...)` panics on invalid mode string.
- `crates/openpulse-ardop/src/bridge.rs:141` — `thread::spawn().expect(...)` panics on resource exhaustion.
- `crates/openpulse-kiss/src/lib.rs:80` — `self.tx_data_rx.take().expect(...)` panics on double-call.

**Medium:**
- `plugins/scfdma/src/channel.rs:139` and `plugins/ofdm/src/channel.rs:62` — `known.last().unwrap()` guarded only by early return; fragile to refactoring.
- `crates/openpulse-modem/src/benchmark.rs:344,353` — `panic!()` in public `assert_benchmark_regression()`.

**Low:**
- `crates/openpulse-core/src/session_key.rs:39` — HKDF expand `expect()` (currently infallible).
- `crates/openpulse-modem/src/benchmark.rs:240` — Plugin registration `expect()`.
- `crates/openpulse-core/src/frame.rs:53` — `payload.len() as u8` truncating cast.

### 4. Security and Safety (7 findings)

Well-hardened overall. Gaps are defense-in-depth, not exploitable vulnerabilities.

**Medium:**
- `crates/openpulse-keystore/src/lib.rs:147` — Argon2 KDF at OWASP minimum (19 MiB). Production key material should exceed the floor.
- `crates/openpulse-keystore/src/lib.rs:53-57` — No `ZeroizeOnDrop` for master password or decrypted secrets.
- `crates/openpulse-keystore/src/store.rs:28` — TOCTOU between `exists()` check and `create()`.

**Low:**
- `crates/openpulse-config/src/secret_file.rs:27-30` — No-op permissions on non-Unix.
- `crates/openpulse-core/src/session_key.rs:38-39` — `expect()` in crypto path (violates lib convention).
- `crates/openpulse-config/src/lib.rs:77-78` — §97.221 auto-control compliance not enforced in software.
- `crates/openpulse-core/src/pq_handshake.rs:493-504` — No explicit input size bound before serde_json deserialization.

### 5. Correctness / Domain Logic (5 findings)

The Costas PLL finding requires deeper DSP review; the CE-SSB timing issue is the most actionable.

**High:**
- `crates/openpulse-dsp/src/cessb.rs` — CE-SSB peak stretcher is O(window) in a blocking loop. On RPi4 at 8 kHz, this blows the real-time frame budget.

**Medium:**
- Costas PLL — potential unit mismatch between omega_rel and loop filter normalization. Requires reference-implementation cross-check.
- `crates/openpulse-dsp/src/filter.rs` — `group_delay()` assumes symmetric taps without guarding; wrong on adaptive filters.
- `crates/openpulse-core/src/hpx.rs` — No timeout out of `Disconnecting`/`Waiting` states.

**Low:**
- `crates/openpulse-modem/src/rate_policy.rs` — u64 frame counter wraps theoretically.

### 6. Documentation Drift (6 findings)

Stale plugin lists and version strings; no capability-impacting issues.

**Medium:**
- `README.md:432-443` — Plugins table omits `mfsk16-plugin` (SL1 of hpx_hf).
- `.github/copilot-instructions.md:17` — Lists only 7 of 10 plugins.

**Low:**
- `CLAUDE.md:417` — Phase 5.2 heading still says Done but LZHUF was removed.
- `docs/dev/project/release-1.0-criteria.md:14` — Version is stale (v0.15.0).
- `docs/osi-layer-map.md:20` — Omits Zstd from compression row.
- `CLAUDE.md:147` — `hpx_wideband_hd` summary says SL12–SL15 but covers SL9–SL15.

---

## What's Solid

- **FEC decode path**: Well-defended with audit annotations (F6, F-4, RX-1, RX-2, RX-4).
- **SAR reassembly**: Thoroughly tested with poison/fragment/flood tests.
- **§97.119 station ID**: 74+ code references, gates preventing TX without valid MYID.
- **Handshake trust**: Ed25519 + ML-DSA-44 verification with replay freshness checks.
- **Relay forwarding**: Trust-weighted path scoring with hop-limit and duplicate suppression.
- **Format/lint gates**: Clean workspace; zero clippy warnings.

---

## Recommended Priority

1. **Fix vacuous tests** (Findings 1-2, 16) — highest priority; acceptance gates are lying.
2. **Wire dead config fields** (Findings 3-4) — operators expect these to work.
3. **Replace unwrap() in FEC decode** (Finding 5) — latent crash on future code changes.
4. **Tune Argon2 params** (Finding 11) — simple defense-in-depth improvement.
5. **Add zeroize-on-drop** (Finding 12) — defense-in-depth for secret lifecycle.

---

*Generated by multi-agent loose-ends audit. 6 finder dimensions, ~44 raw findings, 34 kept after verification.*
