# OpenPulseHF Gap Analysis — 2026-05-18

## Executive summary

Four clusters of gaps stand out across the 40 findings below:

1. **QSY/repeater not wired into the daemon.** `openpulse-qsy` and `openpulse-repeater` are mature crates with tests, but the daemon's `apply_command_to_engine` treats `AcceptQsy`, `RejectQsy`, `EnableRepeater`, and `DisableRepeater` as no-ops. Panel buttons exist; nothing happens when they are pressed.

2. **LDPC is single-block only.** The engine rejects any encoded frame larger than `LDPC_MAX_INFO_BYTES` and asks the call site to split — but no call site does. This caps the practical LDPC payload at ~230 bytes.

3. **Regulatory requirements partially unimplemented.** Periodic station identification (FCC §97.119: every 10 minutes during communication) is required but not scheduled by the daemon. Default `N0CALL` callsign is only rejected by the gateway binary, not the daemon or TUI.

4. **Test coverage is uneven.** `openpulse-panel`, `openpulse-tui`, and `openpulse-gateway` have either zero or one test. Several new functions (`save_qsy_config`, LDPC error path) have no coverage at all.

---

## Fabrication risk

Strongly supported findings (verified by reading code and running grep):
- QSY/repeater no-ops: `crates/openpulse-daemon/src/lib.rs:624-634`
- LDPC cap: `crates/openpulse-modem/src/engine.rs:1359-1364`
- N0CALL only checked in gateway: `crates/openpulse-gateway/src/main.rs:264`
- Zero test counts for panel, TUI: confirmed with `grep -rn "#\[test\]"`
- `WireEnvelope` unknown-type error vs spec discard: code:117 vs spec:229-230

Inference-heavy or lower-confidence claims:
- "Regulatory station ID not scheduled" — absence-of-evidence; I didn't find a periodic ID transmit loop, but the daemon is large and there may be a path I missed.
- "B2F `accepted_count` edge case" — hypothetical; no evidence of a real failure.
- "SC-FDMA 64QAM deferred" — confirmed by a test that explicitly asserts the mode stays below a BER gate; this is intentional, not a bug.

---

## Findings by topic

### 1. QSY integration gap

`AcceptQsy` and `RejectQsy` are wired in the panel, forwarded through the command channel, but silently dropped in `apply_command_to_engine`.

- No-op arm: [`crates/openpulse-daemon/src/lib.rs:626-627`](../../../crates/openpulse-daemon/src/lib.rs#L626)
- QSY session state machine exists but is never instantiated in the daemon: `crates/openpulse-qsy/src/session.rs`
- `QsyScanner` likewise has no callers in the daemon or CLI
- The `allow_trustlevels` policy field in `QsyPolicy` is enforced inside `QsySession` but the daemon never reads `cfg.qsy.allow_trustlevels` from config to construct a policy

### 2. Repeater integration gap

`EnableRepeater` and `DisableRepeater` are no-ops in the daemon. `openpulse-repeater` provides a `Repeater` struct with filter policy and forwarding logic but is never instantiated at runtime in the daemon.

- No-op arm: [`crates/openpulse-daemon/src/lib.rs:628-629`](../../../crates/openpulse-daemon/src/lib.rs#L628)
- Panel buttons at `apps/openpulse-panel/src/app.rs:394-396` send the commands

### 3. SetFreq is a no-op

`ControlCommand::SetFreq` exists in the protocol, the panel renders a rig bar (`draw_rig_bar`), and the rig status events flow in. But `apply_command_to_engine` drops `SetFreq` with no CAT side effect.

- No-op arm: [`crates/openpulse-daemon/src/lib.rs:625`](../../../crates/openpulse-daemon/src/lib.rs#L625)
- `RigctldController` exists in `crates/openpulse-radio/` but is never connected to the daemon command path

### 4. LDPC single-block ceiling

`transmit_with_ldpc` and `receive_with_ldpc` enforce a hard cap at `LDPC_MAX_INFO_BYTES`. Payloads exceeding it return `ModemError::Frame("split payload at call site")`. No call site performs the split, so LDPC is effectively limited to one encoded block (~230 bytes of payload).

- Cap check: [`crates/openpulse-modem/src/engine.rs:1359-1364`](../../../crates/openpulse-modem/src/engine.rs#L1359)
- Comment acknowledging single-block limit: [`crates/openpulse-modem/src/engine.rs:1889`](../../../crates/openpulse-modem/src/engine.rs#L1889)
- No test exercises the error return path for an oversized payload

### 5. Periodic station identification not implemented

FCC §97.119 and CEPT ECC/REC(05)06 require station identification at least every 10 minutes during a communication. The mesh node has a `BeaconScheduler` (`crates/openpulse-mesh/src/beacon.rs:14`) that is configurable, but the daemon modem session has no equivalent scheduled ID transmission.

- Regulatory requirement: [`docs/regulatory.md:54-57`](../../regulatory.md#L54)
- `BeaconScheduler`: [`crates/openpulse-mesh/src/beacon.rs:14`](../../../crates/openpulse-mesh/src/beacon.rs#L14)
- No equivalent timer or trigger in `crates/openpulse-daemon/src/lib.rs`

### 6. N0CALL callsign not rejected at daemon/TUI startup

The gateway rejects `N0CALL` before any transmission (`main.rs:264`). The daemon reads callsign from config but never validates it — a station can transmit over RF with the placeholder callsign.

- Gateway guard: [`crates/openpulse-gateway/src/main.rs:264`](../../../crates/openpulse-gateway/src/main.rs#L264)
- Default callsign in config: [`crates/openpulse-config/src/lib.rs:221`](../../../crates/openpulse-config/src/lib.rs#L221)
- Daemon startup: no equivalent guard in `crates/openpulse-daemon/src/main.rs`

### 7. DCD threshold not user-configurable

`DcdState::new(threshold, hold_samples)` takes threshold and hold window as constructor arguments, but `OpenpulseConfig` has no `[dcd]` section. The modem engine wires DCD with a hardcoded threshold. Users who need sensitivity adjustment for a noisy HF environment have no knob.

- Constructor: [`crates/openpulse-core/src/dcd.rs:26`](../../../crates/openpulse-core/src/dcd.rs#L26)
- No `DcdConfig` or `dcd` section in: `crates/openpulse-config/src/lib.rs`

### 8. SC-FDMA 64QAM and 16QAM modes remain deferred

A test explicitly asserts these modes stay below the BER gate on Watterson channels and therefore remain excluded from the session profile. This is an intentional deferral but represents a capability gap for wideband SC-FDMA.

- Deferral test: [`plugins/scfdma/tests/pilot_channel_estimation.rs:364`](../../../plugins/scfdma/tests/pilot_channel_estimation.rs#L364) (`scfdma_qam_modes_remain_deferred_on_watterson_profile_entry_matrix`)
- Roadmap entry: `docs/dev/roadmap.md` deferred section

### 9. Phase 5.5-reg on-air validation not executed

The roadmap marks Phase 5.5-reg as deferred with no target date. The on-air test plan exists (`docs/on-air_testplan.md`) but there is no execution report, no compliance checklist sign-off, and no evidence of real RF testing. Using the modem on amateur radio frequencies without completing this phase carries regulatory risk.

- Roadmap: `docs/dev/roadmap.md:237`
- Test plan: `docs/on-air_testplan.md`

---

### 10. WireEnvelope: unknown msg_type returns error, spec says discard

The peer-query-relay wire spec says receivers "should check the msg_type field before decoding and discard" unknown types (spec:229-230). The decoder returns `WireQueryError::UnknownMsgType` instead of discarding, making the system less forward-compatible than the spec intends. The test at `wire_query.rs:901` codifies the reject behavior, diverging from the spec.

- Code: [`crates/openpulse-core/src/wire_query.rs:117`](../../../crates/openpulse-core/src/wire_query.rs#L117)
- Spec: [`docs/dev/peer-query-relay-wire.md:229`](../peer-query-relay-wire.md#L229)
- Test: `crates/openpulse-core/src/wire_query.rs:901` (`envelope_rejects_unknown_msg_type`)

### 11. QsyConfig `allow_trustlevels` never read from config file

`QsyPolicy::allow_trustlevels` is defined and enforced in the QSY session state machine, but the `OpenpulseConfig::QsyConfig` struct does not expose this field and the daemon never passes it through. Operators cannot restrict QSY to verified-only peers via config.

- Policy field: [`crates/openpulse-qsy/src/session.rs:29`](../../../crates/openpulse-qsy/src/session.rs#L29)
- Enforcement: [`crates/openpulse-qsy/src/session.rs:301-302`](../../../crates/openpulse-qsy/src/session.rs#L301)
- Missing from config: `crates/openpulse-config/src/lib.rs` (no `allow_trustlevels` field)

### 12. B2F LZHUF: internal and Winlink codecs exist but IRS path choice is non-obvious

`compress_lzhuf` (BE prefix, internal) and `compress_lzhuf_winlink` (LE prefix, Winlink-compatible) both exist. The IRS path in `crates/openpulse-b2f-driver` and `crates/openpulse-gateway` must pick the right one when talking to external Winlink software. The compat decoder (`decompress_lzhuf_compat`) handles both on receive, but there is no test verifying the gateway sends the Winlink-compatible variant to a mock RMS Express response.

- Winlink codec: [`crates/openpulse-b2f/src/compress.rs:79`](../../../crates/openpulse-b2f/src/compress.rs#L79)
- Compat decoder: [`crates/openpulse-b2f/src/compress.rs:116`](../../../crates/openpulse-b2f/src/compress.rs#L116)

### 13. `apps/openpulse-panel` has zero tests

The panel binary has no unit or integration tests. Regressions in event handling, state management, or protocol dispatch are only caught by manual visual inspection.

- Test count: 0 (confirmed by `grep -rn "#\[test\]" apps/openpulse-panel/`)

### 14. `crates/openpulse-tui` has zero tests

The TUI crate has no tests directory and no inline tests. The new QSY status display, key bindings, and `cycle_bandplan` helper are untested.

- Test count: 0
- `cycle_bandplan`: `crates/openpulse-tui/src/main.rs:179`

### 15. `openpulse-gateway` has only one test

The only test (`gateway_round_trip`) uses a mock TCP server. There is no test for partial frame delivery, banner parse failure, or Type C vs Type D proposal negotiation divergence.

- Only test: `crates/openpulse-gateway/src/main.rs:325`

### 16. `save_qsy_config` has no test

The function was added in the previous sprint but is untested. It silently swallows `None` from `default_config_path()` and could silently fail to persist settings.

- Function: [`crates/openpulse-config/src/lib.rs:425`](../../../crates/openpulse-config/src/lib.rs#L425)

### 17. LDPC oversized payload error path is untested

The engine returns `ModemError::Frame(...)` for payloads exceeding `LDPC_MAX_INFO_BYTES` but no test exercises this path.

- Cap: [`crates/openpulse-modem/src/engine.rs:1359`](../../../crates/openpulse-modem/src/engine.rs#L1359)

### 18. `openpulse-dsp` has no integration test directory

All 37 tests are inline unit tests (`#[cfg(test)]` blocks). There is no `tests/` directory exercising multi-stage DSP chains (e.g., modulate → add noise → demodulate) within the DSP crate. End-to-end DSP coverage relies on the modem integration tests.

### 19. `openpulse-mesh` beacon interval compliance untested

The mesh beacon fires on a user-configurable interval. No test checks that the beacon fires at least once every 10 minutes (the regulatory minimum). A misconfigured or zero `beacon_interval_s` could silence the beacon entirely.

- `BeaconScheduler::should_send`: [`crates/openpulse-mesh/src/beacon.rs:26`](../../../crates/openpulse-mesh/src/beacon.rs#L26)

### 20. `openpulse-repeater` has only 6 tests

The repeater's relay policy evaluation and packet forwarding logic have limited test coverage. No test exercises the repeater under a simulated fading channel with dropped packets.

- Test count: 6 (all inline)

### 21. Gardner timing recovery not tested under frequency offset

The Gardner TED (`crates/openpulse-dsp/src/timing.rs`) has 5 clean-channel inline tests but no test under a constant or Doppler frequency offset combined with timing drift. In a real HF environment, AFC and timing work together; they are tested in isolation only.

- Timing tests: `crates/openpulse-dsp/src/timing.rs:101-168`

### 22. ConvCodec not tested through the channel simulation harness

`fec_loopback.rs` verifies ConvCodec in isolation. No test runs BPSK + ConvCodec through `ChannelSimHarness` with Watterson or Gilbert-Elliott models the way `channel_loopback.rs` does for RS FEC. The combination is needed to validate the AWGN-path claim in the codebase documentation.

- ConvCodec reference: `crates/openpulse-modem/tests/fec_loopback.rs:15`
- Channel harness tests: `crates/openpulse-modem/tests/channel_loopback.rs`

### 23. Preamble detector only tested with clean signals

`preamble.rs` has 7 inline tests (correlation, phase coherence, etc.) but all use synthesized clean signals. No test subjects the detector to multipath delay spread or a simulated Watterson F1 profile to measure false-positive/false-negative rates.

- Tests: `crates/openpulse-dsp/src/preamble.rs:314-387`

### 24. PQ handshake SAR: no test with fragmented multi-block delivery

The CLAUDE.md integration test entry claims a "SAR encode→fragment→reassemble→decode round-trip" test exists in `pq_handshake_integration.rs`. Verify whether the test exercises genuine multi-fragment reassembly (>1 fragment) or just the encode/decode path with a short payload that fits in one fragment. If the latter, cross-fragment PQ frame reassembly is untested.

- Test file: `crates/openpulse-core/tests/pq_handshake_integration.rs`

### 25. `openpulse-config` `save_qsy_config` falls back silently when path is unavailable

When `default_config_path()` returns `None` the function returns `Ok(())` without writing. In a read-only or sandboxed environment this means QSY config changes appear to succeed but are not persisted across restarts. There is no warning log at this point.

- Silent `None` return: `crates/openpulse-config/src/lib.rs:427-428`

### 26. Regulatory requirement: transmission bandwidth verification absent

`docs/requirements.md:123` ("no excessive bandwidth") and `docs/regulatory.md` reference IARU band plan bandwidth limits. No runtime check prevents configuring a wideband mode (e.g., `SCFDMA52` at 20+ kHz) on a narrowband amateur allocation. The band plan guardrail in `openpulse-qsy` applies to QSY frequency proposals but not to baseline mode selection.

### 27. Trust store not enforced before transmission

`docs/requirements.md:22` requires trust-store-based verification for station identities. The trust store and `InMemoryTrustStore` are implemented, but `ModemEngine::transmit` does not check whether the caller has a valid signed identity loaded. An unconfigured station can transmit without a trust store entry.

### 28. `ConnectPeer` / `DisconnectPeer` are event-only — no modem handshake

The daemon's `apply_command_to_engine` emits `ControlEvent::RfConnectionChanged` for `ConnectPeer`/`DisconnectPeer` but does not start an HPX session or negotiate a waveform. The panel's connect button changes the UI state but does not initiate an actual over-the-air handshake.

- Event-only handler: [`crates/openpulse-daemon/src/lib.rs:602-613`](../../../crates/openpulse-daemon/src/lib.rs#L602)

### 29. `GetConfig` / `SetConfig` round-trip not tested over WebSocket path

The TCP path has integration tests (`crates/openpulse-daemon/tests/control_port.rs`). The WebSocket path (`crates/openpulse-daemon/src/ws.rs`) dispatches to the same `dispatch_command`, but no test covers the full WebSocket GetConfig → SetConfig → GetConfig round-trip that the panel actually uses.

- WS dispatch: [`crates/openpulse-daemon/src/ws.rs:355`](../../../crates/openpulse-daemon/src/ws.rs#L355)

### 30. `openpulse-config` missing `[dcd]` and `[csma]` sections

CSMA persistence (0.3) and DCD threshold are hardcoded in the modem engine. Operators running in high-noise environments may want to raise the DCD threshold or adjust CSMA to avoid excessive deferrals. No config struct or TOML section exists for these parameters.

- CSMA persistence: `crates/openpulse-modem/src/engine.rs` (hardcoded 0.3)
- DCD default: `crates/openpulse-core/src/dcd.rs:83` (0.01 RMS)

### 31. BPSK AFC estimate only tested at zero offset

`afc_estimate_hz` is tested but tests only verify the estimate is near zero when there is no offset. No test applies a synthetic carrier offset (e.g., +50 Hz) and verifies the estimate tracks it within ±N Hz.

- Inline AFC tests: `plugins/bpsk/src/demodulate.rs:585+`

### 32. OFDM channel estimation not tested under Doppler spread

The OFDM plugin has clean-channel loopback tests and pilot-position tests. No test runs OFDM through a Watterson model to verify pilot-based channel estimation survives frequency-selective fading.

- OFDM tests: `plugins/ofdm/src/lib.rs:129-234`

### 33. `openpulse-mesh` no test for max-hop TTL enforcement

The mesh re-broadcasts beacons with a TTL field. No test verifies that a packet with TTL=0 or TTL=1 is not re-broadcast, confirming the loop-prevention mechanism.

### 34. `openpulse-b2f` no test for multi-message IRS session with partial accept

`B2fSession` in IRS mode processes FC/FS proposal exchange. There is no test where the IRS accepts a subset of proposed messages (mix of accept/defer/reject) and verifies that only accepted messages are transferred.

### 35. `openpulse-ardop` GRIDSQUARE / ARQBW commands untested in integration suite

Pat-compatible commands `GRIDSQUARE`, `ARQBW`, `ARQTIMEOUT`, `CWID`, `SENDID`, `PING` were added to the command parser. The existing integration tests (`ardop_integration.rs`) do not cover these commands; only core commands (VERSION, MYID, STATE, etc.) are exercised.

- Commands added: `crates/openpulse-ardop/src/command.rs`
- Test file: `crates/openpulse-ardop/tests/ardop_integration.rs`

### 36. `openpulse-kiss` AX.25 SSID range not validated

`Ax25Addr` accepts any SSID value from the caller. AX.25 permits SSID 0–15 only (4 bits). Values 16–255 are silently truncated in the wire encoding (1-bit left shift leaves room for exactly 4 bits of SSID). No validation or error is returned for out-of-range values.

- Addr encoding: `crates/openpulse-kiss/src/ax25.rs`

### 37. `openpulse-b2f-driver` `run_iss` timeout is constant

`run_iss` has a hardcoded connect timeout. There is no test verifying that the ISS correctly reports `DriverError::Timeout` when the remote end does not respond within the configured deadline.

- Driver: `crates/openpulse-b2f-driver/src/lib.rs`

### 38. `openpulse-cli` `manifest verify` path not tested end-to-end

`manifest verify` is described as "fully wired to `verify_manifest()`" in CLAUDE.md (PR #189). There is no integration test in `crates/openpulse-cli/tests/` that exercises the full CLI subcommand path (parse args → load manifest file → call verify → exit code).

### 39. `openpulse-gpu` CPU fallback path not exercised in CI

GPU tests are gated behind `#[cfg(feature = "gpu")]`. The CPU fallback path (when `GpuContext` initialization fails or `None` is returned from GPU readback) is only reachable when wgpu cannot create a device. No test exercises the fallback explicitly with a deliberately failing `GpuContext`.

### 40. QSY bandplan awareness: `"unrestricted"` sentinel not documented in protocol spec

The protocol uses the string `"unrestricted"` to mean `bandplan_awareness_enabled = false`. This mapping is implemented in `save_qsy_config` and the TUI cycle array, but is not documented in the daemon protocol (`crates/openpulse-daemon/src/protocol.rs`) or in any architecture document. A client implementor has no spec to follow.

- Mapping code: `crates/openpulse-config/src/lib.rs:425`
- Protocol struct: `crates/openpulse-daemon/src/protocol.rs`
