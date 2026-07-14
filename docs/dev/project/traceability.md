# Traceability ledger

Running record of substantive changes as a full chain:
**requirement/change → architecture/design decision → implementation → tests → test results.**

Newest first. See `CLAUDE.md` → *PR hygiene → Traceability* for the standing rule. The per-feature
acceptance gates live in `CLAUDE.md` → *Acceptance criteria*; this ledger adds the design rationale
and the actually-observed results per change.

---

## 2026-07-14 — feat(daemon): independent PTT watchdog thread (B1 / #863)

- **Requirement/change:** the PTT watchdog must force-release the transmitter on its 180 s max-keyed
  deadline **even while the daemon's single async command loop is blocked inside a long synchronous
  handler** (a QSY frequency scan or an OTA send-retry burst). The PR #853 `select!`-arm watchdog cannot:
  the loop never re-enters `select!` during such a handler, so the arm never runs and the transmitter
  can key past the duty-cycle limit.
- **Design decision:** approach B — a `SharedPtt(Arc<Mutex<PttInner>>)` holding the PTT controller **and**
  its deadline behind one lock, driven by an **independent OS thread** (`spawn_watchdog`) that checks the
  deadline every 100 ms regardless of the async loop. A plain thread (not a tokio task) so it is immune to
  runtime flavor / worker starvation / a missing `block_in_place`; it holds only a `Weak` so it exits on
  its own when the daemon drops `runtime_state` (no stop-flag plumbing). `SharedPtt` is the single source
  of truth (rejected a mirror-atomic variant — split-brain races). Lock discipline: the mutex is only ever
  held for one hardware call or a deadline read/write, **never across an RF burst**, so the thread can
  preempt at any point. Beacon keys stay silent (`key(None)`/`unkey(None)`); OTA-ACK / station-ID /
  OTA-send / filexfer keys emit `PttChanged` edges only on real transitions. The manual `PttAssert`/
  `PttRelease` split is preserved: HW in `handle_ptt_command` (server), arm+event in
  `apply_command_to_engine` (lib), so a hard HW failure still skips dispatch (#836 contract). Per a Fable
  adversarial review, two behaviours were upgraded past bare parity: (1) on a **failed** force-release
  (stuck rig) the deadline is left **armed** and no `{false}` is emitted (the rig is still keyed — the
  pre-#863 code cleared + lied), and the 100 ms thread retries until it releases, logging the stuck
  condition once; (2) all `PttChanged` sends now happen **under the lock** so a concurrent re-key can't
  misorder the edges.
- **Implementation:** `crates/openpulse-daemon/src/ptt.rs` (new — `SharedPtt`, `PttInner`, `UnkeyOutcome`,
  `key`/`unkey`/`hw_assert`/`hw_release`/`arm`/`disarm`/`is_keyed`/`force_release_if_expired`/
  `spawn_watchdog`); `lib.rs` (`RuntimeControlState.ptt: SharedPtt` replacing the `ptt_asserted_at` +
  `ptt_max_duration` fields; `check_ptt_watchdog` delegates; `PttAssert`→`arm`, `PttRelease`→`disarm`,
  `GetPttState`→`is_keyed`); `server.rs` (`build_ptt_controller` → `Box<dyn PttController + Send>`;
  construct `SharedPtt` + `spawn_watchdog`; loop-local `ptt` clone drives every key/unkey site; helpers
  `handle_ptt_command`/`ota_send_with_ptt`/`transmit_beacon_with_ptt`/`drain_filexfer_tx` take
  `&SharedPtt`; `release_ptt_on_watchdog` deleted; two inline rx-tick keying sites rewritten). Net −104
  lines of scattered PTT bookkeeping in server.rs.
- **Tests:** `ptt.rs` unit suite (8) — key/unkey single-event arming, failed-release-stays-armed,
  force-release single-fire + idempotent, not-expired-no-fire, **watchdog thread force-releases a blocked
  loop** (the #863 guarantee), thread exits when the last `SharedPtt` drops, **stuck-rig retry stays armed
  + silent until release**, **late unkey after the watchdog fired is a single-fire no-op**. Migrated
  server.rs tests (beacon-arm, `handle_ptt_command` guard, watchdog release) to the `SharedPtt` API.
- **Test results:** `cargo test -p openpulse-daemon --no-default-features` → 106 lib + 2 + 13 + 5 + 3
  integration, 0 failed; `cargo build --workspace --no-default-features` clean; clippy `-D warnings` +
  fmt clean.

---

## 2026-07-14 — feat(kiss): honor the FullDuplex control frame (CSMA); clarify no-profile modes

- **Requirement/change:** roadmap follow-ups D1 (KISS control frames were dropped-and-logged; "real fix if
  host TX-timing control is wanted") and D2 (plugin modes with no profile home).
- **Design decision (D1):** this TNC has no PTT-keying layer (data → `engine.transmit`), so TXDELAY/TXtail
  (PTT keying delays) genuinely don't apply, and P/SlotTime (CSMA persistence/slot) have no public setter;
  those stay logged-and-ignored (honoring them would be defined-but-unconsumed). But KISS **does** enable
  engine CSMA, so **FullDuplex (0x05)** maps cleanly: a non-zero value → full duplex → `disable_csma`
  (no carrier-sense deferral); zero → `enable_csma`. Extracted the control-frame handling into a testable
  `apply_kiss_control_frame` (per the #836 pattern). Added `ModemEngine::is_csma_enabled` getter and
  `kiss::KISS_FULLDUPLEX`.
- **Design decision (D2):** the no-profile modes (BPSK100, 64QAM500/1000, SCFDMA52-64QAM-P4) are
  **intentionally manual-select-only**, not a gap — documented as such in the roadmap so it stops reading
  as open work. No code change.
- **Implementation:** `crates/openpulse-modem/src/engine.rs` (`is_csma_enabled`);
  `crates/openpulse-kiss/src/kiss.rs` (`KISS_FULLDUPLEX`); `crates/openpulse-kiss/src/bridge.rs`
  (`apply_kiss_control_frame` + test); `crates/openpulse-kiss/src/server.rs` (call it); `roadmap.md` (D2).
- **Tests:** `kiss_fullduplex_control_frame_toggles_csma` — FullDuplex 1 → CSMA off, 0 → on, TXDELAY →
  no-op.
- **Test results:** `cargo test -p openpulse-kiss --no-default-features` → 3 lib (2 → 3) + 9 integration;
  fmt + clippy clean.

---

## 2026-07-14 — docs(ardop): document the engine-mutex-across-RF-burst invariant (audit #830 close-out)

- **Requirement/change:** #830's ARDOP concurrency item ("worker holds the engine mutex across blocking RF
  TX/RX while CONNECT/DISCONNECT lock it on the async executor"). The CONNECT/DISCONNECT/MYID half was
  fixed in #846/#849 (`spawn_blocking`). This entry closes out the *worker-holds-the-lock* half.
- **Investigation (Fable adversarial concurrency analysis + verified):** ARDOP's concurrency is **host-side
  only** — a TCP command server (`tokio::spawn` per client) plus a background modem OS thread
  (`worker_loop`) sharing `Arc<Mutex<ModemEngine>>`; the on-air path is deliberately half-duplex (single
  frame at a time), and **the engine mutex doubles as the half-duplex channel-access serializer**. Verified
  facts: `CpalInputStream::read` is a ≤10 ms poll (so the IRS receive→ACK lock scope is ~tens of ms, not a
  block); the long hold is TX playback (`stream.flush` blocks the driver drain) across `transmit_arq`, which
  has **no cancellation path**; ABORT is lock-free (`command.rs` "ABORT" arm, regression-tested by
  `connect_holding_the_engine_lock_does_not_stall_an_abort`, #846). So after #846 the residual is only
  bounded, off-executor latency on CONNECT/DISCONNECT/MYID (runtime never stalls; STATE/ABORT stay live).
- **Design decision:** **working-as-intended after #846 — document, don't "fix".** Hoisting the IRS capture
  read out of the lock would *introduce* bugs (a second input stream drains the shared LoopbackBackend
  buffer; a CONNECT's `begin_secure_session` could reset session state between capture and ACK — the exact
  interleave the single scope prevents), and a lock-scope change would only return a host's response line
  sooner while the RF keeps transmitting. The only real shortening is a future interruptible `transmit_arq`
  (an `AtomicBool` checked between retransmits) — a separate feature, not this item, not filed absent a
  bug report.
- **Implementation:** `crates/openpulse-ardop/src/bridge.rs` — invariant comments at the ISS-TX and IRS-RX
  lock sites (why the hold is deliberate, why not to hoist the capture read, and the ABORT escape hatch).
- **Tests:** none new (documentation only); `cargo test -p openpulse-ardop` → 7 lib + 23 integration
  unchanged; fmt clean. The ABORT-responsiveness guarantee remains enforced by the #846 test.

---

## 2026-07-14 — feat(core/mesh): source-accumulated multi-hop route discovery (wire-path TLV)

- **Requirement/change:** roadmap 12.3 deferral — a `WireEnvelope` carries no hop trail, so a route
  answerer could only vouch for a single self-hop (or a cached route); true source-accumulated multi-hop
  paths were out of scope.
- **Design decision:** extend `RouteDiscoveryRequest` with an `accumulated_path: Vec<[u8; 32]>` (wire:
  `path_count(1) | path[count × 32]` after the 47-byte header; a header-only 47-byte frame decodes to an
  empty path, so old encoders stay compatible). Each forwarding node appends its own id via
  `accumulate_forwarder` (loop-guarded — refuses a peer already on the path — and bounded by `max_hops`)
  before re-flooding, so the destination answers with the real route: `path_hops(accumulated_path)` then
  itself (a cached-route answer prepends the path to its cached hops). The mesh does this in a new
  `propagate_route_request` (decode → accumulate → re-encode → flood).
- **Implementation:** `crates/openpulse-core/src/wire_query.rs` (field + `HEADER_SIZE` + encode/decode);
  `crates/openpulse-core/src/route_discovery.rs` (`accumulate_forwarder`, `path_hops`, `answer` builds
  from the path); `crates/openpulse-mesh/src/lib.rs` (`propagate_route_request`).
- **Tests:** core `destination_answers_with_the_source_accumulated_multihop_path` (two forwarders +
  loop-guard → 3-hop route recorded) and `accumulate_forwarder_is_bounded_by_max_hops`; the request
  round-trip now carries a 2-element path; mesh `route_request_accumulates_the_forwarder_path` (decode B's
  re-flooded frame → `accumulated_path == [B]` → a destination responder answers with `[B, dst]`).
- **Test results:** `cargo test -p openpulse-core --no-default-features` → route_discovery 13 (11 → 13),
  lib 277; `-p openpulse-mesh` → 12 (11 → 12); `cargo build --workspace` green; fmt + clippy clean.

---

## 2026-07-14 — feat(mesh): route-maintenance drive — RelayRouteUpdate/Reject (0x07/0x08)

- **Requirement/change:** issue #830 — the #830 route-discovery item named four msg types; #840/#841/#850
  built the request/response drive (0x03/0x04), leaving `RelayRouteUpdate` (0x07) and `RelayRouteReject`
  (0x08) **codec-only** (route repair/teardown — no node originated/answered/applied them). This closes
  the last route-discovery gap.
- **Design decision:** extend `openpulse-core::route_discovery` with the maintenance drive, mirroring the
  0x03/0x04 model. **Update (0x07)** is *signed* (self-authenticating, as the response is):
  `sign_route_update`/`verify_route_update` over `(route_id, prev_hop_count, reason, replacement_hops)`;
  `RouteResponder::build_route_update` emits it; `apply_route_update` verifies against the emitter's
  `src_peer_id`, derives the destination from the last replacement hop, and refreshes the table
  (`RouteTable::apply_update` overwrites the entry when the `route_id` matches — an authoritative refresh,
  even on degradation — else admits via `record`). **Reject (0x08)** is *unsigned* on the wire, so it is
  authorized structurally: `apply_route_reject` tears down the route only when `reject_hop_peer_id` is
  actually one of that route's hops (`RouteTable::entry_by_route_id` + `remove`) — an off-path peer cannot
  invalidate a route it does not carry. `MeshDaemon` gains dispatch arms (0x07 → apply + flood; 0x08 →
  apply + flood), `RouteUpdated`/`RouteRejected` events, and `send_route_update`/`send_route_reject`
  originators.
- **Implementation:** `crates/openpulse-core/src/route_discovery.rs` (sign/verify + `apply_route_update` /
  `apply_route_reject` + `RouteTable::{entry_by_route_id, remove, apply_update}` +
  `RouteResponder::build_route_update`); `crates/openpulse-mesh/src/lib.rs` (dispatch + events +
  originators).
- **Tests:** core — `route_update_verifies_and_refreshes_the_existing_route`,
  `route_update_rejects_a_tampered_signature_and_empty_hops`,
  `route_reject_from_an_on_path_hop_tears_down_the_route` (on-path teardown + off-path `Unauthorized` +
  `UnknownRoute`); mesh — `route_update_then_reject_over_air` (a signed update is applied through the mesh
  dispatch → `RouteUpdated`, then an on-path reject tears it down → `RouteRejected`, end-to-end via the
  loopback tap).
- **Test results:** `cargo test -p openpulse-core --no-default-features` → route_discovery 11 (8 → 11),
  lib 275; `cargo test -p openpulse-mesh --no-default-features` → 11 (10 → 11); `cargo build --workspace`
  green; fmt + clippy (`--tests -D warnings`) clean. **Route discovery is now fully driven (0x03–0x08).**

---

## 2026-07-14 — fix(daemon): decouple the PTT watchdog + drop `biased` so a command flood can't starve rx

- **Requirement/change:** issue #830 robustness — `server::run`'s single `tokio::select!` was `biased` with
  the command arm first, and the safety-critical PTT watchdog + rx decode both lived **inside** the
  `rx_ticker` arm. A sustained (default loopback-only) client command flood therefore always won the
  biased race, starving the rx tick **and** the watchdog with it — the transmitter could stay keyed past
  its deadline.
- **Design decision:** give the watchdog its **own** `select!` arm on a fast (100 ms) timer and remove
  `biased`, so tokio schedules the three arms (watchdog / commands / rx) fairly — no arm can be starved by
  a flood, and the watchdog's force-release runs on its own cadence regardless of rx load. The watchdog
  body is extracted into `release_ptt_on_watchdog` and called from both the dedicated arm (primary,
  flood-proof) and the rx tick (idempotent belt-and-suspenders). The engine is a single-threaded owned
  resource shared by both arms, so this is fairness within one loop — not a move to separate tasks.
- **Implementation:** `crates/openpulse-daemon/src/server.rs` — `watchdog_ticker`, the new `select!` arm,
  the removed `biased`, and the `release_ptt_on_watchdog` helper.
- **Tests:** `watchdog_releases_the_transmitter_when_the_deadline_passes` (armed + past-deadline → one
  hardware release via a counting PTT double + disarm + a single `PttChanged{active:false}`; idempotent on
  a second call). The full daemon suite — incl. `twin_daemon_bridge` which drives `server::run` — stays
  green, so the restructuring is behaviour-preserving.
- **Test results:** `cargo test -p openpulse-daemon --no-default-features` → 98 lib (97 → 98) +
  integration all pass; fmt + clippy (`--tests -D warnings`) clean.
- **Remaining (deferred, separate change):** the *in-handler* blocking half — a QSY scan or an OTA
  send/retry burst still holds the command arm (with internal awaits) for tens of seconds, during which no
  `select!` arm (watchdog included) is evaluated, so an `Abort`/`PttRelease` is delayed until the handler
  returns. Fixing that needs the long handlers to yield to the loop (a state machine) or a truly
  independent watchdog task with `Arc<Mutex>`-shared PTT state — a larger, higher-risk refactor than this
  fairness fix.

---

## 2026-07-14 — fix(modem): compliance-fence the `transmit_iq` TX path (audit G-2)

- **Requirement/change:** issue #830 — `transmit_iq` writes baseband IQ via `write_iq` directly, bypassing
  the single `stage_emit_output` audio seam, so it ran **no** regulatory TX-metadata log, never incremented
  `frames_transmitted` (which arms the auto-ID timer), and applied no TX attenuation. Test-only today, but a
  latent non-compliant on-air path for any future SDR/IQ backend caller.
- **Design decision:** extract the seam's compliance bookkeeping (regulatory §97 log + `frames_transmitted`
  bump) into a shared `record_tx_frame(mode)` called by **both** `stage_emit_output` and `transmit_iq`, so
  no emit path can drift from the log/ID accounting. `transmit_iq` also now applies the configured
  `tx_attenuation_db` to the baseband IQ (power control; 0 dB is a no-op). The only seam transforms it still
  omits are the audio-envelope-domain CE-SSB conditioner and the `tanh` peak limiter, which have no
  IQ-domain equivalent — documented as the caller's (hardware/PA/SDR-headroom) responsibility on this path.
- **Implementation:** `crates/openpulse-modem/src/engine.rs` — new `record_tx_frame`; `stage_emit_output`
  and `transmit_iq` both call it; IQ attenuation scaling + revised `transmit_iq` doc.
- **Tests:** `transmit_iq_records_regulatory_log_and_arms_auto_id` (IQ TX → `frames_transmitted() == 1`,
  one regulatory-log frame with the station id) and `transmit_iq_applies_tx_attenuation` (−20 dB → IQ
  magnitude RMS scales to ~0.1×). The existing `station_id_txcount` + `station_id` suites still pass (the
  `record_tx_frame` extraction is behaviour-preserving for the audio seam).
- **Test results:** `cargo test -p openpulse-modem --no-default-features` → iq_output 7 (5 → 7) + full
  suite pass; fmt + clippy (`--tests -D warnings`) clean.

---

## 2026-07-14 — feat(daemon): honor the per-band `SetTxAttenuation { band }` override

- **Requirement/change:** issue #830 — `SetTxAttenuation { db, band }` carried an optional `band`, but the
  daemon destructured it away (`{ db, .. }`) and only ever set the single global engine attenuation, so
  per-band requests were silently dropped. Honoring `band` needs per-band memory + application on retune.
- **Design decision:** mirror the existing per-band DCD-squelch mechanism exactly. `RuntimeControlState`
  gains `tx_attenuation_default` + `tx_attenuation_bands` (band label → dB), and a new
  `apply_band_attenuation(engine, rs, freq_hz)` resolves per-band override → default (twin of
  `apply_band_squelch`), called alongside it on the `SetFreq` retune. In `apply_command_to_engine`,
  `band: Some(label)` stores the override and applies it immediately only when `band_label_for_hz(last_freq_hz)`
  matches; `band: None` sets the global default and applies the effective value (a matching per-band
  override still wins on the current band). `SetConfig` seeds the default the same way. In
  `dispatch_command`, the shared/reported global attenuation is updated only for `band: None` (a per-band
  override is tracked engine-side, not conflated into the reported global).
- **Implementation:** `crates/openpulse-daemon/src/lib.rs` — struct fields + defaults, `apply_band_attenuation`,
  the rewritten `SetTxAttenuation`/`SetConfig` apply arms, the `SetFreq` retune call, and the
  `dispatch_command` guard.
- **Tests:** `apply_band_attenuation_uses_per_band_override_else_default` (40m override / 20m default /
  out-of-band default); `set_tx_attenuation_per_band_stores_and_applies_on_the_matching_band` (stored but
  not applied off-band → applied on retune to that band → global default set while on 20m still yields the
  20m override → moving to an un-overridden band applies the default). The pre-existing
  `apply_set_config_updates_mode_and_tx_attenuation` still passes (backward compatible).
- **Test results:** `cargo test -p openpulse-daemon --no-default-features` → 97 lib (95 → 97) +
  integration all pass; fmt + clippy (`--tests -D warnings`) clean.
- **Note:** per-band overrides are runtime-only (no config field / no persistence); `GetConfig`/`GetStatus`
  report the global default, not the per-band-effective value.

---

## 2026-07-14 — feat(mesh): drive the route originator + consume discovered routes for relay send

- **Requirement/change:** issue #830 — #841 wired the `RouteResponder` into `MeshDaemon` (a node answers
  route queries), but nothing **originated** a query, nothing **applied** a response, and no live path
  **consumed** a discovered route — `score_route`/`select_best_scored_route` were still uncalled outside
  their unit tests. Route discovery dead-ended at "answer only".
- **Design decision:** add a `RouteOriginator` to `MeshDaemon` and close the loop. `discover_route(dst)`
  originates + transmits a `RouteDiscoveryRequest`; the `dispatch` gains a `RouteDiscoveryResponse` arm
  that, for a response addressed to us, verifies + records the route via `apply_response` (else forwards
  it toward the originator). `send_via_route(dst, payload)` **consumes** the table: it builds candidate
  routes (the discovered multi-hop route, plus a direct link when `dst` is a cached neighbour), picks the
  best with `select_best_scored_route` (which calls `score_route`), and transmits a `RelayDataChunk` whose
  hop limit is the chosen route's length. `MeshError::NoRoute` when nothing is known. Full
  source-route-in-the-envelope forwarding still needs the deferred wire-path field (#840); this consumes
  the route to bound and gate the send, which is what closes the "nothing consumes a route" gap.
- **Implementation:** `crates/openpulse-mesh/src/lib.rs` — `route_originator`/`relay_policy`/nonce fields;
  `discover_route`, `send_via_route`, `handle_route_discovery_response`; `MeshEvent::RouteDiscovered` /
  `RouteUsed`; `MeshError::NoRoute`.
- **Tests:** `originator_discovers_then_sends_along_the_route` (mesh loopback) — `NoRoute` before
  discovery; A `discover_route` → B answers (`RouteAnswered`) → A applies (`RouteDiscovered`) → A
  `send_via_route` (route len 2) → B `FrameDelivered`. End-to-end through the loopback tap.
- **Test results:** `cargo test -p openpulse-mesh --no-default-features` → 10 loopback (9 → 10) + lib all
  pass; `cargo build --workspace` green; fmt + clippy (`--tests -D warnings`) clean.
- **Remaining (deferred):** `select_best_scored_route` now runs live, but multi-candidate selection only
  bites once several routes to a destination coexist (a candidate-route store); source-accumulated
  multi-hop paths await the wire-path TLV (#840). (The route-*maintenance* messages `RelayRouteUpdate`
  (0x07) / `RelayRouteReject` (0x08) — codec-only after this PR — got their drive next; see the
  2026-07-14 route-maintenance entry above.)

---

## 2026-07-14 — fix: record the operator callsign + declared TX power in the regulatory TX log

- **Requirement/change:** issue #830 — the engine's §97 TX-metadata log stamps `self.callsign` /
  `self.max_power_watts` on every emitted frame, but `set_callsign`/`set_max_power_watts` were only wired
  from two CLI subcommands. The daemon, ARDOP TNC, KISS TNC, and mesh daemon never set them, so their log
  recorded an **empty callsign + 0 W** on every frame.
- **Design decision:** add a `station.tx_power_watts` config field (operator-declared; the modem can't
  measure PA output; `0.0` = unspecified) and wire both values into each binary's engine at startup:
  daemon (`server::run`), KISS, and mesh set the callsign from `cfg.station.callsign`; all four set the
  power from `cfg.station.tx_power_watts`. ARDOP's operating call is the runtime host `MYID`, not config,
  so its callsign is mirrored into the engine from the `MYID` command handler instead (off the executor
  via `spawn_blocking`, matching the CONNECT/DISCONNECT rationale) — power still comes from config. Wiring
  `max_power_watts` is log-only: the audio limiter uses the separate `tx_limiter_threshold`, so no TX
  behaviour changes.
- **Implementation:** `crates/openpulse-config/src/lib.rs` (`tx_power_watts` field + default + template);
  `crates/openpulse-modem/src/engine.rs` (`callsign()` getter); `crates/openpulse-daemon/src/server.rs`,
  `crates/openpulse-kiss/src/main.rs`, `crates/openpulse-mesh/src/main.rs`,
  `crates/openpulse-ardop/src/main.rs` (startup wiring); `crates/openpulse-ardop/src/command.rs` (MYID →
  engine callsign).
- **Tests:** config `tx_power_watts_parses_for_the_regulatory_log` + default assertion in
  `missing_fields_get_defaults` (and the template round-trips through `modem_profile_loads_and_template_parses`);
  ardop `myid_mirrors_the_callsign_into_the_engine_for_the_regulatory_log`.
- **Test results:** `cargo test` for config / modem (lib) / ardop / kiss / mesh / daemon — all pass (ardop
  6 → 7 lib, config +2); `cargo fmt --all --check` + clippy (`--tests -D warnings`) clean.

---

## 2026-07-14 — fix(kiss): refuse on-air TX with no valid AX.25 source callsign (§97.119)

- **Requirement/change:** issue #830 — the KISS/AX.25 TNC (the other deferred half of audit #10) modulated
  whatever frame the host queued, with no station-ID gate. Unlike ARDOP there is no `MYID` command: a
  packet station identifies via the **AX.25 source address** in each frame, so a host defaulting to
  `N0CALL` (or a malformed frame) would produce an unidentified emission. §97.119.
- **Design decision:** gate the worker's on-air TX on the frame's own source address, reusing the shared
  `openpulse_core::station_id::callsign_is_valid`. Extract the source from the standard 14-byte address
  header (`Ax25Addr::source_from_frame`, bytes 7..14) — a format common to UI/I/S frames, so the gate does
  **not** restrict connected-mode traffic. A frame with an absent, `N0CALL`, or undecodable source is
  dropped with a warning (KISS is a bare frame pipe with no host response channel, so no FAULT surface).
  Loopback mode is unaffected.
- **Implementation:** `crates/openpulse-kiss/src/ax25.rs` (`Ax25Addr::source_from_frame`);
  `crates/openpulse-kiss/src/bridge.rs` (`tx_source_callsign` + the worker TX gate).
- **Tests:** `tx_source_callsign_gates_on_the_ax25_source` (valid → `Some`, `N0CALL`/empty/too-short →
  `None`); `worker_refuses_invalid_source_but_passes_a_valid_one` — queues an `N0CALL` frame then a
  `W1AW-9` frame and asserts exactly one frame is transmitted (proving the placeholder was processed and
  dropped, in order, not merely not-yet-reached).
- **Test results:** `cargo test -p openpulse-kiss --no-default-features` → 2 lib (0 → 2) + 9 integration
  pass; fmt + clippy (`--tests -D warnings`) clean. Completes the ARDOP/KISS MYID-before-TX line of #830.

---

## 2026-07-14 — fix(ardop): refuse on-air TX without a valid MYID (§97.119)

- **Requirement/change:** issue #830 (deferred half of audit #10) — the ARDOP TNC took its callsign from
  the host `MYID` at runtime but never gated transmission on it: with `MYID` unset (or `N0CALL`) the worker
  would still key the transmitter for host data, IRS ACK/Nack, the auto-ID frame, and relay forwards —
  unidentified emissions. §97.119 requires a station to transmit its own valid call sign. (The mesh half
  was fixed config-side in #827; a TNC can't hard-refuse at config time without breaking Pat/Winlink, so
  the gate belongs in the TNC session logic.)
- **Design decision:** add one shared validity predicate `openpulse_core::station_id::callsign_is_valid`
  (non-empty after trim, not `N0CALL`, case-insensitive) and a bridge-local `tx_callsign(&bridge) ->
  Option<String>` reading the live MYID through it. Gate every keyed-emission site on it: host data TX
  refuses + emits a `FAULT` (so the operator sees why nothing went out, instead of a silent illegal
  emission); the IRS ACK/Nack is suppressed (RX still runs); the auto-ID path now rejects `N0CALL` (was
  empty-only); relay forwarding is suppressed. Loopback mode is unaffected (no RF).
- **Implementation:** `crates/openpulse-core/src/station_id.rs` (`callsign_is_valid` + test);
  `crates/openpulse-ardop/src/bridge.rs` (`tx_callsign` + the four gates).
- **Tests:** core `callsign_validity_rejects_empty_and_placeholder`; ardop
  `tx_callsign_gates_on_a_valid_myid`, `worker_refuses_onair_data_without_myid_and_faults` (no MYID → FAULT
  + `frames_transmitted() == 0`), `worker_transmits_onair_data_once_a_valid_myid_is_set` (valid MYID → frame
  goes out, no FAULT).
- **Test results:** `cargo test -p openpulse-ardop -p openpulse-core --no-default-features` → ardop 6 lib
  (3 → 6) + 23 integration, core 272 lib + suites, all pass; `cargo build --workspace` green; fmt + clippy
  (`--tests -D warnings`) clean.
- **Follow-up (separate PR):** KISS has no callsign/MYID surface — its §97.119 identity is the AX.25 source
  address in each host frame — so its guard (refuse a frame whose source is empty/`N0CALL`) is a distinct
  change, tracked under the same #830 line.

---

## 2026-07-14 — fix(ardop): acquire the engine lock off the executor on CONNECT/DISCONNECT

- **Requirement/change:** issue #830 robustness — the ARDOP command dispatcher (`dispatch`, an async task
  per client) locked the engine `std::sync::Mutex` **inline on the async executor** for CONNECT
  (`begin_secure_session`) and DISCONNECT (`end_secure_session`). The modem worker (`worker_loop`) holds
  that same lock across a full RF TX/RX burst (`transmit_arq` / `receive_with_ack_hint` / `do_receive`),
  so an in-flight CONNECT/DISCONNECT would park an executor thread for the burst — stalling other clients'
  commands, including `ABORT`.
- **Design decision:** move the blocking lock + session call onto the blocking pool via
  `tokio::task::spawn_blocking`, exactly as the sibling `PTT TRUE`/`FALSE` handlers already do. The
  command still awaits completion before responding (ordering preserved: `CONNECTED` only after the
  handshake attempt), but the executor thread is freed while the lock is contended, so unrelated commands
  keep flowing. Poisoned-lock recovery switches from silent-skip (`if let Ok`) to
  `unwrap_or_else(|e| e.into_inner())`, matching the worker and PTT paths.
- **Implementation:** `crates/openpulse-ardop/src/command.rs` — CONNECT + DISCONNECT engine work wrapped
  in `spawn_blocking` (clone the `Arc<Mutex<ModemEngine>>` into the closure; keep `peer` for the response
  by logging a clone).
- **Tests:** `connect_holding_the_engine_lock_does_not_stall_an_abort` — a std thread holds the engine
  lock (stand-in for the worker mid-burst); a CONNECT is dispatched on a single-worker runtime, then an
  `ABORT` must complete within 3 s, observed from the (non-worker) test thread via a std channel so the
  regression fails as a clean timeout rather than a runtime deadlock. **Verified failing-first:** with the
  old inline lock the test FAILS at 3.15 s; with the fix it passes in 0.15 s.
- **Test results:** `cargo test -p openpulse-ardop --no-default-features` → 3 lib (2 → 3) + 23 integration
  pass; fmt + clippy (`--tests -D warnings`) clean.

---

## 2026-07-14 — test(daemon): cover the discovery rx-tick dwell-tee + rendezvous-connect handoffs

- **Requirement/change:** issue #830 test-coverage gap (#15) — two discovery handoffs live inline in
  `server::run`'s rx-tick `select!` arm and could be deleted with the suite green: the **dwell-audio tee
  predicate** (raw capture audio is fed to the weak-signal decoder only while dwelling) and the
  **rendezvous-connect handoff** (a completed-rendezvous QSY becomes a `ConnectPeer` for the peer). #839
  covered the third (DCD-busy beacon-defer, which lives in `discovery_tick`); these two did not.
- **Design decision:** follow the #836 precedent — extract the inline `select!` logic into small named
  helpers and unit-test them, rather than stand up a twin-harness loop. `discovery_is_dwelling(&rs)` is
  the tee predicate; `take_rendezvous_connect(&mut rs)` maps the ready `(peer, freq)` into a
  `ControlCommand::ConnectPeer` and consumes it (take-once), which the arm then feeds to
  `apply_command_to_engine`. Behaviour is unchanged — the arm now calls the helpers.
- **Implementation:** `crates/openpulse-daemon/src/server.rs` — new `discovery_is_dwelling` /
  `take_rendezvous_connect` helpers wired into the rx-tick arm; tests
  `dwelling_predicate_gates_the_dwell_audio_tee` (Inactive/None → false, drive to Dwelling → true) and
  `take_rendezvous_connect_maps_ready_peer_and_consumes_it` (None → None; ready → `ConnectPeer{W1AW}` +
  field cleared; second poll → None).
- **Tests:** the 2 new tests + the 9 existing `discovery_tick_tests`.
- **Test results:** `cargo test -p openpulse-daemon --no-default-features` → 95 lib (93 → 95) +
  integration all pass; fmt + clippy (`--tests -D warnings`) clean.

---

## 2026-07-13 — test(daemon): cover the filexfer resume→FileAccept.have_bitmap composition

- **Requirement/change:** issue #830 test-coverage gap — the daemon file-transfer *resume* path was only
  tested at the helper level (`load_partials` / `persist_block` / `partial_dir_for`). No test pre-placed
  `.blk` partials and drove the whole receive composition to check that the emitted `FileAccept` announces
  exactly the held blocks (so the sender skips them). The seeding-to-bitmap wiring could be deleted with
  the suite staying green — the repo's "wired at one seam, tested at another" class on a shipped feature.
- **Design decision:** exercise the composition through `on_offer` (the production offer entry, reached
  from `route_inbound_fragment`) rather than a helper: pre-place blocks 0 and 2 into the content-hash-keyed
  partial dir, feed an auto-accept offer, then SAR-reassemble the queued control frames and decode the
  `FileAccept`. Asserting `have_bitmap == 0b0000_0101` (bits 0 and 2, block 1 absent) is a genuine guard —
  if the resume seeding regressed, `bitmap_from_bools` would emit an empty vec and the assertion fails.
- **Implementation:** `crates/openpulse-daemon/src/filexfer.rs` test module —
  `resume_offer_announces_held_blocks_in_accept_bitmap` (test-only; no production change).
- **Tests:** the new test + the 8 existing filexfer helper/state tests.
- **Test results:** `cargo test -p openpulse-daemon --no-default-features` → 93 lib + integration all pass
  (lib 92 → 93); fmt + clippy (`--tests -D warnings`) clean.

---

## 2026-07-13 — feat(daemon): GetPttState resync query

- **Requirement/change:** issue #830 — PTT state is delivered only via edge-triggered
  `ControlEvent::PttChanged`. A client whose broadcast-ring slot lapsed (256-slot lossy channel) can
  miss a release edge and show a stale "keyed" indicator, with no way to recover the current state.
- **Design decision:** add a `GetPttState` control command that re-broadcasts the current state as a
  `PttChanged` event, so a client's existing handler resyncs with no new event type. The current state
  is `runtime_state.ptt_asserted_at.is_some()` — the watchdog arm, set on every key path (manual +
  automatic) and cleared on release, so it is the single source of truth for the logical PTT state and
  is available in `apply_command_to_engine` without the hardware controller. The command is *not* a
  direct-reply/inline command, so it falls through `handle_command`/`ws.rs` to `dispatch_command` →
  `apply_command_to_engine` identically on both transports — no inline-set parity change needed.
- **Implementation:** `crates/openpulse-daemon/src/protocol.rs` (`ControlCommand::GetPttState`),
  `crates/openpulse-daemon/src/lib.rs` (`apply_command_to_engine` arm re-emits `PttChanged`),
  `crates/openpulse-cli/src/{cli.rs,commands/daemon.rs}` (`openpulse daemon ptt-state` prints
  `{"active": …}`), `docs/cli-guide.md`.
- **Tests:** `get_ptt_state_rebroadcasts_the_current_keyed_state` — keyed → `PttChanged{active:true}`,
  unkeyed → `PttChanged{active:false}`.
- **Test results:** `cargo test -p openpulse-daemon --no-default-features` → all pass; CLI + workspace
  build clean; clippy + fmt clean.

---

## 2026-07-13 — fix(daemon): count the .partial subtree in the per-peer filexfer quota

- **Requirement/change:** issue #830 — the per-peer file-transfer disk quota under-counted. (The
  paired "block segment-id collides with the control segment at block_count=65535" claim was **verified
  a false positive**: `block_count` caps at `u16::MAX` via `u16::try_from`, so block indices run
  `0..=65534` → `segment_id ∈ [1, 65535]`, never 0 — no collision is reachable.)
- **Design decision:** `dir_size` (which `quota_would_exceed` uses) walked only the top level of a
  peer's download dir with `read_dir` + `is_file()`, so bytes held in the peer's `.partial/` **subtree**
  (in-flight resumable blocks) were never counted — a peer could accumulate unbounded data there while
  the quota reported them under limit. Make `dir_size` recurse into subdirectories (summing files at
  every depth). `DirEntry::metadata` does not traverse symlinks, so a symlinked directory reports as
  neither file nor dir and is skipped — no loop risk; `saturating_add` guards the sum.
- **Implementation:** `crates/openpulse-daemon/src/filexfer.rs` — recursive `dir_size`.
- **Tests:** `dir_size_counts_the_partial_subtree_for_quota` — a peer dir with a 100-byte top-level file
  plus a `.partial/<hash>/` subdir holding 250+150-byte blocks must total 500 (pre-fix returned only
  the 100-byte top-level file).
- **Test results:** `cargo test -p openpulse-daemon --no-default-features` → all pass; clippy + fmt
  clean.

---

## 2026-07-13 — feat(mesh): answer route-discovery requests on-air

- **Requirement/change:** issue #830 follow-up to the route-discovery driver — a mesh node must
  actually *answer* route requests, otherwise an interop partner's request floods but is never
  answered (the dead-end just moves up a layer). The mesh dispatcher fell `RouteDiscoveryRequest`
  through to the PeerQuery-only `QueryForwarder`, which now floods it but nobody replied.
- **Design decision:** give `MeshDaemon` a `RouteResponder` + `RouteTable` (the node already holds the
  Ed25519 `signing_key_seed` and a `PeerCache`). On `RouteDiscoveryRequest`, try `RouteResponder::answer`
  first; on `Some`, transmit the signed `RouteDiscoveryResponse` directed back to the originator
  (`dst = request src`) and emit `MeshEvent::RouteAnswered`; on `None`, flood as before. The responder's
  peer id is `verifying_key(seed)`, which the mesh binary also uses as `local_peer_id` (main.rs:111), so
  the answering identity matches; `capability_mask = 0` mirrors the self peer-cache entry. The
  catch-all propagate body was extracted to `propagate_query` and reused.
- **Implementation:** `crates/openpulse-mesh/src/lib.rs` — `route_responder`/`route_table` fields,
  `handle_route_discovery_request`, `propagate_query`, `MeshEvent::RouteAnswered`, dispatch arm.
- **Tests:** `crates/openpulse-mesh/tests/mesh_loopback.rs::route_discovery_destination_answers` — A
  originates a request for B's route-identity and transmits it over the loopback; B recognises itself as
  the destination, emits `RouteAnswered`, and transmits a response.
- **Test results:** `cargo test -p openpulse-mesh --no-default-features` → 9 pass (was 8); clippy + fmt
  clean. An interop partner's route request now gets a signed answer from a mesh node.

---

## 2026-07-13 — feat(core): route-discovery driver (originate / answer / apply)

- **Requirement/change:** issue #830 — the route-discovery wire messages
  (`RouteDiscoveryRequest/Response`, msg types 0x03/0x04) were **codec-only**: encode/decode existed
  but no node originated, answered, or applied them, and nothing stored a discovered route. An interop
  partner implementing the wire spec dead-ended.
- **Design decision:** add the missing driver in `openpulse-core` as pure, no-I/O protocol logic.
  Because a `WireEnvelope` carries no accumulating path trail (only a `hop_index` counter), the
  answerer builds the response `hops` from what it can vouch for: the **destination** answers with a
  single hop (itself); a node holding a **cached route** answers with that route. Signatures are
  **self-authenticating** (as in `PeerDescriptor`) — the responder signs with the Ed25519 key whose
  public bytes *are* its `peer_id`, so the originator verifies with the responder id off the reply
  envelope, no external key store. Route-request **propagation** is completed by extending
  `QueryForwarder::propagate` to accept `RouteDiscoveryRequest` (its `route_query_id` is at the same
  leading-`u64` offset as a peer query's `query_id`, so hop-limit + `(src, id)` dedup are identical).
- **Implementation:** `crates/openpulse-core/src/route_discovery.rs` (new) — `RouteTable`/`RouteEntry`
  (bounded, TTL, best-route = fewer hops then higher bottleneck reliability), `RouteOriginator`
  (`originate` → request envelope + pending tracking; `apply_response` → verify + record + consume),
  `RouteResponder` (`answer`), and `sign_route_response`/`verify_route_response`. `query_propagation.rs`
  extended to flood route requests. Registered + re-exported in `lib.rs`.
- **Tests:** 8 unit tests in the module (destination-answers, capability-decline, non-destination-none,
  tamper→BadSignature, unknown/expired-query, route-table shorter-wins + reliability-tiebreak +
  expiry) + `tests/route_discovery_integration.rs` (3: full originate→propagate→answer→apply round
  trip incl. dedup + replay rejection, hop-limit drop, intermediate-with-cached-route answers).
- **Test results:** `cargo test -p openpulse-core --no-default-features` → all pass (0 failures);
  `cargo test -p openpulse-mesh --no-default-features` → 8 pass (mesh now floods route requests via the
  extended forwarder, no regression); clippy + fmt clean. **Follow-up:** wire `RouteResponder` into the
  mesh dispatcher so a mesh node answers route requests on-air (it has the signing key + peer cache);
  tracked under #830.

---

## 2026-07-13 — test(daemon): cover the discovery DCD-busy beacon-defer guard

- **Requirement/change:** issue #830 test-coverage item #15 — the discovery beacon-emit path has a DCD
  gate (`discovery_tick`: defer the beacon when `engine.is_channel_busy()`, so the station never keys
  over an in-progress QSO), but only the *idle*-channel emit was tested; the busy-channel defer had no
  coverage and could be deleted with the suite staying green.
- **Design decision:** add a busy-channel counterpart to `discovery_tick_transmits_a_beacon_in_beacon_mode`
  with the identical beacon-mode setup, but drive the engine's DCD busy first (register BPSK, then
  `transmit` + `receive` on the loopback so the echoed energy trips DCD — nothing in `discovery_tick`
  feeds the DCD, so the busy state persists across ticks). Assert every beacon-due tick returns `None`.
  Non-vacuous: the companion emit test proves a beacon *is* due at those slots, so removing the gate
  would make the tick return `Some` and fail this test.
- **Implementation:** `crates/openpulse-daemon/src/server.rs` — new
  `discovery_tick_defers_a_due_beacon_when_the_channel_is_busy`.
- **Tests:** the new test; `discovery_tick` group → 9 pass (was 8).
- **Test results:** `cargo test -p openpulse-daemon --no-default-features` → 113 pass; clippy + fmt
  clean. Remaining #15 handoffs that live in `server::run`'s `select!` loop rather than `discovery_tick`
  (the dwell-audio tee predicate and the rendezvous-connect handoff) still want a twin-harness,
  loop-driven test — left open under #830.

---

## 2026-07-13 — docs: cli-guide daemon/FF-15/FF-16 CLI + README hpx_hf ladder row

- **Requirement/change:** issue #830 docs items #44 and #45. (#44) `docs/cli-guide.md` was frozen at
  v0.2.0 and documented none of the `openpulse daemon` control CLI — including the FF-15 discovery and
  FF-16 file-transfer commands — yet the traceability matrix cites it as covering REQ-UX-02. (#45) the
  README `hpx_hf` adaptive-profile row still named the pre-(SC-FDMA→OFDM re-seat) ladder.
- **Design decision:** document the daemon CLI grouped by function (connection/PTT/mode, messaging/OTA,
  runtime toggles) with explicit FF-15 (`enable-discovery`/`disable-discovery`/`stations`/`peers`) and
  FF-16 (`send-file`/`accept-file`/`reject-file`/`cancel-file`/`files`) sections, plus the six missing
  top-level subcommands (`monitor`/`broadcast`/`beacon`/`qsy`/`calibrate`/`config`). Args taken from
  the clap derives in `crates/openpulse-cli/src/cli.rs` (verified: `daemon --addr` default
  `127.0.0.1:9000`; `broadcast --payload`; `beacon --callsign` required). Also corrected the stale
  `--pki-url` default (`localhost:8080` → `127.0.0.1:8787`) and bumped the doc's version/date stamp.
- **Implementation:** `docs/cli-guide.md` (new "Daemon control CLI" section + top-level additions +
  stamp/URL fixes); `README.md` `hpx_hf` row → `SL2–SL17` / top mode `OFDM52-64QAM` (matching
  `crates/openpulse-core/src/profile.rs::hpx_hf`, SL2–SL17, SL17 = `OFDM52-64QAM`).
- **Tests:** `bash scripts/validate-doc-frontmatter.sh` → exit 0. CLI surface cross-checked against the
  clap `enum Commands`/`enum DaemonCommands` definitions.

---

## 2026-07-13 — fix(gpu): calibrate the 64QAM + psk8 GPU soft-demod LLRs

- **Requirement/change:** issue #830 DSP-calibration follow-up — the GPU soft-demod paths emitted
  σ²=1 (uncalibrated) LLRs, so on a GPU-equipped station HARQ combining over-weighted deep-fade
  attempts even after the CPU paths were fixed (#833/#835). (OFDM has no `gpu` feature; SC-FDMA's GPU
  path computes LLRs on CPU — only 64QAM and psk8 use the shared σ²=1 `gpu_soft_demod` kernel.)
- **Design decision:** `openpulse_gpu::gpu_soft_demod` returns raw max-log-MAP squared-distance
  differences (`min1 − min0`); the CPU paths turn these into true LLRs by dividing by the noise
  variance (`symbol_llrs`'s `1/σ²`, and psk8's explicit `1/(2σ²)`). Apply the identical scaling to the
  GPU output using the *same* estimator each CPU path uses — 64QAM: the corner-preamble residual
  `preamble_noise_var` (#833), psk8: `psk_symbol_noise_var` — so the GPU result is calibrated by
  construction and equals the CPU result.
- **Implementation:** `plugins/64qam/src/demodulate.rs` (`qam64_demodulate_soft_gpu`) and
  `plugins/psk8/src/demodulate.rs` (`psk8_demodulate_soft_gpu`) — scale the returned LLRs by the
  shared `1/noise_var` before returning. Both behind `#[cfg(feature = "gpu")]`.
- **Tests:** `gpu_soft_llrs_match_calibrated_cpu` in each plugin — a `#[cfg(feature = "gpu")]`,
  `#[ignore = "requires a GPU adapter"]` equivalence test that (when an adapter exists) asserts the GPU
  soft LLRs match the CPU soft LLRs within f32 tolerance. Follows the repo's existing GPU-test
  convention (`GpuContext::init()` → skip when absent).
- **Test results:** `cargo test -p qam64-plugin -p psk8-plugin --no-default-features --features gpu` →
  compiles and passes (the two equivalence tests report *ignored, requires a GPU adapter* in this
  headless environment — they run on GPU hardware/CI). clippy clean in both feature states; default
  (no-`gpu`) suites unaffected. Runtime verification pending a wgpu adapter.

---

## 2026-07-13 — fix(64qam): calibrate soft-LLR noise variance from the corner preamble

- **Requirement/change:** issue #830 DSP-calibration item — 64QAM soft LLRs under-report σ² at
  moderate SNR, so their magnitude is over-confident. Single-frame decode is scale-invariant and
  unaffected, but MAP HARQ combining (`combine_llrs_map`) over-weights a deep-fade attempt whose LLRs
  claim a certainty they do not have. Same class as the SC-FDMA `mmse_llr_noise_var` fix (#690).
- **Design decision:** `qam64_demodulate_soft` measured `noise_var` with `estimate_decision_noise_var`
  (mean squared distance to the *nearest* constellation point). On the dense 8×8 grid that saturates:
  once a symbol crosses a decision boundary its distance is taken to the wrong-but-near point, so the
  estimate reads σ² far too low. The plugin already carries a known, constant-modulus **corner
  preamble** (16 symbols, level- and carrier-corrected on the constellation scale), so its mean
  squared deviation from `preamble_symbols()` is an unbiased 2-D noise variance at any SNR — the same
  unit `symbol_llrs` expects. Fall back to the decision-directed estimate only if no preamble symbols
  are present. Scoped to the CPU path; the `gpu` soft path emits σ²=1 (uncalibrated) LLRs regardless
  and is untestable without the (CI-off) `gpu` feature — noted in #830 as a follow-up.
- **Implementation:** `plugins/64qam/src/demodulate.rs` — new `preamble_noise_var()`; used in
  `qam64_demodulate_soft`.
- **Tests:** `plugins/64qam/tests/llr_reliability.rs` (new) — bins emitted `|L|` and compares the
  empirical bit-error rate against `1/(1+e^{|L|})`, asserting the worst bin is ≤ 4× (the max-log-MAP
  approximation's own optimism). Mirrors `plugins/scfdma/tests/llr_reliability.rs`.
- **Test results:** before the fix the gate failed at **24×** over-confident (64QAM500 @10 dB, |L|≈11).
  After: worst 1.78× (10 dB), 1.35× (12 dB), 1.93× (14 dB) for 64QAM500; 64QAM2000-RRC reads slightly
  *under*-confident (0.35× / 0.14×), the safe direction. `cargo test -p qam64-plugin
  --no-default-features` → all pass; `cargo test -p openpulse-modem --no-default-features --test
  llr_calibration` → 2 passed (mean|LLR| still grows with SNR — real calibration, not suppression);
  clippy + fmt clean.

---

## 2026-07-13 — fix(ofdm): calibrate soft-LLR noise from pilots + drop the ZF double-count

- **Requirement/change:** issue #830 DSP-calibration items — OFDM soft LLRs are over-confident (σ²
  under-read at moderate SNR) **and** double-count the ZF noise enhancement. Both corrupt MAP HARQ
  combining, where an over-confident deep-fade attempt out-votes a clean one. Same class as SC-FDMA
  #690 / 64QAM #833.
- **Design decision:** the old per-SC noise was `block_noise · mean|H|²/|H_k|²`, where `block_noise`
  = `estimate_decision_noise_var` measured on the *post-ZF* data. That estimator saturates on the
  dense QAM grid (distance to the wrong-but-near point), and being measured after ZF it already
  carries the `1/|H_k|²` blow-up — which the code then applied a *second* time. Replace it with the
  physically-correct model: (1) a frequency-domain per-bin additive noise `σ²_bin` (2-D `E|N_k|²`,
  white so channel-independent) measured from the **pilots across symbols** — `Y_p[s+1]−Y_p[s]`
  cancels the constant `H_p`, leaving `2σ²_bin`, non-saturating because it uses no decisions; (2) a
  channel-estimate-error term — `Ĥ_k` is interpolated from one-pilot-each LS estimates, so
  `X_k = Y_k/Ĥ_k` gains a signal-power term, giving `noise_var_k = σ²_bin·(1+P_c)/|H_k|²` with `P_c`
  = the (unit) constellation power and the conservative `σ²_ce ≈ σ²_bin`. This is a *single* `1/|H|²`
  and the OFDM analogue of the SC-FDMA `mmse_llr_noise_var` term. A one-symbol frame (no pilot
  difference) keeps the legacy estimate.
- **Implementation:** `plugins/ofdm/src/demodulate.rs` — `demodulate_soft_with_params` restructured
  into an FFT/deramp pass that keeps the frequency frames, a `pilot_noise_var` estimate, and an LLR
  pass; new `pilot_noise_var()` + `points_avg_power()` (reusing the already-`pub` `pilot_positions`).
- **Tests:** `plugins/ofdm/tests/llr_reliability.rs` (new) — bins `|L|` vs the empirical error rate on
  flat and in-CP two-ray channels, asserting worst-bin over-confidence ≤ 4× (max-log-MAP's own
  optimism). Mirrors the SC-FDMA / 64QAM gates.
- **Test results:** before, the gate failed badly — **50.7×** over-confident (OFDM52-16QAM flat @10),
  36× (64QAM flat @16), and **90–171×** on two-ray. After: every case is *under*-confident
  (0.09–0.47×), the safe direction. `cargo test -p ofdm-plugin --no-default-features` → 36 pass;
  `openpulse-modem` `llr_calibration` (2), `llr_combining_gain` (2), `llr_convention_conformance` (1)
  all pass — mean|LLR| still grows with SNR (real calibration, not suppression). clippy + fmt clean.

---

## 2026-07-13 — fix(pilot): calibrate soft-LLR noise from the pilot/preamble residual

- **Requirement/change:** issue #830 DSP-calibration item — the pilot-framed plugin's soft LLRs are
  over-confident. Same class as SC-FDMA #690 / 64QAM #833 / OFDM #834; matters for MAP HARQ combining.
- **Design decision:** `symbols_to_llrs` used a decision-directed `noise_var` (distance to the nearest
  point) over the recovered data symbols, which saturates on the dense 16QAM/32APSK grids — a new
  gate measured **60–1599×** over-confident at 4–8 dB (the SNRs where errors appear). The waveform
  already carries a fully-known BPSK preamble and sparse data-region pilots, so measure the additive
  noise from their residual instead: the amplitude-normalised known symbol minus its reference, over
  the settled preamble (skip the first half — acquisition transient) + all data pilots. That is
  decision-free, so it does not saturate. The pilot residual alone still under-read the *data*-symbol
  scatter ~1.7–1.8×, because the data symbols are additionally de-rotated and amplitude-normalised by
  a phase/amplitude reference estimated from those same noisy pilots — the single-carrier analogue of
  the OFDM channel-estimate-error term. With the conservative `σ²_est ≈ σ²` that is a `(1+P_c)` factor
  (`P_c` = average constellation power). A frame too short for ≥8 known symbols keeps the legacy
  decision-directed estimate.
- **Implementation:** `plugins/pilot/src/frame.rs` — `recover_data_syms` now also returns the
  pilot/preamble-residual noise; `symbols_to_llrs` takes it and applies the `(1+P_c)` factor, with the
  decision-directed path as fallback. `decode`/`decode_soft` updated for the new tuple return.
- **Tests:** `plugins/pilot/tests/llr_reliability.rs` (new) — bins `|L|` (wide edges, since the broken
  demod drives `|L|` into the hundreds) vs the empirical error rate, asserting worst-bin
  over-confidence ≤ 4×. Mirrors the SC-FDMA/64QAM/OFDM gates.
- **Test results:** before, **60–1599×** over-confident (errors sat at `|L|≈14` claiming ~1e-7 error).
  After, every error-bearing bit is at `|L|≈5` with empirical ≈ predicted (ratio 0.3–0.4, slightly
  under-confident — the safe direction) and the high-`|L|` bits are genuinely near-error-free.
  `cargo test -p pilot-plugin --no-default-features` → all pass (incl. `soft_fec_loopback`);
  `openpulse-modem` `llr_calibration` (2) + `llr_convention_conformance` (1) pass — mean|LLR| still
  grows with SNR. clippy + fmt clean.

---

## 2026-07-13 — test(daemon): cover the command-path PTT hardware-failure guard

- **Requirement/change:** issue #830 test-coverage item — the `ptt_hard_failed` skip-dispatch guard on
  the daemon command path was unverified. When a manual `PttAssert`/`PttRelease` hits a stuck/absent
  rig, the daemon must skip the engine dispatch so it does not emit a `PttChanged` claiming a PTT
  state the hardware never reached. #817 added a failing-PTT double for the beacon helper, but the
  command-path guard had no test because the logic was inline in `server::run`'s `select!` loop.
- **Design decision:** extract the inline `PttAssert`/`PttRelease` hardware call + `ptt_hard_failed`
  computation into `handle_ptt_command(cmd, ptt) -> bool` (behavior-identical; the `select!` arm now
  calls it), so the guard is unit-testable without standing up the whole async loop. Extend the
  existing `FlakyPtt` double with a `fail_assert` flag (it only failed release before).
- **Implementation:** `crates/openpulse-daemon/src/server.rs` — new `handle_ptt_command`; the command
  arm of the `select!` loop calls it; `FlakyPtt` gains `fail_assert` (and `#[derive(Default)]`).
- **Tests:** `ptt_command_guard_reports_hardware_failure_to_skip_dispatch` — a failed assert and a
  failed release each report `true` (caller skips dispatch); a successful assert/release reports
  `false` (dispatch proceeds); a non-PTT command and the no-controller case report `false`.
- **Test results:** `cargo test -p openpulse-daemon --no-default-features` → 112 pass (incl. the new
  test and the unchanged `automatic_tx_arms_the_watchdog…` beacon test); clippy + fmt clean.

---

## 2026-07-11 — test(discovery): FF-15 Phase F-3c-iv — two-runtime end-to-end rendezvous

- **Requirement/change:** an acceptance test that two independent stations actually reach a rendezvous
  over the air, not just each seam in isolation.
- **Design decision:** a real-time twin daemon would take minutes (JS8's 15 s UTC slots × the Propose +
  Accept + switch overs), so `crates/openpulse-discovery/tests/rendezvous_end_to_end.rs` instead shuttles
  the **actual GFSK audio** each `DiscoveryRuntime` transmits into the other's capture buffer under a
  manual clock: initiator `Propose` over → responder decodes + agrees → responder `Accept` over →
  initiator decodes + agrees, on the same channel index. Exercises the full stack (initiator session ↔
  directed TX framing ↔ GFSK modem ↔ RX reassembler ↔ responder decision) across two instances.
- **Implementation:** `crates/openpulse-discovery/tests/rendezvous_end_to_end.rs`.
- **Tests:** happy path (both agree on channel 1; initiator session concludes); no-common-channel path
  (responder does not agree, sends a Reject over, initiator surfaces `RendezvousRejected`).
- **Test results:** `cargo test -p openpulse-discovery --no-default-features --test rendezvous_end_to_end`
  → 2 passed; clippy clean. **Phase F (rendezvous → handoff) is functionally complete**; only on-air
  validation (Phase H) remains, environment-gated.

## 2026-07-11 — feat(daemon): FF-15 Phase F-3c-iii — rendezvous QSY + CONREQ handoff

- **Requirement/change:** complete the rendezvous flow — after an agreement, both stations QSY to the
  working frequency and start the signed HPX session. The QSY must wait `switch_in_slots` (so the Accept
  is heard and both retune together), then reuse the existing signed-connect path.
- **Design decision:** on `RendezvousAgreed`, `discovery_tick` schedules `rendezvous_qsy_due = (peer,
  freq_hz, now + switch_in_slots × 15 s)` (NORMAL slot) rather than QSYing immediately — the Accept keeps
  transmitting via the runtime's priority queue meanwhile. When the deadline passes, `discovery_tick`
  clears the discovery home (so stand-down does **not** tune back), CAT-retunes to the working frequency,
  stands discovery down, and arms `rendezvous_connect_ready`. `server::run` takes that and runs
  `apply_command_to_engine(ConnectPeer { callsign: peer })` — the same `begin_secure_session` + CONREQ-
  over-RF path an operator connect uses (it owns `&mut engine` + the rig). Gated behind the existing
  Full-mode/callsign/clock TX guards + an explicit `RendezvousWith` or Full-mode responder — no unbidden
  transmission.
- **Implementation:** `crates/openpulse-daemon/src/{lib,server}.rs` (`RuntimeControlState` gains
  `rendezvous_qsy_due` + `rendezvous_connect_ready`; `JS8_NORMAL_SLOT_MS`).
- **Tests:** the responder `discovery_tick` test extended — after agreement a QSY is scheduled; once the
  switch delay elapses the daemon retunes to the working frequency, arms the handoff, consumes the
  schedule, and stands discovery down.
- **Test results:** `cargo test -p openpulse-daemon --no-default-features discovery_tick` → 6 passed;
  clippy clean. F-3c-iv (twin-daemon end-to-end) next.

## 2026-07-11 — feat(daemon): FF-15 Phase F-3c-ii — rendezvous command, events, bandplan gate

- **Requirement/change:** expose the rendezvous runtime through the daemon control surface — an operator
  command to initiate, events for the outcome, the responder's per-band channel wiring, and a startup
  bandplan check of the configured working channels.
- **Design decision:** `ControlCommand::RendezvousWith { callsign }` → `start_rendezvous_cmd` resolves the
  current band's channel indices, mints a 2-char base-36 token, and calls `runtime.start_rendezvous`
  (errors surfaced via `CommandError` when discovery is unconfigured, has no channels for the band, or is
  not TX-capable). `discovery_tick` sets the responder's available channels from the home band each tick
  and maps the runtime's `RendezvousAgreed/Rejected/TimedOut` outcomes to new
  `ControlEvent::RendezvousAgreed { peer, freq_hz }` / `RendezvousFailed { peer, reason }` (resolving the
  agreed **index**→Hz from the dwelling band's table). `validate_rendezvous_channels` logs a bandplan
  warning per out-of-segment channel at startup (advisory — honoured, not rejected). The QSY + CONREQ
  handoff is **F-3c-iii** (coupled to the `switch_in_slots` delay), so this step stops at surfacing the
  agreement.
- **Implementation:** `crates/openpulse-daemon/src/{protocol,lib,server}.rs`; `RuntimeControlState`
  gains `discovery_rendezvous_channels_hz` (populated from config in `server::run`).
- **Tests:** command JSON round-trips; `start_rendezvous_cmd` happy path + 3 error paths (unconfigured /
  no-channels / no-callsign); `discovery_tick` responder decodes a Propose over and emits
  `RendezvousAgreed` at the resolved Hz.
- **Test results:** `cargo test -p openpulse-daemon -p openpulse-config --no-default-features` → all green
  (6 new); `cargo build --workspace --no-default-features` clean (shared-enum exhaustive matches);
  clippy clean.

## 2026-07-11 — feat(discovery): FF-15 Phase F-3c-i — rendezvous runtime orchestration

- **Requirement/change:** the `DiscoveryRuntime` must drive both rendezvous roles end-to-end (still pure,
  no I/O) so the daemon glue stays thin: propose to a peer, respond to inbound proposals, and surface the
  agreement/reject/timeout the daemon acts on.
- **Design decision:** embed a `RendezvousAssembler` (fed every decode alongside the hint assembler) and
  an optional `RendezvousInitiator`. New outcomes `RendezvousAgreed { peer, channel, switch_in_slots }`
  (channel is a table **index** — the daemon resolves Hz + does the QSY/CONREQ), `RendezvousRejected`,
  `RendezvousTimedOut`. `start_rendezvous()` queues the Propose over into a **priority** TX queue that
  `maybe_transmit` drains before beacons and without the cadence gate; the responder role (`respond()`
  → Accept/Reject over) is gated to `TxMode::Full`; the initiator times out per-slot. Reused
  `TransmitBeacon` for rendezvous audio (the daemon path is identical). `set_rendezvous_channels()` takes
  the current band's available indices — no `DiscoveryParams` churn.
- **Implementation:** `crates/openpulse-discovery/src/runtime.rs`.
- **Tests:** responder accepts highest-ranked common channel + transmits the Accept; rejects with no
  common channel; RX-only/Beacon does not answer; initiator proposes → agrees on Accept; initiator
  reports a reject; initiator times out. (6 new; 50 crate tests total.)
- **Test results:** `cargo test -p openpulse-discovery --no-default-features` → 50 passed; clippy clean.
  F-3c-ii (daemon `RendezvousWith` command, QSY, CONREQ handoff, bandplan gate) + F-3c-iii (twin) next.

## 2026-07-11 — feat(discovery): FF-15 Phase F-3b — rendezvous RX reassembler

- **Requirement/change:** an inbound rendezvous over must be reassembled from its per-slot JS8 frames and
  surfaced only when it is addressed to us.
- **Design decision:** `openpulse-discovery/src/rendezvous_assembler.rs` — `RendezvousAssembler` mirrors
  `HintAssembler` (per-offset buffering, `First`-reset, stale sweep) but keys the `CompoundDirected`
  target on **our own callsign** and parses the reassembled Huffman text as a `RendezvousMsg` instead of
  a capability hint. Returns `RecognizedRendezvous { from, grid, msg, base_freq_hz }`.
- **Implementation:** `crates/openpulse-discovery/src/rendezvous_assembler.rs`; exports in `lib.rs`.
- **Tests:** `directed()` (F-3a) builds the frames the assembler consumes — Propose addressed to us
  recognised; an over to another station ignored; Accept + Reject both decode; non-rendezvous free text
  to us not recognised.
- **Test results:** `cargo test -p openpulse-discovery --no-default-features rendezvous_assembler::` →
  4 passed; clippy clean. F-3c (daemon orchestration + QSY + CONREQ handoff + twin test) next.

## 2026-07-11 — feat(js8): FF-15 Phase F-3a — directed free-text TX builder

- **Requirement/change:** a rendezvous message rides as a JS8 **directed free-text over** to the peer.
  The beacon layer only built the `@OPULSE` group hint; add a general directed builder.
- **Design decision:** generalise `opulse_hint` — extract `build_over(sender, grid, target, text)` and add
  `beacon::directed(sender, grid, to, text)`, which targets a **callsign** in the `CompoundDirected`
  frame (`opulse_hint` now just calls `build_over` with `@OPULSE`). Structure is identical: `Compound`
  (sender+grid) + `CompoundDirected`(to) + Huffman `Data` frames, `First`/`Last`-bracketed.
- **Implementation:** `plugins/js8/src/beacon.rs` (`directed`, `build_over`); export in `lib.rs`.
- **Tests:** `directed_over_carries_sender_target_and_free_text` — sender + target callsign decode from
  the compound frames and the Huffman data frames reassemble to the original OPHF text.
- **Test results:** `cargo test -p js8-plugin --no-default-features --lib beacon::` → 3 passed; clippy clean.

## 2026-07-11 — feat(config): FF-15 Phase F-2 — rendezvous working-channel table

- **Requirement/change:** a `Propose` carries channel **indices**, not Hz (F-1) — so both stations need
  a shared per-band index→frequency table. Add it to config with sensible defaults.
- **Design decision:** `DiscoveryConfig.rendezvous_channels_hz: BTreeMap<String, Vec<u64>>` (band label →
  ordered working frequencies; the `Vec` position is the on-air channel index). `rendezvous_channel_hz(band,
  index)` resolves it. Defaults sit in each band's data/ARQ segment, clear of the JS8 calling frequency.
  Bandplan validation of these entries lands at daemon startup in F-3 (config has no `openpulse-qsy` dep).
- **Implementation:** `crates/openpulse-config/src/lib.rs` — field, `Default`, template `[discovery.
  rendezvous_channels_hz]`, `rendezvous_channel_hz()` resolver.
- **Tests:** `discovery_defaults_are_opt_in_and_rx_only` extended — index-in-range, out-of-range → `None`,
  unknown-band → `None`, and the template round-trips the table.
- **Test results:** `cargo test -p openpulse-config --lib --no-default-features` → 16 passed; clippy clean.

## 2026-07-11 — feat(discovery): FF-15 Phase F-1 — rendezvous protocol (codec + session)

- **Requirement/change:** with discovery + beacon TX done, start Phase F — negotiate a working
  frequency with a discovered `@OPULSE` peer over JS8, then hand off to the signed HPX handshake
  (plan §5.3 / decision D3). This unit is the pure protocol foundation; no I/O.
- **Design decision:** `openpulse-discovery/src/rendezvous.rs` — a 2-message exchange (`Propose` /
  `Accept` / `Reject`) carried as JS8 OPHF free text, with channel **indices** into a per-band table
  (not Hz) so a proposal fits ~2 frames, and **no signature** (the post-QSY signed CONREQ is the
  auth). `RendezvousMsg` codec + `RendezvousInitiator` (propose → accept/reject/timeout → `Qsy`) +
  `respond()` (highest-ranked common channel, else `NoCommonFreq`). Timeouts live here.
- **Implementation:** `crates/openpulse-discovery/src/rendezvous.rs`; exports in `lib.rs`.
- **Tests:** codec round-trip + non-rendezvous rejection (incl. distinguishing from the `OPHF1`
  capability hint); responder channel selection / reject; initiator accept→Qsy / timeout / reject.
- **Test results:** `cargo test -p openpulse-discovery --no-default-features` → all green (7 new); fmt
  + clippy (`-D warnings`) clean. F-2 (channel table) + F-3 (daemon wiring, QSY, CONREQ handoff) next.

## 2026-07-11 — feat(discovery/daemon): FF-15 Phase E-4/5/6 — beacon TX scheduler + daemon wiring

- **Requirement/change:** with the seam (E-3) in place, schedule + emit beacons and honor
  `[discovery] mode = "beacon"` — completing Phase E. Off by default; TX is gated.
- **Design decision:** `DiscoveryRuntime` gains a `TxMode` + beacon params and a `maybe_transmit`
  slot scheduler — on a slot boundary, if opted-in and the clock is within `max_clock_skew_ms`,
  emit the next beacon frame (heartbeat on cadence; every Nth is an `@OPULSE` hint), half-duplex
  (a TX slot skips RX). It returns a `TransmitBeacon { audio, mode }` outcome. `discovery_tick`
  applies the **direct DCD gate** (not the 0.3-persistence CSMA, which would break slot alignment)
  and returns the due beacon; `server::run` transmits it with `transmit_beacon_with_ptt` (PTT wrap +
  `engine.transmit_raw_audio`). `build_discovery_runtime` maps `mode`/callsign/grid/hint/intervals
  from config; the `[discovery] mode` field is now honored (warn removed).
- **Implementation:** `crates/openpulse-discovery/src/runtime.rs` (`TxMode`, params, scheduler,
  `TransmitBeacon`); `crates/openpulse-daemon/src/server.rs` (DCD gate, `transmit_beacon_with_ptt`,
  `build_discovery_runtime`); `crates/openpulse-config/src/lib.rs` (mode doc); CLAUDE.md.
- **Tests:** `runtime::beacon_mode_transmits_a_heartbeat_on_cadence`, `runtime::rx_only_never_transmits`;
  `server::discovery_tick_transmits_a_beacon_in_beacon_mode` (drives beacon mode → `transmit_raw_audio`,
  `raw_audio_frames_transmitted == 1`).
- **Test results:** discovery + daemon suites green; fmt + clippy (`-D warnings`) clean. **Phase E
  (beacon TX) complete — off-by-default; on-air TX requires the operator to set `mode = "beacon"`, a
  callsign, real hardware/PTT, and to meet §97.221 in their jurisdiction.**

## 2026-07-11 — feat(js8): FF-15 Phase E-2 — beacon assembly + full TX→RX loopback

- **Requirement/change:** with the TX packers (E-1) in place, build the beacon frame sequences and
  synthesise their audio, and prove the discovery loop closes OpenPulse-to-OpenPulse.
- **Design decision:** `plugins/js8/src/beacon.rs` — `heartbeat()` (one self-identifying `@HB` frame),
  `opulse_hint()` (the `Compound`(sender+grid) + `CompoundDirected`(`@OPULSE`) + Huffman `Data` over,
  `First`/`Last`-bracketed), and `frame_audio()` (one frame → info bits → tones → GFSK). Added
  `pack_huff_frame` (E-1's remaining packer) to `encode.rs`. **Still no transmit path is wired** — the
  scheduler + engine seam (E-3/E-4) come next; this only builds bytes/audio in memory.
- **Implementation:** `plugins/js8/src/{beacon.rs,encode.rs}`; exports in `lib.rs`.
- **Tests:** `encode::huff_frame_matches_upstream_and_round_trips` (== Qt5 ground truth);
  `beacon::heartbeat_beacon_transmits_and_decodes_off_air`; `beacon::opulse_hint_builds_a_first_last_bracketed_over`;
  and the milestone `openpulse-discovery/tests/beacon_loopback.rs::opulse_beacon_transmits_and_is_recognized_end_to_end`
  — mint a hint via `encode_hint` → `opulse_hint` → per-frame `frame_audio` → `decode_window` →
  `HintAssembler` → recognised peer (DC0SK/JN58, caps 0xB105).
- **Test results:** js8-plugin + discovery suites green; fmt + clippy (`-D warnings`) clean.

## 2026-07-11 — feat(js8): FF-15 Phase E-1 — JS8 TX packers (beacon foundation)

- **Requirement/change:** Phase E (beacon TX) — the §97.221 doc gate (#790) is cleared. The TX side
  needs the encode counterparts of the RX decoders. This unit is the foundation; it emits nothing.
- **Design decision:** port `packAlphaNumeric50` (50-bit compound callsign) and `packCompoundFrame`
  (heartbeat/compound/compound-directed 72-bit payloads) into `plugins/js8/src/encode.rs`, plus a
  `pack_heartbeat_frame` convenience. Validated against the same Qt5 ground truth as the decoders and
  round-tripped through them. **Off-path:** these are pure functions; no transmit path is wired, so
  nothing keys a radio.
- **Implementation:** `plugins/js8/src/encode.rs`; exports in `lib.rs`.
- **Tests:** `pack_alphanumeric50_matches_upstream_and_round_trips`,
  `heartbeat_frame_matches_upstream_ground_truth` (== the RX ground-truth payload),
  `packed_heartbeat_decodes_back_to_sender_and_grid`, `opulse_group_packs_and_round_trips`,
  `directed_and_data_flags_are_rejected`.
- **Test results:** `cargo test -p js8-plugin --no-default-features --lib` → all green (5 new); fmt +
  clippy (`-D warnings`) clean.

## 2026-07-11 — docs/test: low-priority audit closeout (G-2/G-5/G-7 + serial PTT)

- **Requirement/change:** G-2 — `transmit_iq` bypasses the emit seam (no reg-log/frame-count/auto-ID),
  undocumented. G-5 — the KISS TNC runs no auto-ID timer (undocumented whether intentional). G-7 —
  guarded `unwrap()`s in the modem soft/hard FEC split, uncommented. H2-serial — `SerialRtsDtrPtt` was
  never instantiated in a test.
- **Design decision:** document `transmit_iq` as a seam-bypassing experimental IQ path (test-only, not
  for on-air); document that the KISS TNC self-identifies via the AX.25 source-callsign address field
  (§97.119) so it needs no separate ID cycle; add one comment stating the producer↔arm invariant that
  makes the FEC-split unwraps safe; add a `SerialRtsDtrPtt::open` error-path test (feature-gated to
  match the backend).
- **Implementation:** `crates/openpulse-modem/src/engine.rs` (`transmit_iq` doc + FEC-split comment);
  `crates/openpulse-kiss/src/lib.rs` (station-ID note); `crates/openpulse-radio/src/serial.rs`
  (`#[cfg(all(test, feature = "serial"))]` test).
- **Tests:** `serial::tests::open_on_a_nonexistent_device_errors` (under `--features serial`).
- **Test results:** modem + kiss clippy clean (`-D warnings`); radio `--features serial` clippy clean +
  serial test passes. **All audit findings are now addressed** (see the audit artifact).

## 2026-07-11 — test: module coverage — profile/fading/pq/panel + pki honesty (audit H3/H4/H8/H9/H10)

- **Requirement/change:** several sizeable modules had no inline tests: `profile.rs` (ladders),
  `fading.rs` (Doppler/analytic primitives — the Watterson-bug area), `pq_handshake.rs` (error
  branches), panel `app.rs` (the Message→ControlCommand action layer); and the pki API tests reported
  green without a DB (inflated coverage).
- **Design decision:** add behavioral tests, not smoke tests. Profile: a cross-crate test that
  registers all plugins and asserts every ladder rung resolves (catches a mode-string typo — the
  `#444→#445` regression class). Fading: `analytic_signal` holds a constant envelope + recovers the
  real part (the property the Watterson quadrature fix relies on), and `doppler_envelope` is
  power-normalised + varies. PQ: decode entry points return `SerializationError` (not panic) on
  malformed bytes. Panel: inject a command channel, assert UI actions dispatch the right commands.
  PKI: make the DB-gated skip unmistakable + document that a green run without the DB is not coverage.
- **Implementation:** `crates/openpulse-modem/tests/profile_modes_resolve.rs`;
  `crates/openpulse-channel/src/fading.rs` (inline tests); `crates/openpulse-core/src/pq_handshake.rs`
  (inline tests); `apps/openpulse-panel/src/app.rs` (inline dispatch test);
  `pki-tooling/tests/api_flow.rs` (module doc + skip message).
- **Tests:** the 5 new tests above.
- **Test results:** modem/channel/core/panel suites green; fmt + clippy (`-D warnings`) clean.

## 2026-07-11 — test(daemon): control-command handler dispatch tests (audit H1)

- **Requirement/change:** ~22 of 39 `ControlCommand` handlers were never dispatched in any test — the
  daemon's operator control surface. Highest-risk were the cross-cutting RX/TX front-end toggles
  (`SetNotch`/`SetAgc`/`SetCessb`) whose seam-gaps are exactly what the CLAUDE.md warnings target.
- **Design decision:** add dispatch tests through the real `apply_command_to_engine` that assert the
  observable effect (engine state / emitted event), not just serde round-trips, for the hot-spot arms.
- **Implementation:** `crates/openpulse-daemon/src/lib.rs` `command_apply_tests` — `apply` helper +
  `front_end_toggle_commands_reach_the_engine` (notch/agc/cessb flip engine state),
  `set_dcd_squelch_rejects_invalid_threshold` (CommandError path), `ptt_commands_track_state_and_emit_changed`,
  `ota_set_level_bounds_emits_status`. (With the earlier `list_files` + discovery + OTA
  lock/hysteresis/aggressiveness tests, the handler hot spots are now covered; remaining untested arms
  — file accept/reject/cancel dispatch, needing an active transfer — are lower value.)
- **Tests:** the four new tests above.
- **Test results:** `cargo test -p openpulse-daemon --no-default-features command_apply_tests` →
  38 passed; fmt + clippy (`-D warnings`) clean.

## 2026-07-11 — docs: §97.221 automatic-control compliance design (audit G-3 / REQ-REG-04)

- **Requirement/change:** REQ-REG-04 (§97.221 automatic-control documentation) was an open ⚠ gap and
  the hard prerequisite for JS8 FF-15 Phase E beacon TX. No document mapped OpenPulse's automatic-TX
  safeguards to the rule.
- **Design decision:** add an "OpenPulse automatic-control design" subsection to `docs/regulatory.md`
  §97.221 — a table mapping each rule requirement (control-point termination, off-by-default/operator
  turn-off, ≤100 W, third-party authorization, §97.119 ID) to the OpenPulse mechanism and the
  operator's residual responsibility, plus the JS8-beacon-specific safeguards (rate limits, ±2 s
  clock-skew TX refusal, CSMA deferral). Framed explicitly as engineering documentation, not legal
  advice — the control operator verifies for their jurisdiction.
- **Implementation:** `docs/regulatory.md` (subsection + summary-table row + date);
  `docs/dev/project/traceability-matrix.md` (REQ-REG-04 moved out of the gap list).
- **Tests:** docs-only.
- **Test results:** N/A. Note: this document is the *prerequisite* for Phase E; it does not enable any
  TX by itself.

## 2026-07-11 — ci: restore auto-triggers + macOS build + pre-push fmt (audit TR-01/TR-02)

- **Requirement/change:** the audit found every workflow set to `workflow_dispatch`-only (CI gates
  never ran on push/PR, set in commit 5c93ca2), macOS silently dropped from CI, and the pre-push hook
  reduced to `cargo check` — so the "every PR passes `cargo test --workspace`" rule was not
  machine-enforced.
- **Design decision:** re-enable `on: pull_request` for `ci.yml` (activates the already-present
  core/full/gpu/pi5 gates + the pull_request-gated long-runner) and `docs.yml` (frontmatter +
  version-bump checks). Add a lean `macos-build` job (compile check under `--no-default-features`;
  macOS runners are a 10× cost multiplier, so no full suite — CPAL audio stays a hardware/on-air
  check). Strengthen the tracked pre-push to a fast `fmt --check` + `cargo check` (clippy/test/audit
  stay in CI). `release.yml`/`copilot-review.yml` stay manual by design.
- **Implementation:** `.github/workflows/{ci,docs}.yml`; `.cargo-husky/hooks/pre-push`; CLAUDE.md
  acceptance-table row corrected.
- **Tests:** YAML validated (`yaml.safe_load`); pre-push `bash -n` clean.
- **Test results:** N/A locally (CI activates on the next PR after this merges).

## 2026-07-11 — test/fix: PTT-backend tests, 32APSK calibration gate, discovery-mode warning (H2/H7/G-4)

- **Requirement/change:** H2 — the hardware PTT backends (`VoxPtt`, `RigctldPtt`) that key a real
  transmitter had no test; the ≤50 ms gate only covered `NoOpPtt`. H7 — `PILOT-32APSK` (densest pilot
  mode, the regression canary) was absent from the LLR-calibration gate. G-4 — `[discovery] mode` was
  documented but silently ignored (only `rx_only` is honored today).
- **Design decision:** add a `RigctldPtt` assert/release + error-response test against the existing
  mock rigctld, and a `VoxPtt` state + ≤50 ms test; add `PILOT-32APSK500` to `llr_calibration` with a
  conservative ≥1.5× floor; warn at daemon startup when a non-`rx_only` discovery mode is set and mark
  the config field reserved-for-Phase-E. (`SerialRtsDtrPtt` still needs a pty to test meaningfully —
  left as a known gap.)
- **Implementation:** `crates/openpulse-radio/tests/rigctld_integration.rs`,
  `crates/openpulse-radio/src/vox.rs`; `crates/openpulse-modem/tests/llr_calibration.rs`;
  `crates/openpulse-daemon/src/server.rs` (`build_discovery_runtime` warn);
  `crates/openpulse-config/src/lib.rs` (field doc).
- **Tests:** `rigctld_ptt_backend_asserts_and_releases`, `rigctld_ptt_surfaces_error_response`,
  `vox::tests::vox_assert_release_tracks_state_under_50ms`, `llr_calibration` now 7 modes.
- **Test results:** radio + config + modem llr suites green; fmt + clippy (`-D warnings`) clean.

## 2026-07-11 — feat(daemon/cli): implement ListFiles + file-transfer CLI (audit S4-1/S4-2)

- **Requirement/change:** S4-1 — `ControlCommand::ListFiles` was declared with a documented
  `FileList` response but had only a no-op handler; clients got a bare `ok` and files received before
  a client attached were invisible. S4-2 — the entire file-transfer surface (send/accept/reject/
  cancel/list) was drivable only from the panel, so headless/scripted stations couldn't use it.
- **Design decision:** give the daemon a session `received_files: Vec<FileSummary>` populated at the
  single real write site (`filexfer::reassemble_verify_write`, where `rs` and the path are in hand),
  and serve it from a real `ListFiles` handler (`emit_file_list`). Add five `openpulse daemon`
  subcommands mirroring the existing client pattern.
- **Implementation:** `crates/openpulse-daemon/src/lib.rs` (`received_files` field, `emit_file_list`,
  `ListFiles` arm); `filexfer.rs` (`reassemble_verify_write` takes `&mut rs`, pushes a `FileSummary`);
  `crates/openpulse-cli/src/cli.rs` (`SendFile`/`AcceptFile`/`RejectFile`/`CancelFile`/`Files`);
  `commands/daemon.rs` (dispatch + `list_files`).
- **Tests:** `command_apply_tests::list_files_reports_this_sessions_received_files`;
  `commands::daemon::tests::files_lists_received_files` (mock_daemon); clap verified via `daemon --help`.
- **Test results:** `cargo test -p openpulse-daemon` → 71 passed; `-p openpulse-cli` file test green;
  fmt + clippy (`-D warnings`) clean.

## 2026-07-11 — fix(modem/daemon): remove dead DCD blocks + stabilize flaky OTA test (audit G-6/S4-3)

- **Requirement/change:** G-6 — 13 vestigial `let prev_busy = self.dcd.is_busy(); … if
  self.dcd.is_busy() != prev_busy { DcdChange }` blocks in `engine.rs` read the same value twice with
  no `dcd.update()` between, so the `DcdChange` emission can never fire (dead since DCD moved to
  `update_dcd_at_seam`). S4-3 — `ota_ladder_steps_under_traffic_between_two_real_daemons` timed out
  under concurrent suite load (40 s outer budget vs 10×6 s inner rounds; ~18 s isolated).
- **Design decision:** delete the 13 dead blocks (the single real emitter, `update_dcd_at_seam` with
  its `dcd.update()`, is preserved). Raise the OTA test's outer/inner timeouts (40→120 s, 6→11 s) —
  the assertion is on the ladder stepping, not latency, so the timeout is a safety bound that must
  survive load.
- **Implementation:** `crates/openpulse-modem/src/engine.rs` (13 blocks removed);
  `crates/openpulse-daemon/tests/twin_daemon_bridge.rs` (timeouts).
- **Tests:** `engine_events` 8/8 (DcdChange still emitted at the seam); `twin_daemon_bridge
  ota_ladder` passes (18.5 s isolated).
- **Test results:** modem build + clippy (`-D warnings`) clean; `engine_events` 8 passed; OTA twin 1
  passed.

## 2026-07-11 — docs: refresh stale FF-15/FF-16 status + crate map (audit TR-03/04/05/06/08)

- **Requirement/change:** the audit found the living docs lagging shipped code — CLAUDE.md described
  FF-15 (JS8 discovery) and FF-16 (file transfer) as unstarted, the crate map omitted
  `openpulse-keystore`/`openpulse-linksec`, the traceability matrix had no forward chain for either
  family, and a `filexfer_loopback` acceptance row promised a `>64 KB` case that lives in another crate.
- **Design decision:** update the CLAUDE.md crate-map rows (js8/discovery/filexfer to SHIPPED), the
  "Active tracks" summary, and the key-docs table; add the two security crates; split the imprecise
  acceptance row; and add a matrix note pointing to this ledger for FF-15/FF-16 (formal CAP-70/71 rows
  deferred rather than fabricated half-complete).
- **Implementation:** `CLAUDE.md`; `docs/dev/project/traceability-matrix.md` (date + note).
- **Tests:** docs-only; no code change. `cargo fmt --all -- --check` unaffected.
- **Test results:** N/A (documentation).

## 2026-07-11 — fix(modem): CSMA gate on broadcast() (audit finding G-1)

- **Requirement/change:** the FF-audit found `ModemEngine::broadcast()` emitting with no
  `csma_check()` — with CSMA enabled, mesh/beacon broadcast frames keyed up on a busy channel
  (reachable from the CLI `broadcast`/`beacon` commands). A live instance of the "wired at callers,
  not the shared seam" bug class.
- **Design decision:** call `csma_check()` at the head of `broadcast()` (pre-encode, before
  incrementing the broadcast sequence — matching every other TX path's rationale of not burning a
  sequence number on deferral). Also corrected the stale CLAUDE.md §2.4 claim that the CSMA check
  lives "in `stage_emit_output()` (all transmit paths)".
- **Implementation:** `crates/openpulse-modem/src/engine.rs` (`broadcast()` + comment); `CLAUDE.md`.
- **Tests:** `crates/openpulse-modem/tests/csma_loopback.rs::csma_blocks_broadcast_when_dcd_busy`
  (fails without the fix — broadcast would emit).
- **Test results:** `cargo test -p openpulse-modem --no-default-features --test csma_loopback` →
  5 passed; mesh 8 passed; fmt + clippy (`-D warnings`) clean.

## 2026-07-11 — feat(js8): FF-15 — JSC codebook decode (general JS8 free text)

- **Requirement/change:** the free-text decoder handled only Huffman data frames; JSC-compressed
  frames (which `packDataMessage` picks when denser) were undecodable — blocking the passive
  INFO-token hint path and general traffic reading, and making beacon recognition depend on
  Huffman-forced TX.
- **Design decision:** embed the full 262 144-entry JSC codebook (ported from GPL-3.0 `jsc_map.cpp`)
  as a zlib-compressed ~1.1 MB blob (`flate2`, already a workspace dep), expanded once on first use,
  and port `JSC::decompress`. Unify `unpack_data_message` to return `Option<String>` (Huffman or JSC),
  dropping the `DataText` marker enum.
- **Implementation:** `plugins/js8/src/jsc.rs` (`jsc_decompress` + codebook loader),
  `plugins/js8/data/jsc_codebook.bin.z` (+ README provenance), `varicode.rs` (`unpack_data_message`
  routes on the `compressed` bit), `Cargo.toml` (`flate2`), exports in `lib.rs`; `hint_assembler.rs`
  updated to the `Option<String>` API (now also assembles JSC-framed beacons).
- **Key bug found + fixed:** the codebook extraction regex silently dropped 35 entries (C escapes
  `\\`/`\n`/`\xNN` and trailing `/* */` comments), which shifted every higher index and corrupted
  high-index decodes — caught because index-220k `ABCDEFGHIJK` decoded to `ARGENTIERE`. The parser now
  handles escapes + comments; the codebook is asserted to be exactly 262 144 entries.
- **Tests:** `jsc::codebook_expands_to_the_expected_size`, `jsc::jsc_data_frames_match_upstream`
  (11 vectors), `jsc::diverse_data_frames_match_upstream` (15 vectors, multi-word/high-index/punct.);
  `varicode` huffman tests updated to the `Option<String>` API. Vectors from the Qt5 ground-truth harness.
- **Test results:** `cargo test -p js8-plugin --no-default-features` → 71 passed, 0 failed;
  `-p openpulse-discovery` 31, `-p openpulse-daemon` 70; fmt + clippy (`-D warnings`) clean.

## 2026-07-11 — feat(panel): FF-15 — Discovery tab (stations + OpenPulse peers)

- **Requirement/change:** the panel had no view of JS8 discovery — Phase G's visual operator surface.
- **Design decision:** add a `Tab::Discovery` rendered from new `PanelState` fields
  (`discovery_state`, `discovery_dial_freq_hz`, `discovery_drift_ms`, `stations`, `peers`) populated
  by the reducer from `DiscoveryStatus`/`StationList`/`PeerList` events; the tab requests the lists on
  open and via a Refresh button, and has Enable/Disable controls. Reducer is unit-tested; the iced
  view is build-verified (the GUI can't be run-verified in a headless session).
- **Implementation:** `apps/openpulse-panel/src/state.rs` (5 fields + Default),
  `connection.rs::apply_event` (3 arms), `app.rs` (`Tab::Discovery`, `Message::{EnableDiscovery,
  DisableDiscovery, RefreshDiscovery}`, `SelectTab` auto-request), `ui.rs` (`Snap` fields + snapshot,
  tab button, `discovery_widget`).
- **Tests:** `connection::discovery_event_tests::station_and_peer_lists_and_status_populate_state`.
- **Test results:** `cargo test -p openpulse-panel` → 19 passed, 0 failed; `cargo build -p
  openpulse-panel` clean; fmt + clippy (`-D warnings`) clean.

## 2026-07-11 — feat(cli): FF-15 — daemon discovery commands (enable/disable, stations, peers)

- **Requirement/change:** discovery had a daemon control surface but no operator client — the only
  way to drive it or read results was raw control-protocol frames.
- **Design decision:** add four `openpulse daemon` subcommands mirroring the existing client pattern
  (`simple` for fire-and-forget, `run_command` + JSON print for queries), so discovery is usable from
  the terminal ahead of the panel tab (Phase G).
- **Implementation:** `crates/openpulse-cli/src/cli.rs` (`DaemonCommands::{EnableDiscovery,
  DisableDiscovery, Stations, Peers}`); `commands/daemon.rs` (dispatch arms + `list_stations` /
  `list_peers` printing `StationList` / `PeerList` as pretty JSON).
- **Tests:** `stations_lists_discovered_stations`, `peers_lists_recognized_opulse_peers` (via the
  `mock_daemon` harness); clap registration verified by driving `daemon --help`.
- **Test results:** `cargo test -p openpulse-cli --no-default-features` → passed (2 new); fmt + clippy
  (`-D warnings`) clean; `daemon --help` lists all four commands.

## 2026-07-11 — feat(daemon): FF-15 — recognized peers into the shared PeerCache + ListPeers

- **Requirement/change:** recognized OpenPulse peers were only in discovery's local `StationTable`
  (surfaced as a bool via `ListStations`). Plan §5.2 wants them in the *shared* `PeerCache` — the
  substrate rendezvous/relay/query read — with capabilities and quality. `station_to_peer_record`
  (`peer_map.rs`) existed for this but was dead code.
- **Design decision:** add a `PeerCache` to the daemon's `RuntimeControlState`; `discovery_tick`
  folds newly-heard hinted stations into it via `station_to_peer_record` (plain JS8 stations map to
  `None` and are skipped). To avoid unread infrastructure, expose it with a `ListPeers`/`PeerList`
  control command (peer_id, capability_mask, route_quality, trust_level) mirroring the existing
  `ListStations`/`StationList` pattern.
- **Implementation:** `crates/openpulse-daemon/src/protocol.rs` (`ControlCommand::ListPeers`,
  `ControlEvent::PeerList`, `PeerSummary`); `lib.rs` (`RuntimeControlState.peer_cache`,
  `sync_discovered_peers`, `emit_peer_list`, `ListPeers` arm in `apply_command_to_engine`);
  `server.rs::discovery_tick` calls the sync when a station is heard.
- **Tests:** `server.rs::discovery_tick_recognizes_an_opulse_peer_into_the_shared_cache` (4-slot
  beacon through the real `discovery_tick` → `PeerCache` holds `js8:DC0SK` caps `0xB105`, and
  `emit_peer_list` reports it); `protocol.rs` round-trip extended with `ListPeers`/`PeerList`.
- **Test results:** `cargo test -p openpulse-daemon --no-default-features` → 70 passed, 0 failed;
  `cargo build --workspace` clean (app crates' `ControlEvent` catch-alls absorb the new variants);
  fmt + clippy (`-D warnings`) clean.

## 2026-07-11 — feat(discovery): FF-15 — @OPULSE HintAssembler + runtime wiring (peer recognition)

- **Requirement/change:** with the free-text/directed unpacker in place, recognise an OpenPulse peer
  from its `@OPULSE` beacon. On air the beacon is a 4-frame compound-directed *over* (one frame per
  15 s slot), so recognition needs cross-slot assembly, not a single-frame decode.
- **Design decision:** a `HintAssembler` buffers decoded frames per audio-offset bucket (a station
  holds one offset across its over), bracketed by the `First`/`Last` transmission flags, and folds
  `Compound`(sender) + `CompoundDirected`(`@OPULSE`) + Huffman `Data` frames into `(sender, text)`,
  then runs the CRC-salted `decode_hint`. The CRC gate makes premature recognition on a partial text
  simply fail, so it recognises opportunistically after each frame (a fade may drop the `Last` flag).
  Root-caused and fixed a latent grammar bug: `unpack_compound_frame` only rejected flags 3–4, so
  flag-5/6/7 data frames were mis-accepted as compound — now rejects all of flag ≥ 3.
- **Implementation:** `crates/openpulse-discovery/src/hint_assembler.rs`
  (`HintAssembler`/`RecognizedHint`); `runtime.rs::decode_slot` feeds every decode to the assembler
  and upserts a recognised sender into the `StationTable` with its `OphfHint`, so the daemon's
  existing `ListStations` reports `is_opulse: true` (no daemon change). `plugins/js8/src/grammar.rs`
  guard fix + exports in `lib.rs`.
- **Tests:** `hint_assembler.rs` (assemble 4-frame beacon; frequency isolation; stale eviction;
  wrong-callsign CRC rejection; interleaved heartbeats); `grammar.rs::data_frames_are_not_compound_frames`;
  `runtime.rs::recognizes_an_opulse_peer_from_a_four_slot_beacon` (end-to-end through the real
  `decode_window`). Assembler vectors from the Qt5 ground-truth harness (`buildMessageFrames` +
  forced `packHuffMessage`).
- **Test results:** `cargo test -p openpulse-discovery --no-default-features` → 31 passed, 0 failed;
  `cargo test -p js8-plugin --lib` → 69 passed; `cargo build -p openpulse-daemon` clean; fmt + clippy
  (`-D warnings`) clean.

## 2026-07-11 — feat(js8): FF-15 — JS8 free-text (Huffman) + directed-message unpacker (Phase-C message-layer gap)

- **Requirement/change:** the `@OPULSE` peer-recognition path (G3) was blocked — `runtime.rs`
  hard-codes `hint: None // needs varicode free-text decode`, and `grammar.rs` returned `None` for
  `Directed`/`Data` frames. Nothing turned a decoded JS8 free-text frame into the `(sender, @OPULSE,
  "OPHF1 …")` triple the existing `openpulse-discovery::hint` codec consumes.
- **Design decision:** port the JS8 message-layer *leaf* decoders and validate against the verbatim
  upstream `Varicode` compiled against real Qt5. A ground-truth harness (upstream `varicode.cpp` +
  `jsc_*.cpp`) measured the on-air reality: the beacon is a **4-frame** compound-directed sequence
  (not the plan's "1–2 frames"), and `packDataMessage` mixes Huffman and the 262k-entry JSC coder.
  **Scope decision:** ship the small, fully-portable Huffman + directed unpacker now; defer the JSC
  codebook (general free-text) and the multi-frame `@OPULSE` correlation + `PeerCache` wiring as
  follow-ons. The OpenPulse beacon is standardized on Huffman-framed data (its alphabet is entirely
  in the Huffman table, and stock JS8Call decodes Huffman frames), so Huffman-only RX covers the hint.
- **Implementation:** `plugins/js8/src/varicode.rs` — `HUFF_TABLE`, `huff_decode`,
  `unpack_data_message` → `DataText::{Huffman, JscUnsupported}`; `grammar::unpack_directed_message`
  (`FrameDirected` [flag 3]: `[from:28][to:28][cmd:5]` + portable/num byte → `DirectedMessage`),
  `DIRECTED_CMD_BY_VALUE` reverse map, `format_snr`, `<....>` placeholder handling; exports in
  `lib.rs`.
- **Tests:** `plugins/js8/src/varicode.rs` (`huffman_data_frames_match_upstream`,
  `jsc_compressed_frames_are_flagged_unsupported`, `non_data_frame_returns_none`,
  `huff_decode_is_prefix_free_and_stops_cleanly`); `grammar.rs`
  (`directed_messages_match_upstream`, `directed_snr_numbers_match_upstream`,
  `non_directed_frame_returns_none`). Vectors emitted by the Qt5 ground-truth harness.
- **Test results:** `cargo test -p js8-plugin --no-default-features --lib` → 68 passed, 0 failed
  (7 new); `cargo build -p js8-plugin -p openpulse-discovery -p openpulse-daemon` clean; fmt +
  clippy (`-D warnings`) clean on `js8-plugin`.

## 2026-07-11 — feat(js8): FF-15 — real per-decode SNR estimate (2500 Hz ref BW), replacing the sync-score proxy

- **Requirement/change:** post-MVP RX refinement. Discovery cached every station's SNR as
  `sync_score − 21` — a monotone proxy, not a real dB value — so `route_quality` scoring and the
  panel display were uncalibrated.
- **Design decision:** matched-filter estimator on the transmitted data tones (re-encoded from the
  decoded info bits, so the signal tone per symbol is known exactly): per data symbol the Goertzel
  energy at the sent tone is signal+noise; the noise floor comes from **out-of-band guard bins**
  (tone offsets −3,−2,9,10 — ≥2 bins outside the 0..7 band). Measuring noise in-band saturates the
  estimate at high SNR because the wide GFSK pulse (BT=2.0) leaks signal energy into the neighbouring
  bins; guard bins decouple the noise floor from signal power. The aggregate bin-bandwidth SNR is
  scaled to the 2500 Hz reference; a single fitted `SNR_CAL_OFFSET_DB` (+0.5) folds in the Goertzel
  ENBW + pulse spreading. Accuracy is gated only on the JS8 weak-signal band (−12…+3 dB); above +3 dB
  the non-coherent estimate compresses, which is immaterial (`route_quality` saturates there).
- **Implementation:** `plugins/js8/src/decoder.rs` `estimate_snr_db` + `Js8Decode::snr_db` (set in
  `decode_window`); `plugins/js8/src/demodulate.rs` `goertzel_energy` made `pub`;
  `crates/openpulse-discovery/src/runtime.rs` `ingest_decode` now uses `d.snr_db`.
- **Tests:** `plugins/js8/tests/snr_estimate.rs` — `tracks_injected_snr` (within 2 dB of the injected
  SNR across −12…+3 dB and strictly monotone across −12…+9 dB, using the B-6 calibrated-AWGN model);
  `characterize` (ignored) prints the fit. Calibration measured: err ≤ ~0.7 dB over −15…0 dB.
- **Test results:** `cargo test -p js8-plugin --no-default-features` → 61 lib + gates pass (incl. the
  B-6 −18 dB `gate_at_minus_18_db`); `snr_estimate::tracks_injected_snr` passes;
  `openpulse-discovery` 25 / `openpulse-daemon` 69 pass; clippy (`-D warnings`, `--tests`) + fmt clean;
  `cargo build --workspace --no-default-features` clean.

## 2026-07-11 — feat(daemon): FF-15 — discovery dwells on the current home band's JS8 freq (RX refinement)

- **Requirement/change:** post-MVP RX refinement. `build_discovery_runtime` hardcoded the 20 m
  calling frequency, so discovery would QSY to 14.078 MHz regardless of the operator's home band —
  contradicting decision **D7** ("single-band dwell = *current* band's JS8 frequency"). The config
  band table (`[discovery.calling_freqs_hz]`) already existed but was only read for the 20 m entry.
- **Design decision:** keep the `openpulse-discovery` crate pure (no bandplan dependency). The daemon
  already resolves band labels for DCD squelch via `openpulse_qsy::bandplan::band_label_for_hz`;
  reuse it. Store the full band table in `RuntimeControlState`; in `discovery_tick`, **while the
  runtime is `Inactive`** (so `last_freq_hz` is still the home dial, not the JS8 freq), resolve the
  home band → calling frequency and push it into the runtime via a new `set_dial_freq_hz` setter.
  Takes effect on the next `Retune`, and `dial_freq_hz()` (drives `DiscoveryStatus`) stays accurate.
- **Implementation:** `crates/openpulse-discovery/src/runtime.rs` `DiscoveryRuntime::set_dial_freq_hz`;
  `crates/openpulse-daemon/src/lib.rs` `RuntimeControlState::discovery_calling_freqs_hz`;
  `crates/openpulse-daemon/src/server.rs` per-band resolution in `discovery_tick` + table populated
  in `RuntimeControlState` construction + updated `build_discovery_runtime` doc.
- **Tests:** `discovery_tick_qsys_to_the_current_home_bands_calling_freq` (home on 40 m → QSYs to
  7.078 MHz, not the 20 m default; asserts `last_freq_hz`, `discovery_home_freq_hz`, and the runtime
  `dial_freq_hz()`). Existing `discovery_tick_activates_dwells_and_hears_an_injected_station`
  (acceptance gate) unchanged.
- **Test results:** `cargo test -p openpulse-daemon --no-default-features --lib` → 69 passed / 0
  failed; `cargo test -p openpulse-discovery --no-default-features` → 25 passed; clippy (`-D
  warnings`) + fmt clean on both crates.

## 2026-07-11 — feat(daemon): FF-15 Phase D-5c(ii) — live rx-tick wiring — **RX-only MVP complete**

- **Requirement/change:** FF-15 Phase D ship line (plan §6.1/§6.2/§6.3): wire `DiscoveryRuntime` into the
  daemon's live receive loop so an idle station actually QSYs to the JS8 calling frequency, dwells, decodes
  each slot, caches stations, and emits events — the RX-only MVP.
- **Design decision:** build the runtime from `[discovery]` config in `server::run` (`build_discovery_runtime`;
  calling freq = the 20 m band for the MVP, per-home-band selection deferred). In the rx-tick, **tee** the raw
  captured samples to the runtime only while dwelling (the DCD-burst pipeline can't carry −24 dB JS8, §6.2),
  then run `discovery_tick`: assemble a simplified idle predicate (`hpx_state==Idle` + no pending handshake /
  file transfer / OTA), run `DiscoveryRuntime::tick` inside `block_in_place` (the ~1 s slot decode off the
  async workers — `spawn_blocking` split is a later refinement), and execute the outcomes — retune via the CAT
  controller (no rig = loopback success), save/restore the home frequency (`last_freq_hz` ↔
  `discovery_home_freq_hz`), report `qsy_complete`, and forward `StationHeard`/`DiscoveryStatus`. Refactored the
  status emit into a reusable `emit_discovery_status`. Untangled the `Option<&mut dyn CatController>` reborrow by
  inlining the retune with a fresh short `rig.as_mut()` borrow per outcome.
- **Implementation:** `crates/openpulse-daemon/src/server.rs` (`build_discovery_runtime`, `discovery_tick`,
  `discovery_retune`, `epoch_ms`; rx-tick tee + call; runtime built in the state literal);
  `lib.rs` (`RuntimeControlState.discovery_home_freq_hz`, `emit_discovery_status`); daemon `js8-plugin` dev-dep.
- **Tests:** `server.rs` `discovery_tick_tests` (2) — **the daemon rx-tick activates → saves home freq → tunes
  to the JS8 calling freq → dwells → buffers an injected NORMAL heartbeat → decodes on the slot boundary →
  caches `KN4CRD`/`EM73` + emits `StationHeard`**; and a no-op when discovery is unconfigured. Added to the
  CLAUDE.md acceptance table.
- **Test results (actually run):** `openpulse-daemon` 91 passed / 0 failed (2 new); workspace builds 0 errors;
  clippy `-D warnings` + fmt clean.
- **RX-only discovery MVP is complete.** An idle daemon with `[discovery] enabled = true` (or after
  `EnableDiscovery`) QSYs to JS8, hears + caches stations, and surfaces them via events / `ListStations` — zero
  TX, zero regulatory exposure. **Next:** later refinements (varicode → `@OPULSE` hint marking + `PeerCache`
  upsert; per-home-band calling freq; `spawn_blocking` decode; true SNR) and then Phase E (beacon TX, gated on
  the §97.221 automatic-control reg doc), F (rendezvous), G (panel), H (on-air).

---

## 2026-07-11 — feat(daemon): FF-15 Phase D-5c(i) — discovery control-protocol surface

- **Requirement/change:** FF-15 Phase D (plan §6.1/§6.5): the operator control surface for discovery — enable/
  disable it and list what's been heard — plus the `RuntimeControlState` field the live loop will drive.
- **Design decision:** add `EnableDiscovery`/`DisableDiscovery`/`ListStations` commands and `DiscoveryStatus`/
  `StationHeard`/`StationList` events (+ `StationSummary`), and `RuntimeControlState.discovery:
  Option<DiscoveryRuntime>` (`None` when unconfigured). `apply_command_to_engine` handles the three commands via
  small helpers: enable/disable toggle the runtime and emit `DiscoveryStatus` (retune-outcome execution happens
  in the rx-tick loop — the next slice); `ListStations` emits a `StationList` from the table; all degrade to a
  `CommandError` when discovery isn't configured. Re-exported `Submode` from `openpulse-discovery` so the daemon
  can build `DiscoveryParams`. Added `openpulse-discovery` as a daemon dependency. **The live rx-tick audio feed
  + CAT retune + injected-audio daemon test are the next slice (D-5c(ii))** — isolated so the delicate async
  loop change lands on its own.
- **Implementation:** `crates/openpulse-daemon/src/protocol.rs` (3 commands + 3 events + `StationSummary`);
  `lib.rs` (`RuntimeControlState.discovery`, `set_discovery_enabled`, `emit_station_list`,
  `discovery_state_label`, command arms); `crates/openpulse-discovery/src/runtime.rs` (`set_enabled`,
  `dial_freq_hz`, `drift_bias_ms` accessors + `Submode` re-export); daemon `Cargo.toml` dep.
- **Tests:** `protocol.rs` `discovery_commands_and_events_round_trip_via_json`; `lib.rs`
  `discovery_commands_toggle_the_runtime_and_list_stations` (unconfigured → `CommandError`; configured →
  `DiscoveryStatus` with the calling freq; `ListStations` → empty `StationList`).
- **Test results (actually run):** `openpulse-daemon` 66 + suites passed / 0 failed (2 new); `openpulse-discovery`
  25 passed; workspace builds 0 errors; clippy `-D warnings` + fmt clean.
- **Next:** D-5c(ii) — wire the rx-tick: tee captured samples + the assembled idle predicate into
  `DiscoveryRuntime::tick` (decode off-thread), execute `Retune`/`RestoreHome` via the CAT controller, forward
  `StationHeard`, and the injected-audio daemon acceptance test.

---

## 2026-07-11 — feat(discovery): FF-15 Phase D-5b — `DiscoveryRuntime` orchestrator (MVP logic)

- **Requirement/change:** FF-15 Phase D: the runtime that ties the pure units together into the working
  RX-only MVP — idle → QSY to the JS8 frequency → dwell → decode each slot → cache stations → emit events.
- **Design decision:** keep the orchestrator **pure and async-free** in `openpulse-discovery` so the eventual
  daemon glue (async loop, CAT retune, event plumbing) stays thin. `DiscoveryRuntime` owns the `DiscoverySm`,
  `Js8Clock`, `SlotTracker`, `StationTable`, and a dwell audio buffer; `push_audio` accumulates while dwelling;
  `tick(now_ms, idle)` runs the SM and, on a UTC slot boundary, decodes the buffered slot via `decode_window`,
  upserts every heartbeat (callsign + grid) and TTL-sweeps; retune is delegated to the daemon via
  `DiscoveryOutcome::Retune`/`RestoreHome` + `qsy_complete(ok)`. Heartbeats carry callsign+grid; `@OPULSE` hint
  marking needs varicode free-text decode (a later unit), so stations are cached un-marked for now, and SNR is a
  documented sync-score proxy pending a true estimate.
- **Implementation:** `crates/openpulse-discovery/src/runtime.rs` (`DiscoveryRuntime`, `DiscoveryParams`,
  `DiscoveryOutcome`); `lib.rs` module + re-exports.
- **Tests:** `runtime.rs` (3) — **the full MVP flow: idle → `Retune` → dwell → buffer a NORMAL slot of
  synthesized JS8 audio → decode → caches `KN4CRD`/`EM73` + emits `StationHeard`**; a disabled runtime never
  retunes; `preempt` restores home and clears the buffer.
- **Test results (actually run):** `openpulse-discovery` 25 passed / 0 failed (3 new); workspace builds 0
  errors; clippy `-D warnings` + fmt clean.
- **Next (D-5c, the final MVP integration):** wire `DiscoveryRuntime` into the daemon `server::run` rx-tick
  (feed captured samples + the assembled idle predicate; run `decode_window` off-thread via `spawn_blocking`),
  execute `Retune`/`RestoreHome` via the CAT controller, add `DiscoveryStatus`/`StationHeard`/enable-disable
  control-protocol surface, and the twin-daemon RX acceptance test. Then later: varicode → `@OPULSE` hint
  marking + `PeerCache` upsert; a true SNR estimate.

---

## 2026-07-11 — feat(config): FF-15 Phase D-5a — `[discovery]` config section

- **Requirement/change:** FF-15 Phase D (plan §8): the operator-facing `[discovery]` config that turns JS8
  discovery on and parameterizes it — the scaffolding the daemon glue reads.
- **Design decision:** add `DiscoveryConfig` following the `FileTransferConfig`/`LogbookConfig` pattern
  (struct-level `#[serde(default)]`, manual `Default`, opt-in `enabled = false`, template block in
  `init_template()` kept parseable under the guard test). Fields: `enabled`, `mode` (`rx_only` default),
  `submode`, `idle_grace_secs`, `dwell_secs`, `station_ttl_secs`, `max_clock_skew_ms`, `group`, the beacon/
  query cadence knobs (unused until Phase E), and a `calling_freqs_hz: BTreeMap<band → Hz>` seeded with the
  published JS8 conventions. Rendezvous-channel config is deferred to Phase F.
- **Implementation:** `crates/openpulse-config/src/lib.rs` (`DiscoveryConfig` + `OpenpulseConfig.discovery`
  field + `init_template()` `[discovery]` block).
- **Tests:** `discovery_defaults_are_opt_in_and_rx_only` (defaults sane + opt-in; the template's `[discovery]`
  round-trips with the band table); the existing `modem_profile_loads_and_template_parses` guard still passes
  (the new section keeps the template valid TOML).
- **Test results (actually run):** `openpulse-config` 16 passed / 0 failed (1 new); workspace builds 0 errors
  (existing `OpenpulseConfig` literals use `..Default::default()`); clippy `-D warnings` + fmt clean.
- **Next:** D-5b — the daemon glue: the dwell audio ring + off-thread `decode_window`, the SM/clock/table wired
  into `server::run`, `DiscoveryStatus`/`StationHeard` control events, and the twin-daemon RX acceptance test.

---

## 2026-07-11 — feat(discovery): FF-15 Phase D-4 — discovery state machine

- **Requirement/change:** FF-15 Phase D (plan §4.3): the discovery lifecycle — when the station is idle, QSY to
  the JS8 calling frequency, dwell (RX-only) decoding each slot, and return home on budget/preemption.
- **Design decision:** a pure `DiscoverySm` (`INACTIVE → ACTIVATING → DWELLING`) driven by `step(event) ->
  Vec<DiscoveryAction>`. The daemon assembles the idle predicate (§4.3) and feeds it as `Tick { idle, clock_ok,
  now_ms }`; the SM tracks how long idle has continuously held and activates only after `idle_grace_ms`
  (emitting `SaveHomeAndTune`). `QsyComplete{ok}` → `Dwelling` or stand-down. In `Dwelling`, each `SlotElapsed`
  emits `DecodeSlot`; the dwell budget (`dwell_ms`, 0 = until preempted) is armed on the first slot and returns
  home (`RestoreHome`) when spent. `Preempt` (operator command needs the modem) and `set_enabled(false)` stand
  down from any active state; a disabled SM or a bad clock never activates. **No TX paths** — RENDEZVOUS and
  slot TX are later phases.
- **Implementation:** `crates/openpulse-discovery/src/discovery_sm.rs` (`DiscoverySm`, `DiscoveryState`,
  `DiscoveryEvent`, `DiscoveryAction`); `lib.rs` module + re-exports.
- **Tests:** `discovery_sm.rs` (6) — activates only after idle holds for the grace period; losing idle resets
  the timer; QSY success dwells / failure restores; dwelling decodes each slot then returns home at the budget;
  preempt + disable stand down from dwell; disabled or bad-clock never activates.
- **Test results (actually run):** `openpulse-discovery` 22 passed / 0 failed (6 new); workspace builds 0
  errors; clippy `-D warnings` + fmt clean.
- **Next:** D-5 — the daemon glue: `[js8]` config, the dwell audio ring + off-thread `decode_window`, wiring the
  SM/clock/table into `server::run`, `DiscoveryStatus`/`StationHeard` events, and the twin-daemon RX acceptance
  test (hears + caches + events, zero TX). That's the MVP ship line.

---

## 2026-07-11 — feat(discovery): FF-15 Phase D-3 — `PeerCache` mapping + capability-bit registry

- **Requirement/change:** FF-15 Phase D (plan §5.2): feed OpenPulse-marked JS8 stations into the shared
  `PeerCache` so the rest of the stack (`PeerCache::query`, relay routing) can find rendezvous-capable peers,
  and claim/document the low `capability_mask` bits (none were registered — the wire doc said
  "application-defined").
- **Design decision:** `station_to_peer_record` maps a hint-carrying `Js8Station` → `PeerRecord`
  (`peer_id = js8:<callsign>`, `capability_mask` = hint caps, `route_quality` = `((snr+30).clamp(0,42)·6)` on
  the 0–252 scale, `TrustLevel::Unknown`, `revision` 0, `callsign_hash` = SHA-256) — and **returns `None` for a
  plain (hint-less) JS8 station** (those stay only in the `StationTable`; they're not OpenPulse peers). A
  key-less `Unknown`-trust `revision`-0 record is exactly right: it passes `Any`/`TrustedOrUnknown`, is
  excluded from `TrustedOnly`, and loses every upsert conflict to an authenticated descriptor record. Added the
  capability-bit registry (`CAP_HPX`/`CAP_RENDEZVOUS`/`CAP_QSY`/`CAP_PQ`/`CAP_RELAY`) in code + normatively in
  `docs/dev/peer-query-relay-wire.md`. Used `sha2` directly rather than widen core's `pub(crate) sha256_bytes`.
- **Implementation:** `crates/openpulse-discovery/src/peer_map.rs` (`station_to_peer_record`, cap constants,
  `route_quality_from_snr`); `openpulse-core` + `sha2` deps; `lib.rs` re-exports;
  `docs/dev/peer-query-relay-wire.md` registry table.
- **Tests:** `peer_map.rs` (4) — a plain station is not a peer; a marked station maps to a key-less
  `Unknown`-trust peer with the right id/caps/hash; `route_quality` is monotone + bounded (0…252); **the record
  passes `TrustFilter::Any` but not `TrustedOnly`** through a real `PeerCache::query`.
- **Test results (actually run):** `openpulse-discovery` 16 passed / 0 failed (4 new); workspace builds 0
  errors; clippy `-D warnings` + fmt clean.
- **Next:** D-4 the discovery state machine (`INACTIVE → QSY_TO_HB → DWELL`, idle-predicate-driven), then D-5
  the daemon dwell-ring glue + config + events + the twin RX acceptance test.

---

## 2026-07-11 — feat(discovery): FF-15 Phase D-2 — `StationTable` + discovered-station records

- **Requirement/change:** FF-15 Phase D (plan §5.1): the discovered-station data model — a callsign-keyed
  table each JS8 decode upserts into, with per-station SNR smoothing, sticky grid/hint, and a TTL sweep.
- **Design decision:** `Js8Station` (callsign, grid, EWMA `snr_db`, freq offset, dial freq, last-heard,
  heard-count, parsed hint, query backoff) in a `BTreeMap`-backed `StationTable`; `upsert(Observation, now)`
  creates-or-updates (SNR EWMA α=0.3; grid/hint only *learned*, never cleared; heard-count bumped) and reports
  new-vs-updated; `sweep(now, ttl)` drops stale stations. `OphfHint::from_payload` maps a decoded hint (raw
  `pref_channel` 63 → `None`; the §5.4 submode code → `Submode`). `QueryBackoff` is carried but unused until the
  TX query policy (Phase E). Pure, no I/O.
- **Implementation:** `crates/openpulse-discovery/src/station.rs` (`Js8Station`, `Observation`, `OphfHint`,
  `QueryBackoff`, `StationTable`); `lib.rs` module + re-exports.
- **Tests:** `station.rs` (4) — upsert creates then updates (heard-count, EWMA between old/new, last-heard);
  grid/hint sticky once learned; TTL sweep drops only the stale station; `OphfHint::from_payload` maps
  pref-channel/submode.
- **Test results (actually run):** `openpulse-discovery` 12 passed / 0 failed (4 new); clippy `-D warnings` +
  fmt clean.
- **Next:** D-3 the `PeerCache` mapping (+ the capability-bit registry) so OpenPulse-marked stations feed the
  shared substrate, D-4 the discovery state machine, then D-5 the daemon dwell-ring glue + twin RX test.

---

## 2026-07-11 — feat(discovery): FF-15 Phase D-1 — `Js8Clock` wall-clock T/R scheduler

- **Requirement/change:** FF-15 Phase D (RX-only discovery MVP, plan §6.3): the wall-clock scheduler — map
  UTC epoch time to a JS8 slot index/phase, carry a drift bias, and gate TX when the clock is too far off.
  Nothing in the daemon is UTC-slot-aligned today (the closest, `StationIdTimer`, is interval-based).
- **Design decision:** keep it **pure** — `Js8Clock` takes `now_ms` (UTC epoch millis), so the daemon owns the
  `SystemTime` read and the unit stays deterministically testable. Slot geometry comes from the submode params
  (NORMAL = 15 000 ms, 500 ms start delay); a `drift_bias_ms` (the running median of decode `dt`s, plan §2.3)
  shifts every reading; `tx_allowed(max_skew_ms)` enforces the ±2 s JS8 tolerance (plan D5 — RX-only degrade
  beyond it). Added a `SlotTracker` that fires once per slot boundary so the daemon can close out each dwell
  window. SlotPlan TX cadence is Phase E (this MVP is RX-only).
- **Implementation:** `crates/openpulse-discovery/src/scheduler.rs` (`Js8Clock`, `SlotTracker`); `js8-plugin`
  dep added; `lib.rs` module + re-exports.
- **Tests:** `scheduler.rs` (4) — 15 s slots index UTC (`:00/:15/:45`) + phase; `next_slot_start_ms` /
  `tx_start_ms` (slot + start delay); drift bias shifts the slot and gates TX at ±2 s; `SlotTracker` fires once
  per boundary.
- **Test results (actually run):** `openpulse-discovery` 8 passed / 0 failed (4 new); clippy `-D warnings` +
  fmt clean.
- **Next:** D-2 `StationTable` + `Js8Station`/`OphfHint` records (TTL sweep, upsert from decodes), D-3 the
  `PeerCache` mapping + capability-bit registry, D-4 the discovery state machine, then D-5 the daemon dwell-ring
  glue + twin RX test.

---

## 2026-07-11 — feat(discovery): FF-15 Phase C-3 — `@OPULSE` capability hint codec

- **Requirement/change:** FF-15 Phase C (plan §3.2/§5.4): the in-band `@OPULSE` marker that lets one OpenPulse
  station recognize another among ordinary JS8 traffic — the last Phase-C piece the RX-only discovery MVP
  needs.
- **Design decision:** create the new pure, no-I/O **`crates/openpulse-discovery`** crate (the home for the
  station table, T/R scheduler, and discovery/rendezvous SMs to come) and seed it with `hint.rs`. The hint is a
  free-text token `OPHF<version> XXXXXXXX` (8 base-36 chars = 40 bits): `caps:16 | pref_channel:6 |
  listen_submode:3 | reserved:7 | check:8`, where `check` is a CRC-8/SMBUS over the low 32 bits **salted with
  the sender callsign**. Detection requires all three of: the exact `OPHF<our-version>` token, a valid 8-char
  base-36 payload, and a CRC that verifies against the callsign — so organic JS8 text can't be mistaken for a
  hint and a copy-pasted payload from another station fails to verify. This is a fresh OpenPulse format (not a
  JS8 port), so it's validated by construction, not upstream vectors. The `@OPULSE` group *addressing* is
  applied by the JS8 message layer (directed frame); peer authentication is deferred to the post-QSY signed
  CONREQ/CONACK.
- **Implementation:** `crates/openpulse-discovery/{Cargo.toml, src/lib.rs, src/hint.rs}` (`encode_hint`,
  `decode_hint`, `HintPayload`, `crc8_salted`, base-36 codec); `Cargo.toml` workspace member + dep.
- **Tests:** `hint.rs` (4) — round-trip; **CRC binds to the sender callsign** (a DC0SK hint won't verify as
  W1AW; callsign salt is case/whitespace-insensitive); rejects non-hint text (wrong token/version/length/char)
  and a random-but-well-formed payload (CRC); all fields survive at their max bit-widths.
- **Test results (actually run):** `openpulse-discovery` 4 passed / 0 failed; workspace builds 0 errors; clippy
  `-D warnings` + fmt clean.
- **Next:** Phase C is functionally done for the MVP (RX field/grammar/hint decode). **Phase D — the RX-only
  discovery MVP ship line:** `Js8Clock` (wall-clock T/R slots), `StationTable` + `PeerCache` mapping, the
  discovery state machine, and the daemon dwell-ring + auto-QSY glue — all zero-TX.

---

## 2026-07-11 — feat(js8): FF-15 Phase C-2 — compound-frame grammar (RX heartbeat decode)

- **Requirement/change:** FF-15 Phase C: parse a decoded 72-bit payload into a sender callsign + frame type +
  grid — the receive-side grammar the discovery MVP uses to learn *who* it heard and *where*.
- **Design decision + finding:** heartbeats pack the callsign with the **50-bit** `packAlphaNumeric50` (not the
  28-bit packer) and lay the frame out as `[flag:3][callsign50:50][num_hi:11][num_lo:5][bits3:3]`. Key
  simplification: `pack72bits` serializes exactly `value64 ‖ rem8`, so the decoder's 9-byte payload **is**
  `value64` (big-endian bytes 0–7) then `rem8` (byte 8) — the fields read straight off the bits, **no
  `alphabet72` char round-trip needed** (verified by matching upstream). Ported `unpackAlphaNumeric50` (incl.
  its position-0 modulo-39 quirk and the separator-space strip) and `unpackCompoundFrame`; added `FrameType`,
  `CompoundFrame`, `Heartbeat`, and `parse_heartbeat` (grid in low 15 bits, `@CQ` alt bit at 0x8000). TX-side
  `packCompoundFrame`/`packAlphaNumeric50` are deferred to Phase E (beacon TX).
- **Implementation:** `plugins/js8/src/grammar.rs` (`unpack_alphanumeric50`, `unpack_compound_frame`,
  `parse_heartbeat`, `FrameType`, `CompoundFrame`, `Heartbeat`); `frame::ALPHANUMERIC` made `pub(crate)`;
  `lib.rs` module + re-exports.
- **Tests:** `grammar.rs` (3) — `unpack_alphanumeric50` matches upstream (KN4CRD/DC0SK/W1AW from Qt
  `packAlphaNumeric50`); **three full heartbeat payloads (assembled by verbatim upstream on Qt) decode to the
  right callsign + grid**; and **the full RX pipeline** — heartbeat payload → LDPC → GFSK audio →
  `decode_window` → grammar recovers `KN4CRD` + `EM73` off the air.
- **Test results (actually run):** `js8-plugin` 61 passed / 0 failed (3 new); workspace builds 0 errors; clippy
  `-D warnings` + fmt clean.
- **Next:** C-3 the `@OPULSE` capability hint (OPHF payload codec + detector + CRC-8, plan §3.2/§5.4) — the
  actual OpenPulse marker, and the last piece the RX-only discovery MVP needs from Phase C. (Varicode/JSC free
  text is only needed for the queried-INFO hint path and can follow.)

---

## 2026-07-11 — feat(js8): FF-15 Phase C-1 — callsign/grid unpackers (RX field decode)

- **Requirement/change:** FF-15 Phase C (message grammar): the RX side of the field codecs — turn a decoded
  frame's 28-bit callsign and 15-bit grid back into strings, so the discovery MVP can read *who* it heard and
  *where*. (The RX-only MVP needs unpack more than pack.)
- **Design decision:** port `unpackCallsign`/`unpackGrid`/`deg2grid` from JS8Call `varicode.cpp` — the exact
  inverses of A-5's packers (mixed-radix unwind + the Swaziland/Guinea workaround reversals; grid via
  `deg2grid`). Validate against ground truth from the **verbatim upstream unpackers compiled on real Qt**, and
  additionally prove `pack∘unpack = id` both directions over the vectors. Group/hashed callsign values (the
  `basecalls` range) are still deferred to the compound-frame grammar.
- **Implementation:** `plugins/js8/src/frame.rs` (`unpack_callsign`, `unpack_grid`, `deg2grid`, `NBASEGRID`);
  `lib.rs` re-exports.
- **Tests:** `frame.rs` (3 new) — `unpack_callsign` matches upstream on the 11 call vectors (incl. `3DA0XX`);
  `unpack_grid` matches on the 8 grid vectors; callsign + grid **round-trip** (`unpack(pack(x)) == x`).
- **Test results (actually run):** `js8-plugin` 58 passed / 0 failed (3 new); clippy `-D warnings` + fmt clean.
- **Next:** C-2 `packCompoundFrame`/`unpackCompoundFrame` (the heartbeat/directed 72-bit payload layout tying
  callsign + type + grid/num together), then varicode free text + JSC, then the `@OPULSE` capability hint.

---

## 2026-07-11 — test(js8): FF-15 Phase B-6 — −18 dB weak-signal go/no-go — **PASS**

- **Requirement/change:** FF-15 Phase B's decisive checkpoint (plan §11): the native JS8 NORMAL decoder must
  reach the **−18 dB** weak-signal class (SNR in the 2500 Hz reference bandwidth), or the D1 fallback (a
  headless external JS8Call for RX) is triggered.
- **Design decision:** a calibrated-AWGN SNR sweep — synthesize a NORMAL frame, add white Gaussian noise at
  `σ² = Ps·(fs/2)/2500 / 10^(snr/10)` (the standard JS8/FT8 2500 Hz-BW SNR), run the full `decode_window`, and
  count content-correct decodes. **Result: 12/12 down to −15 dB, 11/12 (~92%) at −18 dB, floor at −21 dB
  (1/12) — native decode PASSES the gate; no external-process fallback needed.** Committed as
  `gate_at_minus_18_db` (8 trials, requires ≥ 6/8 with margin; runs in ~0.26 s) plus an `#[ignore]`d
  `characterize_decode_floor` that prints the full sweep. Added to the CLAUDE.md acceptance table.
- **Implementation:** `plugins/js8/tests/snr_sweep.rs`; acceptance-table row in `CLAUDE.md`.
- **Tests / results (actually run):** `gate_at_minus_18_db` passes (decoded ≥ 6/8 at −18 dB); full sweep
  (ignored) reproduces the 12/12→11/12→1/12 curve; `js8-plugin` 55 lib + 1 gate passed / 0 failed; workspace
  builds 0 errors; clippy `-D warnings` + fmt clean.
- **Note:** Phase B (the highest-risk, FT8-class native RX) is functionally complete and **passes its go/no-go**
  — LDPC BP decode, soft demod, Costas sync, multi-decode window, plugin round-trip, −18 dB floor. Remaining
  FF-15: Phase C (message grammar + `@OPULSE` hint), D (RX-only discovery MVP), E–H. A Pi-class CPU-budget
  measurement (`cross`) and the bit-exact `reference_vectors` (gfortran) remain as later hardening.

---

## 2026-07-11 — feat(js8): FF-15 Phase B-5 — wire `Js8Plugin::demodulate` (trait round-trip)

- **Requirement/change:** FF-15 Phase B: complete the `ModulationPlugin` by wiring `demodulate` to the window
  decoder, so the plugin round-trips (modulate → demodulate) through the trait.
- **Design decision:** `demodulate` runs `decode_window` over the captured audio in a narrow band around
  `config.center_frequency` (offset 0 — the buffer is assumed slot-aligned; the discovery service uses
  `decode_window` directly for a full-passband, T/R-scheduled window) and returns the strongest CRC-valid
  frame as its packed 10-byte form (`payload9 ‖ i3bit<<5`), or `Err` if nothing decodes. **The plugin remains
  out of the engine's plugin registry by design** — not because the decoder was missing, but because JS8 is
  discovery-service-owned (plan §4.1/§4.2: T/R-slot scheduled, no `Frame` envelope); the daemon glue that owns
  the instance is Phase D.
- **Implementation:** `plugins/js8/src/plugin.rs` (`demodulate` body + module doc).
- **Tests:** `plugin.rs` — `demodulate` on silence → `Err`; **`modulate` → `demodulate` recovers the packed
  frame** (payload + flags) through the trait.
- **Test results (actually run):** `js8-plugin` 55 passed / 0 failed (1 new; `demodulate` test updated); clippy
  `-D warnings` + fmt clean.
- **Next:** B-6 — the −18 dB weak-signal go/no-go: a calibrated-AWGN SNR sweep measuring the decode floor
  (the D1 fallback checkpoint), plus a Pi-class CPU-budget check.

---

## 2026-07-11 — feat(js8): FF-15 Phase B-4 — window decoder (multi-decode + CRC-12 filter)

- **Requirement/change:** FF-15 Phase B: the window decoder — one entry the discovery service drives per
  received 15 s slot, returning *all* JS8 frames in it (JS8Call decodes dozens of overlapping stations), each
  guarded by its CRC-12 so a false decode never escapes.
- **Design decision:** `decode_window` = Costas sync search across the passband → keep above-threshold
  candidates → greedily separate peaks (≥ half a symbol in time, ≥ half a tone in frequency) so one strong
  signal isn't decoded many times → soft-demod + BP-decode each → **keep only CRC-12-valid frames** → dedup by
  content. The CRC is the trust gate (`check_info_crc` added to `message.rs`: re-split the 87 info bits, recompute
  the CRC-12, accept iff it matches). `DecodeCfg` exposes the passband/step/threshold/iteration knobs (defaults:
  300–2500 Hz, 3.125 Hz steps, min score 12/21, ≤ 32 candidates). `sync::sync_score` made `pub(crate)`.
- **Implementation:** `plugins/js8/src/decoder.rs` (`decode_window`, `Js8Decode`, `DecodeCfg`);
  `message.rs` `check_info_crc`; `lib.rs` module + re-exports.
- **Tests:** `decoder.rs` (3) — decodes a single frame at an unknown offset (exactly one CRC-valid result, right
  payload + offset); **decodes two overlapping stations at different base tones** in one window; **pure noise
  yields zero decodes** (the CRC-12 gate holds). `message.rs` (1) — `check_info_crc` accepts a valid frame and
  rejects a tampered payload bit.
- **Test results (actually run):** `js8-plugin` 54 passed / 0 failed (4 new); workspace builds 0 errors; clippy
  `-D warnings` + fmt clean.
- **Next:** wire `Js8Plugin::demodulate` to `decode_window` (return the best decode's packed frame) so the
  plugin round-trips through the engine, then the −18 dB weak-signal go/no-go gate (the D1 fallback checkpoint).

---

## 2026-07-11 — feat(js8): FF-15 Phase B-3 — Costas sync acquisition (full single-decode RX)

- **Requirement/change:** FF-15 Phase B: acquisition — find a slot's start offset and base tone frequency
  from the three Costas sync blocks, so the demod (B-2) can be handed an aligned slot at an unknown timing/
  frequency. Completes the single-decode RX chain.
- **Design decision:** the FT8-class 2-D search over `(time offset × base frequency)`. Score each candidate by
  how much of every sync symbol's energy sits on its expected Costas tone, **normalized by that symbol's total
  energy** (DSP playbook: "acquire on the normalized correlation, not the unnormalised score" — a high-energy
  noise window must not win). A perfect lock scores 21 (one per sync symbol). Coarse grid for now (whole-symbol
  offsets, few-Hz frequency steps); a fine time/frequency refinement is a later unit if the −18 dB gate needs it.
- **Implementation:** `plugins/js8/src/sync.rs` (`find_sync`, `SyncCandidate`, `sync_score`,
  `preamble_samples`); `lib.rs` module + re-exports.
- **Tests:** `sync.rs` (3) — recovers the exact slot offset + base frequency (score > 18/21) from a signal
  buried in leading silence; **`find_sync` → `demodulate_soft` → `bp_decode` recovers the message** at an
  unknown offset and base tone (the full single-decode RX pipeline); the true offset scores well above a
  leading-silence offset.
- **Test results (actually run):** `js8-plugin` 50 passed / 0 failed (3 new); workspace builds 0 errors; clippy
  `-D warnings` + fmt clean.
- **Next:** B-4 — the `decode_window` pipeline (candidate list from the sync search → decode each → CRC-12
  filter → dedup → multiple decodes per 15 s window), then wire `Js8Plugin::demodulate` + the −18 dB go/no-go.

---

## 2026-07-11 — feat(js8): FF-15 Phase B-2 — soft demodulator (first RX round-trip)

- **Requirement/change:** FF-15 Phase B: the soft demodulator — turn a synced slot's audio into the 174 bit
  LLRs the BP decoder (B-1) consumes, and prove the RX chain end to end.
- **Design decision:** non-coherent 8-FSK max-log demod. Per data symbol, measure the eight tone energies by
  Goertzel (at 6.25 Hz spacing the 1280-sample window resolves exactly one tone bin), then convert to three
  bit LLRs over the **direct-binary** tone map (A-6): `LLR_i = (max energy over tones with bit i = 1 − max
  over bit i = 0) / noise`, sign matching the decoder (`> 0` ⇒ bit 1). Estimate the noise scale from each
  symbol's non-peak tone energies (the seven losers are mostly noise) — a self-calibrating scale per the
  repo's LLR discipline. Assumes perfect sync for now (audio aligned to symbol 0, base tone known); Costas
  acquisition is B-3.
- **Implementation:** `plugins/js8/src/demodulate.rs` (`demodulate_soft`, `symbol_tone_energies`,
  `tone_energies_to_llrs`, `goertzel_energy`); `tones::data_positions` made `pub(crate)`; `lib.rs` module.
- **Tests:** `demodulate.rs` (4) — LLR sign vs a known tone (5 = `0b101`); **the first true RX round-trip —
  `message → tones → GFSK audio → demodulate_soft → bp_decode → message`** over a clean channel (4 seeds);
  the same **survives moderate additive white noise**; per-symbol energies peak on the transmitted tone.
- **Test results (actually run):** `js8-plugin` 47 passed / 0 failed (4 new); workspace builds 0 errors; clippy
  `-D warnings` + fmt clean.
- **Next:** B-3 Costas sync/acquisition (freq×time correlation to find the slot start + base tone), then B-4
  the `decode_window` multi-decode pipeline + CRC-12 dedup, then the −18 dB weak-signal go/no-go.

---

## 2026-07-11 — feat(js8): FF-15 Phase B-1 — LDPC(174,87) belief-propagation decoder

- **Requirement/change:** FF-15 Phase B (the RX decoder — the highest-risk, FT8-class track): the FEC-decode
  half — recover the 87 info bits from 174 soft LLRs, correcting errors the channel introduced.
- **Design decision:** port the sum-product BP decoder from JS8Call `lib/ft8/bpdecode174.f90` — vendoring the
  remaining table it needs, `Mn` (per-variable check incidence, column weight 3), alongside the already-ported
  `Nm`/`nrw`/`colorder`/`g`. Standard tanh-domain message passing (init check msgs from the LLRs; per iter:
  total belief `zn = llr + Σ tov`, hard-decide, check the parity syndrome and return on convergence, else
  update check inputs, `tanh(-toc/2)`, and variable messages `2·atanh(-Π extrinsic)`), including the upstream
  early-stop (bail when unsatisfied-check count stops falling). Sign convention matches upstream (`llr > 0` ⇒
  bit 1); `platanh` is a clamped exact `atanh` (upstream uses a piecewise-poly approximation of the same
  function — functionally equivalent for decode). Validated **functionally** (encode → corrupt → recover),
  which is the right gate for a decoder; the −18 dB weak-signal go/no-go is a later end-to-end unit.
- **Implementation:** `plugins/js8/src/ldpc174.rs` (`MN` table, `bp_decode`, `BpDecode`, `platanh`); `lib.rs`
  re-exports.
- **Tests:** `ldpc174.rs` (4 new) — decodes a clean codeword (info == message); **corrects 1/3/6/10 flipped
  bits**; recovers under additive Gaussian LLR noise (σ ≈ 1.2 on ±2.5 LLRs); rejects pure-noise LLRs without
  hanging (or returns a genuine zero-syndrome codeword).
- **Test results (actually run):** `js8-plugin` 43 passed / 0 failed (4 new); lib builds warning-free; clippy
  `-D warnings` + fmt clean.
- **Next:** B-2 soft demod (per-symbol 8-tone energies → calibrated LLRs), B-3 Costas sync/acquisition
  (freq×time correlation search), B-4 the `decode_window` pipeline (multi-decode + CRC-12 dedup), then the
  −18 dB loopback/reference gate.

---

## 2026-07-11 — feat(js8): FF-15 Phase A-8 — `Js8Plugin` ModulationPlugin (TX chain complete)

- **Requirement/change:** FF-15 Phase A close: expose the JS8 waveform as a `ModulationPlugin` and wire the
  full transmit chain end to end — a packed JS8 message → GFSK audio.
- **Design decision:** `Js8Plugin::modulate` runs packed-message (10 bytes: 72-bit payload + 3-bit flags +
  5 pad) → `js8_info_bits` → LDPC `encode174` → `message_to_tones` → `modulate_tones`, using the mode's
  submode params + Costas. `demodulate` is the FT8-class weak-signal receiver (plan Phase B) — it returns a
  clear `ModemError::Demodulation("…not implemented (Phase B)")` rather than a silent stub, and **the plugin
  is deliberately not registered in the daemon** until the decoder lands (a modulate-only plugin would break
  RX routing). `frame_geometry` reports one slot = one frame (`min = max = 79·sps`); `occupied_bandwidth_hz`
  feeds the bandplan width checks. Added `openpulse-core` as the crate's first dependency. Per plan §4.2 the
  discovery service will call `modulate` directly (JS8 must not carry the OpenPulse `Frame` envelope).
- **Implementation:** `plugins/js8/src/plugin.rs` (`Js8Plugin`, `split_message`); `Cargo.toml`
  `openpulse-core` dep; `lib.rs` module + re-export.
- **Tests:** `plugin.rs` (4) — info/geometry/bandwidth; unknown mode → `Err`; `demodulate` → `Err`; **the full
  TX chain: `modulate` output's 79 symbols Goertzel-decode back to exactly `message_to_tones(js8_info_bits(…))`**
  — an end-to-end validation of packing→CRC→LDPC→tones→GFSK (short of a bit-exact WAV compare vs `genjs8`, which
  needs gfortran).
- **Test results (actually run):** `js8-plugin` 39 passed / 0 failed (4 new); workspace builds 0 errors; clippy
  `-D warnings` + fmt clean.
- **Phase A TX core is complete** (submode/Costas, GFSK, LDPC, CRC-12, packers, tones, message bits, plugin —
  8 units, PRs #744–#751, all upstream-anchored). **Next:** Phase B (RX decoder — highest-risk, FT8-class,
  the go/no-go), or the end-to-end `reference_vectors` gate once `genjs8` ground truth is available (gfortran).

---

## 2026-07-11 — feat(js8): FF-15 Phase A-7 — message-bit assembly (payload+flags+CRC → 87 info bits)

- **Requirement/change:** FF-15 Phase A: the `genjs8.f90` head — turn a 72-bit payload + 3-bit transmission
  flags into the 87 LDPC info bits, protected by the JS8 message CRC-12.
- **Design decision:** port the exact `genjs8` layout — CRC-12 over the 11-byte buffer
  `[payload(72) | i3bit<<5 | 0]`, XORed with 42, then info bits `[payload(72) | i3bit(3) | crc(12)]`.
  Validate the CRC (the only non-trivial part) **against ground truth from a g++ harness that replicates the
  `genjs8` byte assembly around the real `boost::augmented_crc<12, 0xc06>`** — so the message CRC is anchored
  to upstream Boost, not just self-consistency. The *semantic* packing of callsign/grid/command into the
  72-bit payload (`packCompoundFrame`) is the message-grammar layer (plan Phase C), not the TX waveform core.
- **Implementation:** `plugins/js8/src/message.rs` (`js8_message_crc12`, `js8_info_bits`); `lib.rs` module +
  re-exports.
- **Tests:** `message.rs` (4) — **12 CRC ground-truth vectors**; all-zero payload → the bare XOR (`0x02a` =
  42); info-bit layout is payload‖flags‖crc; the assembled 87 bits → `message_to_tones` → `tones_to_codeword`
  is **parity-valid** (`H·c = 0`), exercising the full CRC→LDPC→tones chain end to end.
- **Test results (actually run):** `js8-plugin` 35 passed / 0 failed (4 new); clippy `-D warnings` + fmt clean.
- **Next:** the `ModulationPlugin` impl (packed message bytes → `js8_info_bits` → `message_to_tones` →
  `modulate_tones` audio) + daemon registration, then Phase-C message grammar/hint and Phase-B RX decoder.

---

## 2026-07-11 — feat(js8): FF-15 Phase A-6 — tone assembly (codeword → 79 symbols)

- **Requirement/change:** FF-15 Phase A: bridge the LDPC codeword (A-3) to the GFSK modulator (A-2) — map
  the 174-bit codeword into the 79-symbol tone sequence, interleaving the three Costas sync blocks.
- **Design decision + finding:** the plan (§2.1) said "Gray-coded 8-FSK", but **the JS8 source is not
  Gray-coded** — `genjs8.f90` writes `itone(k) = codeword[i]*4 + codeword[i+1]*2 + codeword[i+2]` (direct
  binary), and the JS8 lib has *no* gray table anywhere (`grep -ri gray lib/` is empty). Verified against
  source per the DSP playbook ("verify against source, not the plan"); ported the exact `S7 D29 S7 D29 S7`
  layout — data at 0-based positions 7–35 and 43–71, sync (Costas) at 0–6/36–42/72–78. Added the inverse
  `tones_to_codeword` and a `message_to_tones` (encode174 ∘ codeword_to_tones) tying LDPC + tones.
- **Implementation:** `plugins/js8/src/tones.rs` (`codeword_to_tones`, `tones_to_codeword`,
  `message_to_tones`, `data_positions`); `lib.rs` module + re-exports.
- **Tests:** `tones.rs` (4) — `S7 D29 S7 D29 S7` structure (sync = Costas arrays, no data in a sync block,
  data positions 7/35/43/71, all tones < 8); the direct-binary rule per data symbol; codeword round-trips
  through tones; `message_to_tones` → `tones_to_codeword` recovers a **parity-valid** codeword (`H·c = 0`).
- **Test results (actually run):** `js8-plugin` 31 passed / 0 failed (4 new); clippy `-D warnings` + fmt clean.
- **Next:** the JS8 message-payload assembly (`packHeartbeatMessage` 72-bit layout + i3bit flags + 11-byte
  CRC buffer → 87 info bits) + the `ModulationPlugin` impl (message bytes → tones → GFSK audio) + daemon
  registration; then the end-to-end `reference_vectors` gate (needs `genjs8` ground truth — gfortran).

---

## 2026-07-11 — feat(js8): FF-15 Phase A-5 — frame field packers (callsign + grid, Qt-exact)

- **Requirement/change:** FF-15 Phase A: pack the two fields a JS8 Heartbeat frame carries — a standard
  callsign into 28 bits (`packCallsign`) and a Maidenhead grid into 15 bits (`packGrid`) — bit-exactly to
  JS8Call (`varicode.cpp`).
- **Design decision:** the sandbox has Qt5, so **compile the verbatim upstream `Varicode::packCallsign`/
  `packGrid`/`grid2deg` against real Qt** and emit ground-truth (field → integer) vectors, then write an
  **independent** Rust port (own regex-class scan replacing `QRegularExpression`, own mixed-radix over the
  `alphanumeric` alphabet, own `grid2deg`) and validate it against those vectors. The two implementations are
  separate (different language, different string/regex machinery), so a match is a genuine cross-check, not a
  transcription echoing itself — the strongest validation short of the full `reference_vectors` gate. Ported
  the `/P` strip and Swaziland/Guinea prefix workarounds verbatim; group/hashed calls (the `basecalls` map,
  needed for `@OPULSE`) and directed `packCmd` land with the message-grammar unit.
- **Implementation:** `plugins/js8/src/frame.rs` (`pack_callsign`, `pack_grid`, `grid2deg`, window/alphabet
  helpers, `GRID_INVALID`); `lib.rs` module + re-exports.
- **Tests:** `frame.rs` (5) — **11 callsign vectors** (incl. 3-char calls + the `3DA0…` workaround) and **8
  grid vectors** from verbatim-upstream-on-Qt; case/whitespace insensitivity; unpackable callsign → 0;
  short grid → `GRID_INVALID`.
- **Test results (actually run):** `js8-plugin` 27 passed / 0 failed (5 new); workspace builds 0 errors; clippy
  `-D warnings` + fmt clean.
- **Next:** the JS8 message assembly (payload+flags → 11-byte CRC buffer → 87 info bits → LDPC → Gray/Costas
  tone sequence) wiring callsign/grid/CRC/LDPC/modulate together, then the end-to-end `reference_vectors` gate.

---

## 2026-07-11 — feat(js8): FF-15 Phase A-4 — CRC-12 primitive (boost-exact, vector-gated)

- **Requirement/change:** FF-15 Phase A: the 12-bit CRC that turns JS8's 75-bit message into the 87 info
  bits the LDPC(174,87) code encodes. Upstream (`lib/crc12.cpp`) uses `boost::augmented_crc<12, 0xc06>`, an
  unreflected MSB-first modulo-2 division with the check bits augmented in (appending the CRC divides the
  word to zero). In the prior checkpoint I flagged CRC-12 as needing ground truth (self-consistency alone is
  weak — two internally-consistent CRCs can disagree on the wire).
- **Design decision:** the sandbox has g++ + Boost, so **compile the real `boost::augmented_crc<12, 0xc06>`
  and emit ground-truth (buffer → CRC) vectors**, then port a Rust `augmented_crc12` and validate bit-exactly
  against those 20 vectors — turning the earlier caveat into a solved, upstream-anchored gate. Also verify the
  Boost augmented self-check invariant (append the 12-bit CRC into the trailing 12 bits → whole buffer CRCs to
  zero), independently confirmed with a second g++ harness. The JS8 composite (11-byte `genjs8.f90` assembly +
  `XOR 42`) is deferred to the frame-assembly unit, where the payload/flag layout it needs lives; constants
  `CRC12_POLY`/`JS8_CRC12_XOR` are exposed for it.
- **Implementation:** `plugins/js8/src/crc.rs` (`augmented_crc12`, `CRC12_POLY`, `JS8_CRC12_XOR`); `lib.rs`
  module + re-export. (Reference generators were throwaway g++ harnesses, not committed.)
- **Tests:** `crc.rs` (3) — **20 boost ground-truth vectors** (lengths 1–22); empty/all-zero → 0; the
  append-CRC-divides-to-zero invariant.
- **Test results (actually run):** `js8-plugin` 22 passed / 0 failed (3 new); clippy `-D warnings` + fmt clean.
- **Next:** frame packing (`packCallsign` 28-bit EME-2000 / `packGrid` 15-bit / `packCmd`) + the JS8 message
  assembly (payload+flags+CRC-12 → 87 info bits → LDPC → Gray/Costas tones), then the `reference_vectors` gate.

---

## 2026-07-11 — feat(js8): FF-15 Phase A-3 — LDPC(174,87) encoder + parity tables

- **Requirement/change:** FF-15 Phase A (`docs/dev/design/js8-discovery-rendezvous-plan.md` §2.1, §4.2,
  Appendix B): the FEC half of the JS8 TX core — encode 87 info bits (75 message + 12 CRC) into the 174-bit
  LDPC codeword that becomes the 58 data symbols. This is the *old* FT8 v1 (174,87) code JS8 froze on.
- **Design decision:** vendor the authoritative tables from JS8Call/js8call (**GPL-3.0**, license-compatible
  with this repo) rather than reconstruct them — parity generator `g` + `colorder` from
  `lib/ft8/ldpc_174_87_params.f90`, and the parity-check incidence `Nm`/`nrw` from `lib/ft8/bpdecode174.f90`
  — extracted programmatically (no hand-transcription) with provenance cited in the module header. Port
  `encode174` exactly (`pchecks = G·message` over GF(2), `[pchecks|message]` reordered by `COLORDER`) and add
  a `parity_syndrome` (`H·codeword`). **The acceptance gate is `H·encode174(m) = 0` for many random messages**
  — a single wrong generator/colorder/Nm entry breaks some check on some message, so a green sweep proves the
  three tables are correctly ported and mutually consistent, *without* needing an external codeword vector.
  Belief-propagation decode is Phase B; CRC-12 assembly (boost augmented-CRC + XOR-42) and frame packing are
  their own later units (each deserves a reference vector).
- **Implementation:** `plugins/js8/src/ldpc174.rs` (`G_HEX`/`COLORDER`/`NM`/`NRW` tables, `encode174`,
  `parity_syndrome`, `gen_bit`); `lib.rs` module.
- **Tests:** `ldpc174.rs` (4) — table sizes + COLORDER is a permutation + `nrw` matches `NM` nonzero counts;
  **every codeword satisfies all parity checks** (500 pseudo-random messages, deterministic LCG); all-zero
  message → all-zero codeword (linearity); systematic message bits recoverable at their reordered positions.
- **Test results (actually run):** `js8-plugin` 19 passed / 0 failed (4 new); workspace builds 0 errors; clippy
  `-D warnings` + fmt clean.
- **Next:** CRC-12 (75-bit message → 87 info bits) + frame packing (`packCallsign`/`packGrid`/`packCmd`) +
  Gray/Costas tone assembly, then the bit/tone-exact `reference_vectors` gate against committed JS8Call vectors.

---

## 2026-07-11 — feat(js8): FF-15 Phase A-2 — GFSK tone-synthesis modulator

- **Requirement/change:** FF-15 Phase A (`docs/dev/design/js8-discovery-rendezvous-plan.md` §2.1, §4.2): the
  waveform-synthesis half of the JS8 TX core — turn a symbol→tone sequence into continuous-phase,
  Gaussian-frequency-smoothed 8-FSK audio. Rectangular FSK would decode locally but splatter adjacent JS8
  users (fails interop goal G1), so GFSK is required.
- **Design decision:** port the FT8/JS8 `gen_ft8wave` + `gfsk_pulse` synthesis (`modulate.rs`): a 3-symbol
  Gaussian pulse (BT = 2.0) whose one-symbol-shifted copies form a partition of unity, so a tone run
  synthesizes to that tone's steady frequency and transitions smooth over ~one symbol; instantaneous
  frequency `= base + spacing · Σ_j tone[j]·pulse_j` is integrated into continuous phase, with a
  raised-cosine keying ramp on both ends. Keep it a **pure tone→audio** function (`GfskParams`,
  `modulate_tones`) — bits→tones (Gray coding, Costas insertion, LDPC) is upstream and lands with the frame/
  LDPC units. `erf` via Abramowitz-Stegun 7.1.26 (no libm dependency). Verifiable now *without* upstream
  vectors by structural properties; exact BT/envelope match against JS8Call-improved is the later
  `reference_vectors` gate.
- **Implementation:** `plugins/js8/src/modulate.rs` (`GfskParams`, `gfsk_pulse`, `modulate_tones`, `erf`);
  `lib.rs` re-exports.
- **Tests:** `modulate.rs` (5) — output length = `79·sps` and amplitude ≤ 1; empty tones → no audio; **a
  constant tone lands on `base + tone·spacing`** (Goertzel argmax over the 8 candidates, every tone 0–7);
  `gfsk_pulse` partition-of-unity in the interior; a 0→7 tone jump is phase-continuous (bounded first
  difference), not a rectangular step.
- **Test results (actually run):** `js8-plugin` 15 passed / 0 failed (5 new); clippy `-D warnings` + fmt clean.
- **Next:** frame packing + LDPC(174,87)/CRC-12 (produce the tone sequence from a message), then the
  `reference_vectors` bit/tone-exact gate against committed JS8Call-improved ground truth.

---

## 2026-07-11 — feat(js8): FF-15 Phase A-1 — `plugins/js8` crate (submode table + Costas arrays)

- **Requirement/change:** FF-15 Phase A (`docs/dev/design/js8-discovery-rendezvous-plan.md` §11): begin the
  JS8-compatible waveform with its self-contained protocol foundation — the submode parameter table and the
  Costas sync arrays — before the DSP-heavy encode/decode.
- **Design decision:** a new `plugins/js8` crate (workspace member `js8-plugin`). Transcribe the two
  interop-critical constant sets from the plan's source-verified sections — submode table (§2.2, `commons.h`/
  `JS8Submode.cpp`) and Costas arrays (§2.1, `JS8.h:24–36`) — and guard them with **internal-consistency
  tests** that need no external ground truth: samples/symbol is an exact integer at 8 kHz and matches the
  table, `bandwidth = 8 × tone_spacing`, tabulated TX durations (`79 × sps / 8000`), NORMAL period = 101 120
  samples, only NORMAL uses ORIGINAL Costas, each Costas array is 7 distinct valid tones, and the three
  MODIFIED blocks are pairwise distinct (the anti-false-sync property). The bit/tone-exact `reference_vectors`
  gate is deferred to a later Phase-A unit because it needs committed JS8Call-improved ground-truth vectors;
  no `ModulationPlugin` impl yet (it needs `modulate`, which is the GFSK unit), so the crate is not registered
  in the daemon — pure data + helpers, no half-implemented trait.
- **Implementation:** `plugins/js8/{Cargo.toml, src/lib.rs, src/submode.rs, src/costas.rs}`;
  `Cargo.toml` workspace members + `js8-plugin` path dep.
- **Tests:** `submode.rs` (6 — integer sps, BW, durations, NORMAL period, Costas-kind mapping, case-insensitive
  `params_for_mode` + unknown rejection); `costas.rs` (4 — distinct valid tones, pairwise-distinct MODIFIED,
  `block` dispatch, `sync_map` places 21 sync / 58 data at 0–6/36–42/72–78).
- **Test results (actually run):** `js8-plugin` 10 passed / 0 failed; workspace builds 0 errors; clippy
  `-D warnings` + fmt clean.
- **Next:** frame packing (`packCallsign`/`packGrid`/`packCmd`), LDPC(174,87) encode + CRC-12, GFSK `modulate`,
  then the `reference_vectors` bit/tone-exact gate (needs upstream ground-truth vectors committed).

---

## 2026-07-11 — feat(filexfer): airtime-bounded TX burst splitting (real-radio PTT sequencing)

- **Requirement/change:** FF-16 §5.3 — the file-transfer TX drain keyed PTT **once** and transmitted the
  entire queue as a single transmission. On real half-duplex radio a large transfer would hold PTT past the
  ~180 s watchdog and never yield the channel; the drain must split the queue into airtime-bounded bursts.
- **Design decision:** size bursts by **measured airtime**, not a fragile per-mode bitrate table. New pure
  `ModemEngine::estimate_air_secs(payload_len, mode)` modulates a throwaway buffer through the mode's *real*
  modulator (the trait's `modulate` is side-effect-free — it does not bump the wire sequence, key PTT, or emit
  audio) and returns `samples / sample_rate`. A pure `plan_bursts(n, air_secs, burst_max_secs, max_frags)`
  greedily packs fragments until the next would exceed `burst_max_secs` (default 20 s, well under the watchdog),
  clamped to `MAX_FRAGS_PER_BURST = 64` (plan §5.3), always taking ≥1 fragment so a lone oversized fragment
  still forms its own burst. `drain_filexfer_tx` plans up front (immutable engine borrow), then keys each burst
  as its own assert → transmit → release cycle (`PttChanged` per burst). Splitting the decision (`plan_bursts`,
  pure) from the keying makes it unit-testable with **no hardware** — the USB/loopback rig is only for the later
  Phase-F on-air keying-timing smoke test. `burst_max_secs` is a new `[file_transfer]` config knob.
- **Implementation:** `crates/openpulse-modem/src/engine.rs` (`estimate_air_secs`); `crates/openpulse-daemon/
  src/server.rs` (`plan_bursts`, `MAX_FRAGS_PER_BURST`, rewritten `drain_filexfer_tx`); `openpulse-config`
  (`FileTransferConfig.burst_max_secs` + template); `crates/openpulse-daemon/src/filexfer.rs`
  (`FileTransferPolicy.burst_max_secs`).
- **Tests:** `crates/openpulse-modem/tests/estimate_air_secs.rs` (4 — positive + monotonic in size; slower mode
  takes longer; unknown mode → `None`; emits no audio / no wire-sequence disturbance); `server.rs`
  `burst_planning_tests` (6 — empty; one burst; airtime split 3+3+3+1; oversized-fragment own burst; fragment
  clamp 64+64+22; per-fragment airtime respected).
- **Test results (actually run):** `openpulse-modem` + `openpulse-daemon` + `openpulse-config` 470 passed / 0
  failed (10 new); workspace builds 0 errors; clippy `-D warnings` + fmt clean.
- **Note:** the existing twin round-trip still exercises the drain end-to-end (tiny file = 1 burst). Completes
  the real-radio PTT sequencing refinement flagged after PR #739. Live keying-timing validation is Phase F.

---

## 2026-07-10 — feat(filexfer): Phase E-2 — daemon partial-block persistence + resume detection

- **Requirement/change:** FF-16 Phase E (`docs/dev/design/file-transfer-plan.md` §12): give the E-1 resume
  mechanic something to resume *from* — persist completed blocks across an interrupted transfer, detect the
  resume on a re-offer, and clean up (delete on completion, TTL-purge stragglers).
- **Design decision:** persist each just-completed block to `download_dir/<peer>/.partial/<sha256hex>/<n>.blk`,
  **keyed by the offer's content hash** (not the transfer-id, which is fresh per `SendFile`), so a re-offer of
  the same file finds its earlier blocks. On a new offer the daemon TTL-purges the peer's stale partials, loads
  the `.blk` files whose length matches the expected block size (a cheap corruption guard — a bad block is just
  re-fetched; the whole-file manifest hash is still the authority), `seed_block`s them into the `BlockAssembler`,
  and calls `ReceiverSession::resume(&held)` instead of `new` so the held blocks are announced and skipped. On
  full reassembly the partial dir is deleted. Persistence is best-effort (a write failure only forfeits
  resumability, never the live transfer). New crate surface: `BlockAssembler::seed_block` / `block`. New config:
  `[file_transfer] partial_ttl_hours` (default 72; 0 = keep indefinitely).
- **Implementation:** `crates/openpulse-filexfer/src/blocks.rs` (`seed_block`, `block`); `openpulse-config`
  (`FileTransferConfig.partial_ttl_hours` + template); `crates/openpulse-daemon/src/filexfer.rs`
  (`FxRxState.partial_dir`, `FileTransferPolicy.partial_ttl_hours`, `partial_dir_for`, `expected_block_len`,
  `persist_block`, `load_partials`, `clear_partials`, `purge_stale_partials`; `on_offer` resumes,
  `on_block_fragment` persists, `reassemble_verify_write` clears).
- **Tests:** `crates/openpulse-filexfer/tests/blocks.rs` (`seeded_blocks_complete_without_fragments`);
  `crates/openpulse-daemon/src/filexfer.rs` `#[cfg(test)]` (persist→reload into held mask + assembler;
  wrong-length partial skipped; `clear_partials` removes the dir; content-hash keying; fresh partial survives
  purge + ttl-0 no-op; last-block short length).
- **Test results (actually run):** `openpulse-filexfer` 27 passed (1 new); `openpulse-daemon` 83 passed (8 new
  filexfer unit tests); `openpulse-config` unchanged-green; combined 123 passed / 0 failed; clippy `-D warnings`
  + fmt clean.
- **Note:** completes FF-16 Phase E. Remaining: Phase F (on-air validation, deferred batch).

---

## 2026-07-10 — feat(filexfer): Phase E-1 — block-level resume state mechanic

- **Requirement/change:** FF-16 Phase E (`docs/dev/design/file-transfer-plan.md` §12): an interrupted
  transfer must not re-send blocks the receiver already holds. The state machines needed a way to carry a
  per-block "already have it" set and negotiate it on the wire.
- **Design decision:** reuse the already-specified `FileAccept.have_bitmap` (a per-block held mask) as the
  resume channel — no new frame. The receiver constructor gains a sibling `ReceiverSession::resume(offer,
  decision, held, …)`; `new()` delegates to it with an empty `held`, so every existing call site and test is
  unchanged. On accept it announces `bitmap_from_bools(held)` and pre-counts those blocks done (an all-held
  resume goes straight to `Verify`, receiving nothing). The sender records the accepted bitmap into a `held`
  mask and walks blocks via `next_unheld(from)` on both the initial-accept and per-`BlockAck` advance, so it
  skips held blocks and never sends one twice; an all-held accept goes straight to `AwaitVerify`. Kept the
  daemon-side persistence (writing/reading partial blocks) for E-2 so this PR is pure, in-crate, and testable.
- **Implementation:** `crates/openpulse-filexfer/src/receiver.rs` (`held` field, `resume`, `begin_receiving`
  emits the held bitmap + all-held→Verify edge, `bitmap_from_bools`); `sender.rs` (`held` field, `next_unheld`,
  `bit_is_set`, FileAccept/BlockAck arms honor the bitmap).
- **Tests:** `crates/openpulse-filexfer/tests/resume.rs` (4) — sender skips a held block and advances over it;
  all-held sender sends nothing then completes on `FileComplete`; receiver announces its held bitmap and counts
  those done; all-held receiver verifies immediately.
- **Test results (actually run):** `openpulse-filexfer` 26 passed / 0 failed (4 new + 22 prior); `openpulse-daemon`
  75 passed / 0 failed (unchanged via `new()` delegation); clippy `-D warnings` + fmt clean.
- **Next:** Phase E-2 — daemon block persistence (`download_dir/<peer>/.partial/<sha256hex>/`, resume detection
  on re-offer, seed `BlockAssembler` + `ReceiverSession::resume`, delete on completion, TTL purge).

---

## 2026-07-10 — feat(filexfer): real-radio PTT burst sequencing (queue + drain)

- **Requirement/change:** FF-16 §5.3/§6.4 — file-transfer transmits happen inside
  `apply_command_to_engine` / `process_received_bytes`, which don't hold the PTT controller (it lives in
  `server::run`). On real half-duplex radio each burst must key PTT so the peer can answer; the previous
  event-reactive path transmitted inline without keying (fine on the twin, wrong on air).
- **Design decision:** a **pending-transmit queue**. The `filexfer` module no longer touches the modem — it
  SAR-encodes control frames (`enqueue_ctrl`) and blocks (`enqueue_block`) onto
  `RuntimeControlState::filexfer_tx_queue` (`(fragment, mode)`); `server::run` drains the queue via
  `drain_filexfer_tx` as **one PTT-keyed burst** (assert → transmit all → release → `PttChanged` events)
  after each command and receive tick. This keeps the module I/O-free, puts the half-duplex sequencing with
  the controller, and the two-daemon twin round-trip now exercises the real drain path. `engine` was removed
  from every module transmit function.
- **Implementation:** `crates/openpulse-daemon/src/filexfer.rs` (`enqueue_ctrl`/`enqueue_block`, engine
  removed from the transmit fns); `lib.rs` (`RuntimeControlState.filexfer_tx_queue`; call sites drop `engine`);
  `server.rs` (`drain_filexfer_tx`, drained after the command arm + rx tick).
- **Tests:** the existing receive/send daemon tests and the **twin round-trip** all pass unchanged — the twin
  test (`a_file_crosses_the_bridge_between_two_real_daemons`) now transmits through the queue → drain →
  PTT-keyed path, so it validates the real-radio sequencing.
- **Test results (actually run):** full `openpulse-daemon` suite 75 passed / 0 failed (incl. twin); clippy
  `-D warnings` + fmt clean.
- **Note:** one PTT keying per drain (all queued fragments as a burst); airtime-bounded burst splitting
  (§5.3 `fragments_per_burst`) is a later refinement.

---

## 2026-07-10 — feat(panel): Phase D — Files tab (send, offer prompt, progress, verify badge)

- **Requirement/change:** FF-16 Phase D (`docs/dev/design/file-transfer-plan.md` §7): surface file transfer
  in the operator panel — send a file, respond to an inbound offer, watch progress, and see received files
  with the signed-manifest verify badge.
- **Design decision:** follow the existing panel patterns (Statistics/Messages tabs): a new `Tab::Files`;
  `PanelState` gains `incoming_offer` / `active_transfer` / `received_files` / `file_status`, populated in
  `connection::apply_event` from the `FileOffered`/`FileProgress`/`FileReceived`/`FileSent`/`FileFailed`
  events (replacing the Phase-C catch-all); `App` gains `file_to` / `file_path` inputs and `Message` variants
  (`SendFile`/`AcceptFile`/`RejectFile`/`CancelFile`) that dispatch the control commands via `self.send`. The
  `files_widget` renders a send box, an inbound-offer Accept/Reject prompt, a progress line with Cancel, and a
  verify-badged received-file list (green `✓ verified` / red `UNVERIFIED`).
- **Implementation:** `apps/openpulse-panel/src/state.rs` (`IncomingOffer`/`ActiveTransfer`/`ReceivedFile`
  + fields), `connection.rs` (event reducer arms), `app.rs` (`Tab::Files`, inputs, `Message` variants + update
  arms), `ui.rs` (tab button/dispatch, `Snap` fields, `files_widget`).
- **Tests:** `connection::file_event_tests` — `FileOffered` prompts unless auto-accepted; `FileReceived`
  records the file + clears the offer + sets a verified status; `FileProgress` then `FileFailed` clears the
  active transfer. (First reducer unit tests for the panel.)
- **Test results (actually run):** `openpulse-panel` 18 passed / 0 failed (3 new); clippy `-D warnings` + fmt
  clean; panel builds.
- **Next:** Phase E (resume) + real-radio PTT burst sequencing; Phase F on-air.

---

## 2026-07-10 — test(filexfer): Phase C acceptance — a file crosses two real daemons (twin round-trip)

- **Requirement/change:** FF-16 Phase C acceptance (`docs/dev/design/file-transfer-plan.md` §11/§12): prove a
  file transfers end-to-end between two real daemons over the modem — the MVP ship-line gate. Also make the
  receiver always persist a received file with an accurate verify badge (never silently drop).
- **Design decision:** `reassemble_verify_write` now evaluates the two integrity axes independently — content
  *intact* iff its SHA-256 matches the offer's `payload_hash`, *authenticated* iff a verified-peer key also
  validates the signature — and **always writes the file** with `verified = intact && authenticated`
  (`.unverified` quarantine otherwise), so an unsigned/unknown peer's intact file still lands (badge false)
  rather than being dropped on the signature check. `FileComplete.status` reflects `VerifiedOk` /
  `HashMismatch` / `SignatureInvalid`. The twin test drives the whole flow through the real control protocol.
- **Implementation:** `crates/openpulse-daemon/src/filexfer.rs` (`reassemble_verify_write` rewrite; `sha2` +
  `verify_manifest` imports); `crates/openpulse-daemon/Cargo.toml` (`sha2` dep).
- **Tests:** `crates/openpulse-daemon/tests/twin_daemon_bridge.rs::a_file_crosses_the_bridge_between_two_real_daemons`
  — two bridged real daemons (`spawn_bridged_pair`, clean AWGN), `SendFile` on A via TCP control → B emits
  `FileReceived` and the written file reads back **byte-for-byte** equal to the sent bytes.
- **Test results (actually run):** twin round-trip passes (0.75 s); full `openpulse-daemon` suite 75 passed /
  0 failed (verified-peer receive test still green); clippy `-D warnings` + fmt clean.
- **MVP ship line reached:** file transfer works both directions, is operator-usable via the control port,
  and is proven end-to-end across two real daemons. Remaining polish: panel Files tab (Phase D), resume
  (Phase E), real-radio PTT burst sequencing, and on-air validation (Phase F).

---

## 2026-07-10 — feat(filexfer): Phase C-3 — the send session (SendFile → offer → blocks → FileSent)

- **Requirement/change:** FF-16 Phase C-3 (`docs/dev/design/file-transfer-plan.md` §6.3): the daemon's
  outbound file-transfer session — the operator-usable other half of the MVP.
- **Design decision:** the send side mirrors the receive side and is **event-reactive**: `SendFile` reads +
  size-checks the file, signs its manifest with the station key (`local_callsign` + `station_seed`), builds
  a `SenderSession`, and transmits the offer; the receiver's `FileAccept`/`BlockAck`/`FileComplete` control
  frames arrive on the **same C-2a seam** and drive the next block out (`drive_tx_actions` materializes each
  `SendBlock` via `encode_block` over the file's byte range and transmits its fragments). Terminal `Sent`
  emits `FileSent { receipt_valid }`. Because delivery reacts to inbound ACKs, the loopback/twin path needs
  no separate tick loop; real-radio PTT burst sequencing is a `server::run` refinement layered on top (the
  loopback backend keys nothing). `SendFile` is handled in `apply_command_to_engine` beside the receive
  commands (engine + event_tx are there); one transfer per link (`file_tx`) is enforced.
- **Implementation:** `crates/openpulse-daemon/src/filexfer.rs` (`FxTxState`, `send_file`, `on_tx_frame`,
  `on_tx_cancel`, `drive_tx_actions`; the control dispatch now routes receiver→sender frames to the send
  session); `lib.rs` (`RuntimeControlState.file_tx`; `SendFile` arm).
- **Tests:** `send_file_offers_then_completes_on_receiver_frames` — `SendFile` transmits the offer and opens
  the session; synthetic `FileAccept` → block sent; `BlockAck` → awaiting verify; `FileComplete { VerifiedOk }`
  → `FileSent { receipt_valid: Some(true) }` + session cleared, all through the real
  `apply_command_to_engine` / `process_received_bytes`.
- **Test results (actually run):** full `openpulse-daemon` suite 74 passed / 0 failed; clippy `-D warnings` +
  fmt clean.
- **Status:** both directions are implemented and daemon-level tested (send #this, receive #C-2b), frames are
  proven over the real modem (Phase B `filexfer_loopback`), and the protocol/blocks are unit-tested (Phases
  A/B). The remaining verification is a **two-daemon twin round-trip** integration test (joins the twin-harness
  / on-air validation, Phase F) and optional real-radio PTT burst sequencing.

---

## 2026-07-10 — feat(filexfer): Phase C-2b — the receive session (offer → verify → write) + commands

- **Requirement/change:** FF-16 Phase C-2 (`docs/dev/design/file-transfer-plan.md` §6): the daemon's inbound
  file-transfer session on the proven C-2a seam — accept an offer, receive blocks, verify the signed
  manifest, and write the file; plus the `AcceptFile`/`RejectFile`/`CancelFile` command handlers.
- **Design decision:** `filexfer.rs` grows the full receive handler. `on_offer` verifies the embedded
  manifest signature against the handshake-proven peer key (`RuntimeControlState::verified_peer.pubkey`),
  runs the pure `decide` policy plus the allowlist and per-peer quota, emits `FileOffered`, and either
  transmits `FileAccept` (auto-accept), waits for `AcceptFile` (prompt), or transmits `FileReject`. Block
  fragments feed the session's `BlockAssembler`; a completed block returns a `BlockAck`, and the last block
  drives reassemble → `verify_manifest_with_payload` → **write the file** (`sanitize_filename`,
  `download_dir/<peer>/`, no-overwrite, `.unverified` quarantine on hash mismatch) → `FileComplete`
  (countersigned receipt) → `FileReceived`. The active session lives in `RuntimeControlState.file_rx`
  (taken out while driven, so the rest of the state is freely borrowable); `filexfer_policy` is built from
  `[file_transfer]` config in `server::run`. Everything runs on the production entry
  (`accumulate_capture` → rx tick → `process_received_bytes`); the send-side PTT loop is C-3.
- **Implementation:** `crates/openpulse-daemon/src/filexfer.rs` (`FileTransferPolicy`, `FxRxState`,
  `on_offer`/`on_block_fragment`/`drive_rx_actions`/`reassemble_verify_write`, file I/O, quota, countersign);
  `lib.rs` (`RuntimeControlState.file_rx` + `filexfer_policy`; `AcceptFile`/`RejectFile`/`CancelFile` arms);
  `server.rs` (policy from config); daemon dev-dep `ed25519-dalek`.
- **Tests:** `inbound_offer_and_blocks_write_verified_file` — a signed multi-block offer fed through the real
  `process_received_bytes` auto-accepts, receives every block's fragments, verifies, **writes the file to
  disk** (read back byte-for-byte), emits `FileReceived { verified: true }`, and clears the session.
- **Test results (actually run):** full `openpulse-daemon` suite 73 passed / 0 failed (handshake unchanged);
  clippy `-D warnings` + fmt clean.
- **Next:** C-3 — the live `server::run` `SendFile` PTT burst delivery loop + twin-daemon round-trip test
  (= MVP ship line: both directions over RF).

---

## 2026-07-10 — feat(filexfer): Phase C-2a — inbound OPFX receive-routing seam (handshake preserved)

- **Requirement/change:** FF-16 Phase C-2 (`docs/dev/design/file-transfer-plan.md` §6.2): route inbound
  file-transfer frames off the shared receive path without disturbing the handshake path. Isolated as its own
  step (C-2a) because the refactor touches load-bearing handshake reassembly — the plan's seam-gap discipline.
- **Design decision:** `process_received_bytes` now dispatches its non-QSY/non-relay tail by **SAR
  segment-id** (the 4-byte header is public layout): `0` → `try_reassemble_handshake` (unchanged, so the
  handshake path is bit-for-bit identical and its tests don't move), any other id → `filexfer::route_inbound_fragment`.
  Control frames use segment-id `0xFFFF` (reassembled via a new `filexfer_sar` and decoded), block-data frames
  use `block_index + 1` — three non-overlapping ranges so handshake and file fragments can never share a
  reassembly slot. A `filexfer_frames_routed` tripwire (per the seam-gap checklist) proves file frames reach
  the seam on the production entry (`accumulate_capture` → rx tick → `process_received_bytes`), not just a test
  convenience path. The session handler (offer/data/write/verify) is C-2b; this step lands the proven seam.
- **Implementation:** `crates/openpulse-daemon/src/filexfer.rs` (`route_inbound_fragment`,
  `FX_CONTROL_SEGMENT_ID`); `lib.rs` (`sar_segment_id` helper, seam dispatch, `RuntimeControlState.filexfer_sar`
  + `filexfer_frames_routed`, `FILEXFER_SAR_TIMEOUT`, `mod filexfer`); daemon dep on `openpulse-filexfer`.
- **Tests:** `received_bytes_route_opfx_to_filexfer_and_handshake_stays_untouched` — a handshake fragment
  (seg 0) leaves the tripwire at 0; an OPFX control frame (seg 0xFFFF) and a block fragment (seg k+1) each
  bump it, driven through the real `process_received_bytes`.
- **Test results (actually run):** full `openpulse-daemon` suite 72 passed / 0 failed (all handshake tests
  unchanged); clippy `-D warnings` + fmt clean.
- **Next:** C-2b — the `filexfer.rs` session handler (offer→verify→policy→accept→data→write→verify→
  `FileComplete` + events) + `AcceptFile`/`RejectFile` wiring; C-3 — the live `server::run` send loop + twin test.

---

## 2026-07-10 — feat(filexfer): Phase C-1 — daemon control surface + `[file_transfer]` config

- **Requirement/change:** FF-16 Phase C (`docs/dev/design/file-transfer-plan.md` §6) is staged into steps to
  de-risk the live PTT delivery loop (the plan's own guidance). Step 1: the control-protocol contract and
  config — the API the module + live loop (next steps) will implement against.
- **Design decision:** additive `[file_transfer]` config (`FileTransferConfig`: enabled, download_dir,
  auto_accept/max/quota bytes, require_verified_peer, allowlist, offer timeout — defaults opt-in/off,
  `require_verified_peer = true`, `auto_accept = 0`), and the `SendFile`/`AcceptFile`/`RejectFile`/
  `CancelFile`/`ListFiles` commands + `FileOffered`/`FileProgress`/`FileReceived`/`FileSent`/`FileFailed`/
  `FileList` events + `FileSummary` on `protocol.rs`, mirroring the existing serde-tagged shapes. The
  commands are no-ops on `apply_command_to_engine` (they are intercepted in `server::run` where PTT lives,
  like the `SendMessage` OTA interception — wired in the next step). Panel `apply_event` gets a catch-all for
  the new events (Files tab handling is Phase D).
- **Implementation:** `crates/openpulse-config/src/lib.rs` (`FileTransferConfig` + `OpenpulseConfig` field +
  template section); `crates/openpulse-daemon/src/protocol.rs` (5 commands, 6 events, `FileSummary`);
  `crates/openpulse-daemon/src/lib.rs` (no-op arms); `apps/openpulse-panel/src/connection.rs` (catch-all).
- **Tests:** `filexfer_commands_and_events_round_trip_via_json` (all 5 commands + 6 events serde round-trip);
  the existing config template-parse guard covers the new `[file_transfer]` section.
- **Test results (actually run):** `openpulse-daemon` protocol round-trip passes; `openpulse-config` 15 passed;
  panel/cli/twinview/linksim build clean; clippy `-D warnings` + fmt clean.
- **Next:** Phase C-2 — the `filexfer.rs` daemon module (file I/O, policy, quota) + receive-path
  `try_reassemble_sar` routing; Phase C-3 — the live `server::run` `SendFile` PTT delivery loop + twin test.

---

## 2026-07-10 — feat(filexfer): Phase B — blocks over SAR + multi-object framing + modem loopback

- **Requirement/change:** FF-16 Phase B (`docs/dev/design/file-transfer-plan.md` §12): the byte-level block
  layer under the Phase-A state machines — split a file into blocks, per-block `pack()`, map each to a SAR
  segment, track fragment bitmaps — and prove a real file round-trips through the modem and verifies.
- **Design decision:** the **block is the multi-object unit** that clears the 64 005-byte SAR-segment cap:
  each block is one SAR segment with `segment_id = block_index + 1` (id 0 stays reserved for handshake
  frames). `encode_block` packs a block, wraps it in a `FileData` frame, and SAR-fragments it, with an
  optional missing-bitmap filter for selective retransmission. `BlockAssembler` ingests fragments, peeks the
  4-byte SAR header (no `SarReassembler` API change) to maintain a per-block arrival bitmap for `BlockAck`,
  reassembles + unpacks completed blocks, and concatenates them in order. Integrity is enforced end-to-end
  by `verify_manifest_with_payload` over the reassembled file.
- **Implementation:** `crates/openpulse-filexfer/src/blocks.rs` (`split_blocks`, `encode_block`,
  `BlockAssembler`, `BlockEvent`) + `FxError::BlockTooLarge`; `crates/openpulse-modem/Cargo.toml` dev-deps
  (`openpulse-filexfer`, `ed25519-dalek`).
- **Tests:** `crates/openpulse-filexfer/tests/blocks.rs` (5) — split math; single-block round-trip + verify;
  **>64 005 B multi-object** (100 KB → 7 blocks) reassembled out-of-order + verify; **tampered fragment →
  verify fails**; **missing-bitmap-driven selective retransmit**. `crates/openpulse-modem/tests/filexfer_loopback.rs`
  (2) — a multi-block file's SAR fragments survive `ChannelSimHarness` QPSK500+RS and reassemble + verify;
  a fragment corrupted on the wire fails verification.
- **Test results (actually run):** `openpulse-filexfer` 22 passed (17 Phase-A + 5 blocks); `filexfer_loopback`
  2 passed; clippy `-D warnings` + fmt clean.

---

## 2026-07-10 — feat(filexfer): Phase A — `openpulse-filexfer` crate (OPFX wire + state machines)

- **Requirement/change:** FF-16 Phase A (`docs/dev/design/file-transfer-plan.md` §12): the pure protocol
  crate for direct P2P file transfer — wire codec, offer + policy, sender/receiver state machines — with no
  daemon changes.
- **Design decision:** a no-I/O, no-tokio crate (the `openpulse-b2f`/`QsySession` pattern): sans-I/O
  `SenderSession`/`ReceiverSession` driven by decoded `FxFrame`s + an injected ms clock, emitting `FxAction`s
  the daemon (Phase C) will carry out. New `OPFX` binary frame (7 types, SAR-framed; collision-safe and
  `compression::unpack`-passthrough). Integrity reuses `manifest.rs` — `FileOffer` embeds the four
  `TransferManifest` fields inline; `verify_signature` reconstructs the manifest and calls the existing
  `verify_manifest`. Block/fragment byte-level logic is deferred to Phase B (`blocks.rs`); the receiver
  exposes `note_block_complete`/`set_verify_result` as the Phase A/B seam. Panic-free codec (bounds-checked
  `Reader`, no `unwrap`/`expect` in the library).
- **Implementation:** new `crates/openpulse-filexfer` (`wire.rs`, `offer.rs`, `sender.rs`, `receiver.rs`,
  `sanitize.rs`, `error.rs`, `lib.rs`); workspace member + `[workspace.dependencies]`; `OPFX` registered in
  `docs/dev/design/protocol-wire-spec.md` §8; CLAUDE.md crate-map + acceptance-table rows.
- **Tests:** `crates/openpulse-filexfer/tests/filexfer.rs` — 17 tests: wire round-trips of every frame +
  malformed-frame rejection; offer signature verify + **tamper caught**; policy `decide` (disabled/too-large/
  untrusted/prompt/auto-accept); sender happy path (3 blocks) / reject / offer-timeout / NACK-retransmit-then-
  stall / cancel; receiver auto-accept→verify / verify-failure-unverified / prompt→accept / prompt-timeout /
  reject-decision / cancel; `block_count` math; filename sanitization (traversal/control-chars/empty).
- **Test results (actually run):** `cargo test -p openpulse-filexfer` 17 passed / 0 failed; clippy
  `-D warnings` + fmt clean; crate builds in the workspace.

---

## 2026-07-10 — docs: direct P2P file-transfer design plan (decisions D1–D5 locked)

- **Requirement/change:** capture the approved engineering plan for direct peer-to-peer file transfer — the
  top VarAC-gap item (`docs/dev/research/varac-feature-gap-analysis.md`): offer/accept a file over an RF
  session with progress, size-gated auto-accept, and cryptographic verification VarAC lacks.
- **Design decision:** a Fable-drafted plan grounded in the existing substrate — reuses `sar.rs`
  (chunking), `manifest.rs` (signed SHA-256 integrity), `compression::pack/unpack` (per-block), and the
  OTA/HARQ ARQ path; adds a pure `crates/openpulse-filexfer` crate + daemon `filexfer.rs` glue + an `OPFX`
  wire protocol (7 frames, SAR-encoded through the shared handshake seam). Multi-object framing (≤48 KiB
  blocks, `segment_id = block_index + 1`, id 0 reserved for handshake) clears the 64 005 B SAR object cap.
  Decisions D1–D5 locked to the recommendations: hybrid delivery (OTA per-burst rate + OPFX `BlockAck`);
  1 MiB cap / 16 KiB blocks / auto-accept off; daemon-host path for MVP; block-level resume in Phase E;
  `require_verified_peer = true` + prompt-always.
- **Implementation:** `docs/dev/design/file-transfer-plan.md` (status `approved-plan`);
  `docs/dev/project/roadmap.md` (FF-16 entry); `CLAUDE.md` (key-documents index row).
- **Tests:** none — planning artifact; per-phase acceptance tests specified in the plan's §11/§12 (pure
  state-machine tests, `ChannelSimHarness` loopback file round-trip incl. >64 KB, tampered-chunk verify-fail).
- **Test results (actually run):** N/A (documentation only; no code path changed).

---

## 2026-07-10 — docs: VarAC feature-gap analysis (competitive research)

- **Requirement/change:** identify application/operator-layer features OpenPulse is missing versus VarAC
  (the popular keyboard-to-keyboard ARQ HF chat app), to inform the post-JS8 roadmap.
- **Design decision:** a Fable-authored gap analysis — VarAC feature inventory (cited from its site +
  V4→V14 release history), de-duplicated against verified OpenPulse capabilities, ranked with concrete fit
  sketches against real crates/traits. Top genuine gaps: direct P2P file transfer (≈90% built on
  `sar.rs` + LZ4 + HPX ARQ + `manifest.rs`), native calling-frequency CQ/slots/monitoring (complements the
  FF-15 JS8 plan), VMail-style P2P store-and-forward, live keyboard chat, and alert tags + canned messages.
  Explicit "do not copy" list (VARA/VarAC wire formats, AI/internet gateway, unauthenticated remote
  commands, default content encryption, gamification) to preserve the open/signed/plugin ethos.
- **Implementation:** `docs/dev/research/varac-feature-gap-analysis.md` (the analysis);
  `docs/dev/project/roadmap.md` (candidate-features pointer near FF-15); `CLAUDE.md` (doc-index row).
- **Tests:** none — research artifact; no feature scheduled or implemented.
- **Test results (actually run):** N/A (documentation only).

---

## 2026-07-10 — docs: JS8 discovery & rendezvous design plan (decisions D1–D7 locked)

- **Requirement/change:** capture the approved engineering plan for the JS8-based station discovery and
  rendezvous subsystem (idle auto-QSY to the JS8 calling frequency, in-band `@OPULSE` capability hint,
  discovered-station cache, JS8-negotiated rendezvous + QSY handoff to a native OpenPulse session).
- **Design decision:** a Fable-drafted plan (grounded in code investigation + JS8Call-improved source)
  splits the feature into `plugins/js8` (JS8 waveform `ModulationPlugin`), `crates/openpulse-discovery`
  (pure state machines), and daemon glue; maintainer decisions D1–D7 are locked (native modem with a
  Phase-B external-process fallback; `@OPULSE` group + INFO-token hint keeping the preferred-channel field;
  unauthenticated 2-message rendezvous authenticated by the post-QSY CONREQ/CONACK; off-by-default `rx_only`
  TX with the §97.221 doc gating Phase E; NTP-required, hard TX refusal beyond ±2 s skew; single-band MVP).
- **Implementation:** `docs/dev/design/js8-discovery-rendezvous-plan.md` (the plan; status `approved-plan`);
  `docs/dev/project/roadmap.md` (FF-15 entry); `CLAUDE.md` (key-documents index row).
- **Tests:** none — planning artifact; per-phase acceptance tests are specified in the plan's §10.5 and §11
  (e.g. `reference_vectors`, `js8_loopback`, `discovery_sm`, `rendezvous`) for when implementation starts.
- **Test results (actually run):** N/A (documentation only; no code path changed).

---

## 2026-07-10 — feat(compression): compress the fixed-mode SendMessage path too

- **Requirement/change:** the prior wiring compressed only the OTA session path; a non-OTA (fixed-mode)
  `SendMessage` still transmitted raw. Close that gap so `[compression] enabled` covers both paths.
- **Design decision:** thread the flag through `RuntimeControlState.compress_tx` (already passed to
  `apply_command_to_engine`) rather than change that shared function's signature or duplicate the transmit
  in `server::run`. The fixed-mode `SendMessage` arm packs the body when the flag is set; `server::run`
  sets `compress_tx` from `cfg.compression.enabled` at construction. The `body: String` command is
  unchanged — packing happens at the byte-transmit boundary, so no command-type change is needed. RX is
  already universal (the rx tick unpacks any self-describing frame), so nothing on the receive side changes.
- **Implementation:** `crates/openpulse-daemon/src/lib.rs` (`RuntimeControlState.compress_tx` field +
  Default; pack in the `SendMessage` arm of `apply_command_to_engine`), `crates/openpulse-daemon/src/server.rs`
  (set `compress_tx` in the `RuntimeControlState` construction).
- **Tests:** `apply_send_message_compresses_the_wire_when_enabled` — with `compress_tx = true`, a compressible
  body transmits as a **smaller** frame that `unpack`s back to the original; the existing
  `apply_send_message_transmits_payload_over_active_mode` (flag off) still sends the raw body.
- **Test results (actually run):** full `openpulse-daemon` suite 70 passed / 0 failed; clippy `-D warnings`
  + fmt clean.

---

## 2026-07-10 — feat(compression): end-to-end session compression on the wire (opt-in)

- **Requirement/change:** compression existed as a codec + handshake negotiation but was never applied to
  the data path — the daemon transmitted OTA payloads uncompressed. Enable real on-air compression.
- **Design decision:** a **self-describing** compressed frame (`compression::pack` / `unpack`):
  `PACK_MAGIC "OPZ1"(4) | algo_tag(1) | payload`, where `pack` uses `compress_if_smaller` (best of LZ4/zstd,
  or `None` tag when incompressible) and `unpack` returns `Some(original)` only for a well-formed packed
  frame and `None` for anything else. The magic makes it **passthrough-safe**: control frames (relay `OPHF`,
  QSY, handshake `HSCQ`/`HSAK`) and un-packed data lack the magic and are never touched, so the feature can
  be enabled on one station independently and never corrupts the other end's frames. Applied at the **OTA
  session data seam** (the real receiver-led bulk-transfer path) — TX packs the body when `[compression]
  enabled`, and the RX tick **always** unpacks (self-describing), so a compressing peer is understood
  regardless of the local flag. `unpack` reuses `decompress`'s `MAX_DECOMPRESSED_SIZE` guard (OOM-safe).
- **Implementation:** `crates/openpulse-core/src/compression.rs` (`PACK_MAGIC`, `pack`, `unpack`);
  `crates/openpulse-config/src/lib.rs` (`CompressionConfig { enabled }` default false + template section);
  `crates/openpulse-daemon/src/server.rs` (pack the OTA body on TX when enabled; unpack the decoded RX
  bytes at the rx-tick seam before routing/metrics).
- **Tests:** core `pack_unpack_roundtrips_compressible_data` (packed < raw, non-None tag, restores),
  `..._incompressible_data` (None tag), `unpack_passes_through_non_packed_frames` (OPHF/HSCQ/QSY/plain/empty
  → None), `unpack_rejects_unknown_tag_and_corrupt_payload`; modem `compression_wire.rs` — a packed payload
  survives modem framing + RS FEC over a clean channel and unpacks to the original, and a non-packed payload
  passes through the rx seam untouched.
- **Test results (actually run):** core 436 passed / 0 failed; `compression_wire` 2 passed; config 15 passed;
  daemon 69 passed / 0 failed; ardop/kiss/cli consumers build clean; clippy `-D warnings` + fmt clean.
- **Note (scope):** wired for the OTA session path; fixed-mode `SendMessage` transmit still sends raw (its
  `body: String` command seam can't carry the binary packed frame without a command-type change) — a noted
  follow-up. The RX `unpack` is universal, so it already accepts packed frames from any sender.

---

## 2026-07-10 — fix(panel): Event-log tab fills the full tab width

- **Requirement/change:** the Event-log tab's column and text used the default `Shrink` width, so lines
  occupied only their own length and long events were clipped, leaving the rest of the tab blank.
- **Design decision:** set `Length::Fill` on the log column, each line's `Text`, and the `scrollable`, so
  the log spans the full usable tab width and long lines wrap at the panel edge.
- **Implementation:** `apps/openpulse-panel/src/ui.rs` (`log_widget`).
- **Tests:** none new — layout-only; existing `openpulse-panel` suite re-run as a regression guard.
- **Test results (actually run):** `openpulse-panel` 15 passed; clippy `-D warnings` + fmt clean.

---

## 2026-07-10 — feat(daemon): wire compress_ratio live from the RX data path

- **Requirement/change:** `ControlEvent::Metrics.compress_ratio` was hardcoded `None`, so the panel always
  showed the `1.0` default. Report a real, live compression figure from the data path.
- **Design decision:** the daemon transmits payloads uncompressed (no wire compression, no `[compression]`
  config), so the honest live figure is the **compressibility of the decoded RX payload stream** measured
  with the actual session compressor (`compress_if_smaller` — LZ4/zstd, best-of, never larger than raw).
  Accumulate cumulative raw and best-effort-compressed byte totals at the **single RX-decode seam already
  wired to metrics** (`server.rs`, where `total_rx_bytes` is updated), and emit `compressed / raw` in the 1 Hz
  metrics loop. Cumulative (not windowed) so the ratio is a stable property of the traffic. This is
  observability only — it does not change what is transmitted, so there is no interop/RX-decode impact.
- **Implementation:** `crates/openpulse-daemon/src/lib.rs` (`MetricsSnapshot.raw_payload_bytes` /
  `compressed_payload_bytes`; pure `compression_ratio(raw, compressed) -> Option<f32>`; metrics loop emits it),
  `crates/openpulse-daemon/src/server.rs` (measure + accumulate at the RX seam, non-empty frames only),
  `crates/openpulse-daemon/src/protocol.rs` (doc comment). The panel already renders `Some(ratio)` via the
  earlier `format_compression` "N:1" display — no panel change.
- **Tests:** `compression_ratio_is_none_before_any_payload`, `compression_ratio_reports_compressed_over_raw`
  (1000/200 → 0.20), `compression_ratio_tracks_the_real_compressor_on_compressible_data` (repeated-byte
  payload compresses to < 0.5 through the real `compress_if_smaller`).
- **Test results (actually run):** 3 new tests pass; full `openpulse-daemon` suite 69 passed / 0 failed across
  7 binaries; clippy `-D warnings` + fmt clean.

---

## 2026-07-10 — feat(panel): Statistics tab — successful frames per ladder step, per session

- **Requirement/change:** the operator had no per-session view of how many frames actually got through at
  each ladder step. Also traced the sibling question: `compress_ratio` is populated **nowhere** — the daemon
  emits `ControlEvent::Metrics` at a single site (`crates/openpulse-daemon/src/lib.rs`) with `compress_ratio:
  None` (and `ecc_rate: None`) hardcoded, and the send path compresses with `CompressionAlgorithm::None`, so
  there is no ratio to report; the panel's `None`→`1.0` default now renders honestly as "1.0:1".
- **Design decision:** count frames in the panel from the existing `EngineEvent` stream (no daemon change):
  `FrameReceived`/`FrameTransmitted` bucketed by the ladder step (`speed_level_num`, set by `RateChange`)
  current at that moment; cleared on `SessionStarted`. Separate RX (decoded) and TX counters, since either
  direction is a "successful transfer". Counting logic lives in `PanelState` (pure, unit-tested); the tab
  renders a per-step RX/TX/Total table with a totals row. Bucket 0 is the pre-first-`RateChange` step.
- **Implementation:** `apps/openpulse-panel/src/app.rs` (`Tab::Stats`); `state.rs` (`rx_frames_by_level` /
  `tx_frames_by_level` `[u32; LEVEL_BUCKETS]`, `record_frame`, `reset_frame_stats`); `connection.rs`
  (increment on frame events, reset on `SessionStarted`); `ui.rs` (`stats_widget`, tab button + dispatch).
- **Tests:** `state::frame_stats_tests` — `record_frame_buckets_by_current_level`,
  `level_zero_is_the_pre_lock_bucket`, `out_of_range_level_clamps_into_the_last_bucket` (no OOB panic on an
  absurd level), `reset_zeroes_all_buckets`.
- **Test results (actually run):** `openpulse-panel` 15 passed (4 new + 11 existing); clippy `-D warnings` +
  fmt clean; panel builds clean.

---

## 2026-07-10 — fix(panel): show compression as an "N:1" reduction factor, not a bare fraction

- **Requirement/change:** the panel's Additional-info tab rendered the compression metric as
  `format!("{:.2}×", compress_ratio)`. The daemon reports `compress_ratio` as compressed/raw, so a good
  5:1 compression showed as **"0.20×"** — which reads as *expansion* and mismatched the ~5× gross-vs-effective
  factor the operator sees elsewhere.
- **Design decision:** display the reciprocal as an "N:1" ratio (`5.0:1`), the conventional way to state a
  compression factor. Ratios ≥ 1 (no gain, or the not-yet-wired 1.0 default) read "1.0:1"; a non-positive/NaN
  value shows "—". Extracted a pure `format_compression(ratio)` helper so the mapping is unit-tested.
- **Implementation:** `apps/openpulse-panel/src/ui.rs` (`format_compression` helper; the "Compress" info-row
  uses it instead of the inline `{:.2}×` format).
- **Tests:** new `ui::tests` — `compression_shows_reduction_factor` (0.20→5.0:1, 0.50→2.0:1, 0.25→4.0:1),
  `compression_no_gain_reads_one_to_one` (1.0 and 1.2 → 1.0:1), `compression_guards_bad_values` (0/negative/NaN → "—").
- **Test results (actually run):** `openpulse-panel` 11 passed (3 new + 8 existing); clippy `-D warnings` + fmt clean.

---

## 2026-07-10 — docs: record the completed Fable-audit backlog as roadmap Phase 11

- **Requirement/change:** the roadmap had no section for the Fable full-chain audit backlog (#697–#717),
  so the largest recent body of shipped work was absent from the phase history.
- **Design decision:** add a `Phase 11 — Signal-chain audit hardening` section (matching the Phase 10
  format) grouping the shipped PRs into Tier-1 bugs / OTA-ladder re-seat / measurement fidelity /
  improvement backlog, and recording the two intentionally-skipped items (QPSK β-port, `freq_acquire`) so
  they aren't re-opened. Bumped the roadmap `last_updated` to 2026-07-10.
- **Implementation:** `docs/dev/project/roadmap.md` (Phase 11 section + front-matter date).
- **Tests:** none — documentation-only.
- **Test results (actually run):** N/A (roadmap prose; the per-change gates are logged in the individual
  #697–#717 entries below and were verified green in the full `--workspace --no-default-features` run:
  1656 passed, 0 failed, 22 ignored across 207 binaries).

---

## 2026-07-10 — docs: log the acceptance-table refresh in the ledger (PR #719)

- **Requirement/change:** PR #718 refreshed the `CLAUDE.md` acceptance-criteria table but its own
  requirement→results chain was not yet recorded in this ledger.
- **Design decision:** every session PR carries a ledger entry; a ledger-maintenance PR is no exception, so
  #719 gets this self-describing entry (it closes the 1:1 PR↔ledger coverage rather than leaving #718's entry
  as the only change without its own row).
- **Implementation:** `docs/dev/project/traceability.md` (the #718 entry immediately below; and this entry).
- **Tests:** none — documentation-only.
- **Test results (actually run):** N/A (no code path changed; ledger prose only).

---

## 2026-07-10 — docs: refresh the acceptance-criteria table with this session's gates (PR #718)

- **Requirement/change:** the `CLAUDE.md` → *Acceptance criteria* table (the requirement↔test ledger the
  traceability rule requires kept current) had two placeholder rows ("add in `…​.rs`") and no rows for the
  gates shipped across the Fable-audit backlog (#703/#705/#710, #714–#717), so the table no longer reflected
  what actually guards each requirement.
- **Design decision:** every requirement row must name a real, currently-passing test — no placeholders, no
  "covered" asserted from a grep. Made the Gilbert-Elliott and Watterson-envelope rows concrete and added one
  row per shipped gate.
- **Implementation:** `CLAUDE.md` (Acceptance criteria table): concrete G-E burst-span and Watterson-envelope
  rows; new rows for Watterson continuous fade, SC-FDMA multipath delay reach, symbol-domain SNR, QPSK1000-HF-RRC
  forward-only, and the CI goodput regression gate.
- **Tests:** each linked command was executed to confirm it resolves to a passing test (no dead links).
- **Test results (actually run):** `bursts_span_whole_symbols_with_mean_one_over_pbg` 1 passed;
  `continuous_fade_correlates_across_calls` 1 passed; `openpulse-linksim goodput_gate` 3 passed;
  `scfdma_multipath_timing` / `qpsk_hf_rrc_forward_only` / `symbol_domain_snr` verified as existing gate files
  passing earlier this session.

---

## 2026-07-10 — fix(scfdma): widen the sync back-off so the delay-cliff clears the CCIR-poor spread

- **Requirement/change:** the wideband SC-FDMA rungs (`hpx_wideband_hd` SL12–14: SCFDMA52-16/32/64QAM)
  hard-cliff on multipath. A noiseless static two-ray sweep (`a0·x[n] + a1·x[n−d]`, delayed ray stronger)
  showed all three decode 1.00 through delay `d = 8` then collapse to **0.00 at `d ≥ 10`** — inside the
  32-sample CP, so the CP is not the limiter.
- **Design decision:** the cliff is the sync, not the CE. `find_sync_offset` backs the FFT window off
  `SYNC_EARLY_BIAS = 8` samples ahead of the matched-filter peak; a stronger delayed ray puts the argmax
  `d` past the true onset, so for `d >` bias the window starts late and pulls the next symbol in (a hard
  0.00). Raising the bias to **16** places the window early enough to see a ±16-sample (2 ms) spread, still
  a pure circular shift inside the CP that `deramp_timing` removes. **The CE basis did NOT need widening**:
  an attempted widen (step 5/3→8/3, reach 10→16) over-fit pilot noise on flat channels and broke the
  `llr_reliability` gate (|L|≈11 bits wrong 17× more than promised) — reverted. `deramp_timing` re-centres
  the impulse response on its power centroid, so the original ±10 basis already covers the *re-centred*
  relative spread of a 16-sample two-ray channel. One constant, no calibration cost.
- **Implementation:** `plugins/scfdma/src/demodulate.rs` (`SYNC_EARLY_BIAS` 8 → 16).
- **Tests:** `scfdma_multipath_timing::decodes_a_stronger_delayed_ray_inside_the_cyclic_prefix` gains a
  `SCFDMA52` (QPSK) stronger-delayed ray at `d = 12` (0.00 at bias 8 → ≥ 0.90 now; the denser modes lose
  margin to the −6 dB two-ray null itself at d = 12, not to the sync, so they stay at the shorter cases).
- **Test results (actually run):** delay sweep now decodes 1.00 through `d = 14–16` (was `d = 8`);
  `scfdma_multipath_timing` 3 passed (incl. new d=12); `llr_reliability` passes (flat @10 dB calibration
  intact); scfdma-plugin all binaries (58 lib + integration) pass; Watterson gates `channel_loopback_multimode`
  (11), `waveform_lock_watterson` (9), `ofdm_scfdma_bakeoff` (1) pass; full `openpulse-modem` suite pass;
  clippy `-D warnings` + fmt clean.

---

## 2026-07-10 — feat(channel): opt-in continuous Watterson fade (correlated across apply() calls)

- **Requirement/change:** `WattersonChannel::apply()` synthesises a self-contained FFT fade realization
  *per call*, so a streaming caller that feeds the channel one frame per `apply()` (linksim `run_link`,
  the twin/daemon path) gets an **independent** fade every frame. At low Doppler (F1, ~10 s coherence)
  consecutive frames should be strongly correlated; instead they fully decorrelate — an unphysical
  per-frame re-randomisation that makes a link sim's fade dynamics wrong.
- **Design decision:** add a persistent, phase-continuous **sum-of-sinusoids** fader (`SosFader`,
  M=48 oscillators, Doppler shifts `f_m ~ N(0, σ_d)` → Gaussian PSD, phases `φ_m ~ U(0,2π)` drawn once;
  E[|h|²]=1) that carries oscillator phase across calls. Gate it behind a new `WattersonConfig.continuous`
  flag (default **false**) so the one-shot FFT path — correct within a single call and the basis of every
  existing threshold test — stays **bit-identical**; only streaming callers opt in. FFT-per-call cannot be
  made streamable at F1 (bin_width ≤ doppler/2 needs a ~2^18 FFT), which is why a second generator is the
  right tool rather than a rewrite.
- **Implementation:** `crates/openpulse-channel/src/fading.rs` (`SosFader`),
  `crates/openpulse-channel/src/lib.rs` (`continuous` field on `WattersonConfig` + all 8 presets +
  `.continuous()` builder), `crates/openpulse-channel/src/watterson.rs` (`ray_envelopes` dispatch,
  faders built at `new()` when continuous), `apps/openpulse-linksim/src/lib.rs` (all 3 Watterson specs
  opt in). No external raw-literal `WattersonConfig` construction exists, so the added field breaks nothing.
- **Tests:** new `continuous_fade_correlates_across_calls` (frame-by-frame at F1: continuous lag-1 RMS
  autocorr > 0.5 and ≥ 0.3 above the per-call-re-randomising default) and `continuous_mode_preserves_unit_power`
  (seed-averaged E[|h|²] ∈ [0.75, 1.35]).
- **Test results (actually run):** `openpulse-channel` 48 passed (was 46; the pre-existing FFT-path tests
  unchanged, confirming default is bit-identical); `openpulse-linksim` 17 + 3 goodput gates still pass with
  continuous fades on; `openpulse-modem` `channel_loopback` 12 passed (default path untouched);
  testmatrix/testbench build clean; clippy `-D warnings` + fmt clean.

---

## 2026-07-10 — perf(dsp): QPSK1000-HF-RRC forward-only LMS (drop the fading-harmful DFE)

- **Requirement/change:** the QPSK1000-HF-RRC demod ran a decision-feedback equalizer (`fwd=11, dfe=2`).
  The #697 DFE-sign note flagged that decision feedback propagates errors on a fading channel; this closes
  the loop with a coded measurement and removes the DFE where it hurts.
- **Design decision:** a coded Watterson sweep (SoftConcatenated FEC, the only way this dense HF mode ever
  runs) compared `(11,2)` against forward-only `(11,0)`: forward-only **wins** on `good_f1 @20 dB`
  (0.68 vs 0.60 frame success) and **ties** on AWGN@12 (1.00) and static two-ray ISI@16 (1.00). The forward
  filter + soft FEC already cover the ISI the DFE was there for; the feedback section only adds error
  propagation on fades. So QPSK1000-HF-RRC becomes forward-only; the shorter/cleaner profiles are unchanged.
- **Implementation:** `plugins/qpsk/src/demodulate.rs` (`lms_profile` returns `(11, 0, 0.010)` for
  `QPSK1000-HF-RRC`; unit test `lms_profile_hf_is_forward_only` updated to assert `dfe==0`).
- **Tests:** new gate `crates/openpulse-modem/tests/qpsk_hf_rrc_forward_only.rs` pins the forward-only
  fading floor — 40 `good_f1 @20 dB` coded frames must decode ≥ 0.55 (forward-only measured 0.68; a re-added
  DFE, which measured 0.60, would trip it). The vestigial self-comparison test in `plugins/qpsk/src/lib.rs`
  was renamed to `qpsk1000_hf_decodes_some_bits_on_moderate_f1` and its stale `fwd=9,dfe=2` comment removed.
- **Test results (actually run):** new gate passes (rate 0.68); `qpsk-plugin` suite passes (38 + updated
  unit test); full `openpulse-modem` suite passes; clippy `-D warnings` + fmt clean.

---

## 2026-07-10 — fix(channel): Gilbert-Elliott steps per-symbol (real bursts, not sub-symbol AWGN)

- **Requirement/change:** the Gilbert-Elliott channel ran its two-state Markov chain **per sample**, so a Bad
  run averaged `1/p_bg` *samples* — sub-symbol at the tested baud rates — and looked like elevated-variance
  AWGN rather than a burst. Its own preset docs already *claimed* "mean burst = 1/p_bg symbols", and any
  interleaver/burst-FEC conclusion drawn from it was vacuous.
- **Design decision:** add `symbol_samples` to `GilbertElliottConfig` and step the chain **once per symbol**
  (a boundary-gated `step_state(i)`), holding the state through the symbol, so a Bad run is a contiguous run
  of whole *symbols*. The caller sets it to the mode's samples-per-symbol; presets default to 8. The code now
  matches the documented intent.
- **Implementation:** `crates/openpulse-channel/src/lib.rs` (config field + all four presets),
  `crates/openpulse-channel/src/gilbert_elliott.rs` (`step_state`, `apply`, `generate_noise`); the one
  external construction (`channel_loopback.rs`) updated.
- **Tests:** replaced the standalone Markov re-implementation test with one that drives the *actual* channel:
  recovers the per-symbol Bad/Good state from the output noise energy (Bad is ~20 dB louder on `moderate`)
  and asserts runs average > 3 symbols (a per-sample chain flickers near 1) and within 20 % of 1/p_bg.
- **Test results (actually run):** the new burst-span test passes (was structurally impossible before);
  `channel_loopback` G-E tests (`moderate_burst_no_fec_degrades`, `light_burst_with_fec`) still pass with the
  now-longer bursts; channel/modem/testmatrix/linksim suites pass; clippy `-D warnings` + fmt clean.

## 2026-07-10 — fix(pilot): calibrate the soft LLRs and acquire on the normalised correlation

- **Requirement/change:** the pilot plugin (a) emitted uncalibrated soft LLRs — `symbols_to_llrs` divided by
  a fixed `noise_var = 1.0`, so `mean|LLR| ≈ 2.0` flat in SNR and HARQ combining could not weight a faded
  attempt down (it was also absent from the `llr_calibration` gate); and (b) located the frame onset with
  `IqMatchedFilter::search` (unnormalised score, argmax-favours-energy) — the #689 SC-FDMA acquisition bug
  that never propagated here.
- **Design decision:** (a) divide `symbol_llrs` by the decision-directed 2-D noise variance (mean squared
  distance to the nearest point, measured against the *same* `points`, so it is correct for 32APSK as well),
  matching the OFDM/SC-FDMA calibration; (b) switch `find_onset` to `search_normalized(.., 0.01)` with a 1 %
  energy floor so ρ stays meaningful.
- **Implementation:** `plugins/pilot/src/frame.rs` (`symbols_to_llrs`), `plugins/pilot/src/demodulate.rs`
  (`find_onset`).
- **Tests:** added `PILOT-16QAM500` to `crates/openpulse-modem/tests/llr_calibration.rs` (min ×2.0 from
  8→20 dB).
- **Test results (actually run):** the calibration gate now includes pilot and passes (was flat ~×1.0
  before); pilot plugin suite pass (round-trip + carrier-offset acquisition unchanged with the normalised
  search); full modem suite pass; clippy `-D warnings` + fmt clean.

## 2026-07-10 — perf(dsp): RRC filter span 8→12 on the dense-constellation RRC rungs

- **Requirement/change:** the RRC pulse-shaping filter spanned 8 symbols, leaving a residual-ISI floor
  ~−36 dB that caps EVM on the dense RRC modes (their tight constellations are ISI-floor-limited, not
  noise-limited, at high SNR). Widening to 12 symbols drops the floor to ~−50 dB.
- **Design decision:** bump `RRC_SPAN_SYMBOLS` 8→12 in the dense-mode plugins (qpsk, psk8, 64qam, pilot).
  BPSK stays at 8: its RRC modes are low-order (the −36 dB floor is already far below the ±90° margin) and
  low-baud → high `sps`, so a wider span there is expensive filtering for no benefit. The constant is used
  symmetrically by mod and demod (the demod derives its group delay from `num_taps`), so both ends stay
  matched; both stations must run the same build (a wire/pulse-shape change, not a ladder-fingerprint one).
- **Implementation:** `plugins/{qpsk,psk8,64qam,pilot}/src/modulate.rs` — `RRC_SPAN_SYMBOLS = 12`.
- **Test results (actually run):** `rrc_channel_loopback` 5, qpsk/psk8/qam64/pilot plugin suites — pass
  (both-ends round-trip unchanged); full modem suite pass; clippy `-D warnings` + fmt clean.

## 2026-07-10 — perf(ldpc): PEG graph for the rate-1/2 codec (drop the random xorshift H_s)

- **Requirement/change:** `LdpcCodec::new` (rate-1/2) built its info-part Tanner graph from a random
  xorshift32 draw, which left short cycles that trap the min-sum decoder. The already-shipped PEG builder
  (`with_peg`, used by `high_rate`) maximises girth — a measured ~0.2–0.3 dB AWGN gain at zero cost.
- **Design decision:** `new()` now delegates to `Self::with_peg(1024, 1024, 3)`. Same systematic
  `[H_s | I_m]` structure, so encoding stays a single XOR pass and the decoder is unchanged; TX and RX both
  call `new()`, so the graph swap is symmetric. Removed the now-unused `xorshift32`.
- **Implementation:** `crates/openpulse-core/src/ldpc.rs`.
- **Test results (actually run):** LDPC lib tests 12, `fec_comparison` 6, `ldpc_ladder_rungs` 2 — pass
  (round-trip + decode unchanged in structure, better graph); clippy `-D warnings` + fmt clean.

## 2026-07-10 — test(linksim): real-modem goodput regression gate

- **Requirement/change:** the CI "benchmark regression gate" replays HPX state-machine *events* with no
  modem in the loop, so a DSP change that halves decode throughput sails through green. Add a cheap goodput
  gate that actually runs the modem.
- **Design decision:** three `#[test]`s in `openpulse-linksim` (which CI already runs via
  `cargo test --workspace`), each `run_link`-ing the full ARQ stack (modulate → channel → demodulate → FEC
  → receiver-led rate control) and asserting effective two-way bps stays above a floor set to ≈65 % of the
  measured baseline — enough to catch a halving, loose enough to tolerate normal variation. Deterministic
  (seeded channels), ~4 s total. Covers three DSP surfaces: single-carrier PSK climb (hpx_hf AWGN),
  the OFDM ladder (hpx_ofdm_hf AWGN), and the dispersive-HF path (hpx_ofdm_hf moderate_f1).
- **Implementation:** `apps/openpulse-linksim/src/lib.rs` — `mod goodput_gate`.
- **Test results (actually run):** measured baselines hpx_hf/AWGN-20 = 397 bps, hpx_ofdm_hf/AWGN-20 = 919,
  hpx_ofdm_hf/moderate_f1-25 = 414; floors 250/600/280. All three pass; clippy `-D warnings` + fmt clean.

## 2026-07-10 — feat(fec): byte interleaver on the SoftConcatenated wire (burst-fade tolerance)

- **Requirement/change:** `SoftConcatenated` (outer RS + inner K=7 conv) carried no interleaver anywhere, so
  a deep-fade burst that overwhelms the Viterbi produced a clustered run of byte errors that overran a single
  RS block and failed the frame. The measured win: burst-fade FER 0.98 → 0.20 @4 dB, zero AWGN cost.
- **Design decision:** insert a block byte-interleaver between the outer RS and inner conv (TX:
  RS → interleave → conv; RX: Viterbi → deinterleave → RS), reusing the existing `Interleaver`. It spreads
  the Viterbi's byte-error run across *both* RS blocks of a multi-block frame so each stays under RS's t=16.
  Centralised into two free functions (`soft_concat_encode` / `soft_concat_decode_llrs`) that *all four*
  SoftConcatenated sites (transmit, the timeout receive, the OTA candidate path, and `decode_combined_llrs`)
  now funnel through — the interleaver can never be applied on one end only. Applied **only to
  multi-block frames** (`rs_bytes.len() > 255`): a single RS block gains nothing and the reshuffle
  measurably tipped a marginal single-block 64QAM-SRO threshold case; the length-preserving permutation
  lets the RX mirror the same gate from the Viterbi-decoded length.
- **Implementation:** `crates/openpulse-modem/src/engine.rs` — new helpers + the four call sites refactored
  onto them.
- **Tests:** `crates/openpulse-modem/tests/soft_concat_interleaver.rs` — a 240-byte (two-RS-block) frame
  through a contiguous *phase-inverted* burst (−1.0 × a 5 % span: a real fade rotates the carrier →
  confident-wrong symbols the soft-Viterbi trusts and propagates, unlike attenuation which just lowers LLR
  confidence and is recovered) plus a clean-channel control.
- **Test results (actually run):** with the interleaver the burst frame decodes ≥ 0.80; **ablated (interleaver
  neutralised) it decodes 0.00** — the gate genuinely measures the interleaver. Clean-channel control 10/10
  (zero cost). SoftConcatenated round-trips unchanged (channel_loopback_multimode 11, fec_loopback 32,
  harq_fade_diversity 2); full modem suite pass; clippy `-D warnings` + fmt clean. (Note: the RS↔conv
  placement helps *multi-block* frames — a single-block frame sees no benefit since RS corrects 16 errors
  wherever they sit; the payload is sized to two blocks deliberately.)

## 2026-07-10 — fix(profile+linksim): all-OFDM ladder climbs on dispersive HF; linksim uses the daemon SNR

- **Requirement/change:** two coupled gaps found while checking whether linksim/panel support the OFDM re-seat.
  (1) The receiver-led ladder cannot bootstrap into the OFDM rungs on a Doppler/delay-spread fade: `hpx_hf`
  starts at BPSK31 and the single-carrier PSK rungs (SL2–9) cannot decode moderate_f1 (measured @30 dB:
  BPSK31 0/10, QPSK250 1/10, 8PSK500 1/10; OFDM52-16QAM 10/10 — 1 Hz Doppler spins their long-frame carrier
  phase). (2) Linksim drove its ladder from `estimate_additive_snr_db(tx,rx)`, which reads ≈ −8 dB for a 25 dB
  OFDM signal through moderate_f1 (it counts delay spread as noise), so it did not mirror the daemon and could
  not exercise the OFDM ladder on fading. Panel supports it already (OFDM modes in its list).
- **Design decision:** (1) the all-OFDM `hpx_ofdm_hf` profile (OFDM16 entry, per-symbol pilot CE) is the right
  ladder for dispersive HF, but its entry rungs were unprotected (failed ~50 % on fading — one faded subcarrier
  corrupts a byte) and its floors were AWGN-scale (never cleared). Protect every rung with SoftConcatenated
  (its soft LLRs take OFDM16/OFDM52 to ≥0.9; it does not hit the padded-RS-block geometry plain RS did) and
  recalibrate floors/ceilings into the *plugin-SNR* units the ladder reads — conservative and saturating ~17 dB
  on moderate_f1 (measured floors 8/9/10/12/14/16). (2) Linksim adopts `ModemEngine::rx_snr_db` (made `pub`) —
  the daemon's own symbol-domain estimator — removing the `estimate_additive_snr_db` redundancy so the
  simulator mirrors the real software. Global default left at `hpx_hf` (the OFDM16-floor-8 vs BPSK31-floor-3
  weak-signal tradeoff is a product call flagged for the user), but `hpx_ofdm_hf` is now the working
  dispersive-HF ladder.
- **Implementation:** `crates/openpulse-core/src/profile.rs` (`hpx_ofdm_hf`: SoftConcatenated on SL5–SL10,
  floors 8/9/10/12/14/16, ceilings +2); `crates/openpulse-modem/src/engine.rs` (`pub fn rx_snr_db`);
  `apps/openpulse-linksim/src/lib.rs` (drive the ladder from `self.fwd.rx_engine.rx_snr_db`, drop the
  additive helper). Tests: `session_profile::hpx_ofdm_hf_snr_thresholds` updated; linksim
  `ofdm_hf_profile_climbs_on_a_dispersive_fade` (new); the notch test's tone moved to the band edge
  (2650 Hz) — a far tone is rejected by the demod, so a band-aware SNR correctly sees little notch benefit
  from it (the additive estimator over-penalised out-of-band energy).
- **Test results (actually run):** linksim `hpx_ofdm_hf` climbs to ≥ SL8 on a 30 dB moderate_f1 fade (was
  stuck at SL6 with RS + AWGN floors; hpx_hf is stuck at SL2). Notch tests pass with the band-edge tone. Core,
  cli, linksim suites + full modem suite pass; clippy `-D warnings` + fmt clean. NOTE: fully closing the gap
  *at the default* means switching the daemon default to `hpx_ofdm_hf` — flagged as a product decision.

## 2026-07-10 — refactor(profile): re-index the hpx_hf dense ladder (drop the P4-duplicate rungs)

- **Requirement/change:** after the OFDM re-seat, the former SC-FDMA P4 dense-pilot rungs (SL14/SL18) had
  folded onto plain `OFDM52-64QAM`, duplicating SL15/SL19 (a no-op step). Flagged as a follow-up cleanup.
- **Design decision (measurement-checked first):** verified the floors are **not** overly conservative for
  OFDM — the SC-FDMA-derived numbers land at a consistent ≈+8 dB margin over OFDM's measured AWGN floors
  (8PSK 6 / 16QAM 8 / 32QAM 10 / 64QAM 14; LHR 12/16/20), a sensible HF fading margin — so no floor
  "tightening" was warranted (it would only cut robustness). Removed the two duplicate rungs and re-indexed
  the dense segment to **SL10–SL17** (8 rungs). Chose the *safe* variant: keep the higher-floor (floor-22/30)
  64QAM rungs and drop the low-floor (19/28) ones — the low-floor entries were optimistic on fading
  (OFDM52-64QAM ≈0.52 at 19 dB on moderate_f1). Tradeoff recorded: a small AWGN throughput loss in 17–22 dB
  (no 64QAM entry between SL13 and SL14), which on real HF fading was never a working rung anyway.
- **Implementation:** `crates/openpulse-core/src/profile.rs` — `hpx_hf` modes/fec/floors/ceilings rewritten
  for SL10–SL17; `ack_up_requires_snr_candidate_at` SL19→SL17; comment table + floor rationale updated. Tests:
  `session_profile.rs` (mappings, top rung SL17), `cli_mode_advisor.rs` + `commands/mode_advisor.rs`
  (SNR→level cases), `ldpc_ladder_rungs.rs` (`MEASURED_AWGN_FLOOR_DB` → SL15–17; rung count 10→8; densest-SC
  comparison SL15→SL14). No enum change (SpeedLevel variants untouched; other profiles unaffected);
  `fingerprint()` already excludes floors, so a floor tweak never desyncs peers.
- **Test results (actually run):** `session_profile` 30 (floors monotonic + ceiling = floor(L+1)+2 verified
  across the new ladder; top = SL17, no ceiling), `cli_mode_advisor` 3, `ldpc_ladder_rungs` 2, full modem
  suite + core — pass; ladder-climb over a 35 dB link now tops out at **SL17** (the new ladder top); clippy
  `-D warnings` + fmt clean.

## 2026-07-10 — test(scfdma): ablate the moderate_f1 plateau — it's Doppler, not outage

- **Requirement/change:** the OFDM-vs-SC-FDMA bake-off recorded SC-FDMA's flat ~0.35 `moderate_f1` decode as a
  "structural delay-cliff / deep-fade outage." Fable flagged that a flat-across-SNR number is this repo's bug
  signature and merits the ablation *before* it's written up as pure physics.
- **Design decision:** apply the repo's "delete the mechanism" rule — remove the noise (60 dB) and freeze the
  channel (0 Hz Doppler), and compare SC-FDMA against OFDM on the same `moderate_f1` fade draws (identical
  CP/pilot geometry). If OFDM decodes noiselessly where SC-FDMA does not, the gap is the SC-FDE receiver, not an
  erased-subcarrier information limit that would sink both.
- **Implementation:** `crates/openpulse-modem/tests/scfdma_plateau_ablation.rs` (ignored; diagnostic).
- **Test results (actually run, 60 draws, noiseless):** SCFDMA52-16QAM decodes **0.90 frozen** but **0.50 at
  1 Hz Doppler**; OFDM52-16QAM **1.00** dynamic. SCFDMA52-8PSK 0.93 / 0.68; OFDM52-8PSK 0.95 / 1.00. **Verdict:**
  the plateau is **intra-frame Doppler that SC-FDMA's per-frame Wiener CE + EMA smoothing cannot track**, not
  outage — a recoverable SC-FDE *receiver* limit, and a mechanistic reason the OFDM re-seat wins (OFDM
  re-estimates from pilots every symbol). Distinct from moderate_f2's 0.03 (the ±10-sample CE-reach
  delay-cliff). SC-FDMA is retired from the ladder, so this is **recorded, not fixed**; it corrects the
  "delay-cliff/outage" framing for moderate_f1 in the re-seat entry below.

## 2026-07-09 — feat(rate): multicarrier symbol-domain RX SNR (OFDM + SC-FDMA) — the ladder climbs to SL19

- **Requirement/change:** PR-A gave the PSK rungs a calibrated SNR (escaping the M2M4 SL8 cap → ladder
  reached SL10), and the re-seat put OFDM on SL11–SL19. But the OTA ladder still **stalled at SL10**: M2M4
  reads garbage on a multicarrier envelope (−10 dB on OFDM, −6 on SC-FDMA even at high true SNR), so neither
  the kept narrowband SC-FDMA rung (SL10) nor the OFDM rungs could self-measure SNR to justify climbing.
- **Design decision:** add a non-constant-modulus estimator `qam_symbol_snr_db(symbols, bits_per_sc) =
  10·log10(P_const/σ²)` (mean constellation power over the decision-directed noise var,
  `estimate_decision_noise_var`) — a ratio on the same scale, so uniform-gain-invariant and, being
  decision-directed, saturating on errors (safe). Both multicarrier plugins implement `estimate_snr_db` by
  running their existing front-end to the *equalized* data symbols (OFDM: ZF; SC-FDMA: MMSE + IDFT with the
  `alpha_avg` attenuation undone) and calling it. SC-FDMA is included specifically because keeping one SC-FDMA
  rung (SL10) in the middle of the ladder otherwise walls off the climb.
- **Implementation:** `crates/openpulse-dsp/src/constellation.rs` (`qam_symbol_snr_db`); `plugins/ofdm`
  (extract `equalized_data_symbols`, refactor `ofdm_constellation` onto it, add `estimate_snr_db` + trait
  override); `plugins/scfdma` (extract `equalized_data_symbols` with `alpha_avg` + EMA-CE matching the decode
  path, refactor `scfdma_constellation` onto it, add `estimate_snr_db` + trait override). No engine change —
  the PR-A `rx_snr_db` seam already dispatches to `estimate_snr_db` then M2M4, so the daemon picks it up.
- **Tests:** `symbol_domain_snr.rs` extended with OFDM52-16QAM/64QAM and SCFDMA26-32QAM tracking gates;
  `symbol_snr_ladder_climb.rs` assertion raised from "≥ SL9" to "≥ SL15" (renamed
  `strong_channel_climbs_into_the_ofdm_rungs`).
- **Test results (actually run):** AWGN sweep — OFDM52-16QAM plugin **9.4/20.4/32.3** dB at true 8/20/32
  (near-ideal, no saturation) vs M2M4 **−10.0**; OFDM52-64QAM 12.8/21.0/32.3; SCFDMA26-32QAM 15.0/25.2/36.1 vs
  M2M4 −6.1. End-to-end: the receiver-led ladder over a 35 dB AWGN link now **climbs to SL19** (was SL10
  before this change). Regressions: `scfdma-plugin` 35, `ofdm-plugin` 58, `openpulse-dsp` 90, full modem suite
  — pass; clippy `-D warnings` + fmt clean. This closes the OTA-ladder track: the dense OFDM rungs are now
  both correct (re-seat) and reachable over the air (this change).

## 2026-07-09 — feat(profile): re-seat the hpx_hf dense HF rungs (SL11–SL19) from SC-FDMA to OFDM

- **Requirement/change:** the Fable audit claimed OFDM decisively beats SC-FDMA on frequency-selective HF
  fading at equal rate, and the profile itself admitted "the whole single-carrier segment fails moderate_f1
  fading by design." If true the dense rungs were on the wrong waveform.
- **Design decision (measurement-driven):** ran a matched-rate, matched-Watterson-draw, coded (SoftConcatenated)
  bake-off (`tests/ofdm_scfdma_bakeoff.rs`). A Fable-model review confirmed the comparison is *fair* — OFDM52
  and SCFDMA52 carry the identical 52-SC / 32-sample-CP / 13-pilot geometry (net-rate delta = 0), so OFDM is
  not buying ISI immunity with rate; SC-FDMA simply cannot represent a >±10-sample delay (its `DelayCe` basis
  reach), while OFDM's CP rides it. Re-seat the **wideband** rungs to OFDM; keep **SL10** on SC-FDMA (narrowband
  ~1 kHz fallback — an SNR/interference role, no OFDM 26-SC equivalent); fold the former P4 dense-pilot rungs
  SL14/SL18 onto plain `OFDM52-64QAM` (OFDM's CP makes the dense-pilot trick unnecessary — they now duplicate
  SL15/SL19, a redundant step flagged for a later pre-release re-index). In-place mode-string swap only (keeps
  all SL-index references and floors valid; a full re-index touches 80+ sites). Profile floors kept
  (SC-FDMA-derived, conservative, safe upper bound — OFDM works on fading); floor tightening to reclaim
  throughput is a documented follow-up.
- **Implementation:** `crates/openpulse-core/src/profile.rs` — `hpx_hf` `modes[SL11..=SL19]` → `OFDM52-{8PSK,
  16QAM,32QAM,64QAM,…}`; comment table + fading note updated. Test updates: `crates/openpulse-core/tests/
  session_profile.rs` (mode assertions), `crates/openpulse-cli/tests/cli_mode_advisor.rs` (mode strings),
  `crates/openpulse-modem/tests/ldpc_ladder_rungs.rs` (register OFDM; `MEASURED_AWGN_FLOOR_DB` re-measured for
  OFDM: SL16–SL19 = 12/16/20/20 dB), `crates/openpulse-modem/tests/symbol_snr_ladder_climb.rs` (register OFDM).
- **Tests:** `ofdm_scfdma_bakeoff.rs` — `bakeoff` (moderate_f1/f2 × 16QAM/64QAM SNR sweep), `bakeoff_benign`
  (AWGN + good_f1, no-trade-down check), and a non-ignored `reseated_sl12_decodes_on_moderate_f1` gate reading
  the mode from the profile.
- **Test results (actually run):** bake-off (40 paired draws) — moderate_f1 @20 dB 16QAM OFDM **0.88** vs SCFDMA
  **0.35**, 64QAM 0.52 vs 0.05; moderate_f2 @20 dB 16QAM **0.93** vs **0.03**, 64QAM 0.50 vs 0.00 (SC-FDMA flat
  across SNR = structural delay-cliff). Benign: AWGN ties (1.00=1.00), good_f1 OFDM ≥ SCFDMA (one −0.04 at
  64QAM/14 dB, noise). Re-seat gate: SL12 (OFDM52-16QAM) decodes on moderate_f1 @22 dB (≥0.70). Regressions:
  `session_profile` 30, `cli_mode_advisor` 3, `ldpc_ladder_rungs` 2 (+1 ignored probe), full modem suite —
  pass; clippy `-D warnings` + fmt clean. NOTE: the OTA ladder still stalls climbing *into* the OFDM rungs
  until multicarrier `estimate_snr_db` (Item 2 PR-B) lands — the re-seat makes the rungs correct; PR-B makes
  them reachable over the air.

## 2026-07-09 — feat(rate): per-plugin symbol-domain RX SNR (PSK) replaces M2M4 for the OTA decision

- **Requirement/change:** the receiver-led OTA ladder was capped ~SL8 because its SNR estimate is M2M4,
  which assumes a constant-modulus envelope. Measured (this session, AWGN sweep on the crossfade-pulse
  PSK rungs): M2M4 **saturates at ~15.3 dB** — flat from 26 dB up, it can never read higher — so a rung
  whose SNR ceiling exceeds ~15 dB can never be promoted to. `set_rx_snr_estimate` is test-only, so
  production ran M2M4 unconditionally.
- **Design decision:** add `ModulationPlugin::estimate_snr_db` (default `None`, mirroring the
  `estimate_afc_hz` override pattern) and prefer it over M2M4 in the engine. Per-plugin it measures noise
  from the component of each equalized symbol *orthogonal* to its decision — the already-calibrated
  `psk_symbol_noise_var` (reused, not a new estimator) — via `snr_db_from_amp_noise(A,σ²)=10·log10(A²/2σ²)`.
  Scoped to the two rungs that gate escaping the cap: **QPSK500 + 8PSK500** (SL8/SL9). The promotion into
  SL10 is decided *while receiving* those PSK modes, so accurate PSK SNR alone lets the ladder reach the
  SC-FDMA rungs; multicarrier `estimate_snr_db` (which needs demod-internal `h_est`/`noise_var`) is a
  deliberate follow-up (PR-B), where it falls back to M2M4 with no regression. Known limitation (measured):
  the symbol estimate **over-reads ~+5 dB at low SNR** (few reference points + decision-error amplitude
  bias) and **saturates at each mode's EVM floor** (~24 dB) — inherent; the OTA hysteresis/NACK-downshift
  absorbs the low-SNR optimism.
- **Implementation:** `crates/openpulse-core/src/plugin.rs` (`estimate_snr_db` trait default);
  `crates/openpulse-dsp/src/constellation.rs` (`snr_db_from_amp_noise`); `plugins/qpsk/src/demodulate.rs`
  (extracted shared `extract_data_symbols`, refactored `qpsk_demodulate_soft` onto it, added
  `estimate_snr_db`) + `plugins/qpsk/src/lib.rs` override; `plugins/psk8/src/demodulate.rs`
  (`estimate_snr_db` reusing its `extract_data_symbols`) + `plugins/psk8/src/lib.rs` override;
  `crates/openpulse-modem/src/engine.rs` (new `rx_snr_db(mode,samples)` helper — plugin estimate then
  M2M4 fallback — replacing all four `m2m4_snr_db_gated_from_real` call sites, the OTA one measuring on
  the decoded/recommended candidate mode).
- **Tests:** `crates/openpulse-modem/tests/symbol_domain_snr.rs` — AWGN sweep asserts the plugin estimate
  rises 8→20 dB and, at 32 dB, reads ≥5 dB above M2M4's saturation ceiling (deterministic, seeded).
  `crates/openpulse-modem/tests/symbol_snr_ladder_climb.rs` — real two-engine `OtaRateController` bridged
  through AWGN at the MODCOD FEC; a 35 dB channel must climb past SL8.
- **Test results (actually run):** measured curves — 8PSK500 plugin 8→13.5 / 20→21.8 / 32→23.8 dB vs
  M2M4 32→15.3; QPSK500 plugin 8→13.8 / 20→21.0 / 32→22.9 vs M2M4 32→15.2. Ladder-climb **reached SL10**
  (past the SL8 cap; stalls at SL10 where SC-FDMA hands back to M2M4 — the PR-B boundary). Regressions:
  qpsk-plugin 39+2, psk8-plugin 25+2+1, openpulse-dsp 90, openpulse-core 258 — all pass; clippy
  `-D warnings` + fmt clean.

## 2026-07-09 — feat(engine): HARQ soft-LLR combining across OTA retransmissions

- **Requirement/change:** the #694 union (decode each attempt standalone, then MAP-combine) had **zero
  production callers** — `receive_with_llr_combining` is synchronous multi-capture and RS-only, so it never
  fit the daemon's async, per-MODCOD OTA flow. Its measured deep-fade diversity gain (0.43 → 0.67 on
  `moderate_f1`) never reached the air; a NACK simply discarded the failed burst's soft information.
- **Design decision:** build the combining into the **shared OTA decode seam** (`ota_decode_and_ack`), not
  the daemon, so both `ota_decode_burst` (the daemon rx path) and any test get it, and it persists across the
  daemon's one long-lived engine by construction. Keep it **additive**: the existing standalone candidate loop
  is untouched and runs first; combining engages only when every standalone candidate failed (the
  retransmission path) — the standalone-then-combine union, now stateful. Retain the failed burst's soft LLRs
  keyed by `(session, mode)`, MAP-combine only same-length retained vectors (a mismatched length is a different
  frame and must not misalign), clear on any success (a delivered frame's LLRs must not bleed into the next),
  and cap retention at 3 bursts/mode. Each attempt is demodulated under its own AFC, then soft-combined — the
  correct HARQ model.
- **Implementation:** `crates/openpulse-modem/src/engine.rs` — new `ota_retained_llrs` /
  `ota_retained_session` engine state (cleared on `start_ota_session`/`stop_ota_session`); `decode_combined_llrs`
  (soft-LLR → payload dispatch for SoftConcatenated/Ldpc/LdpcHighRate/Rs/RsInterleaved, running the same
  `DemodulateDecode` → `HpxStateUpdate` routing + `FrameReceived` emit as the live path, side effects only after
  a successful frame decode); `ota_demodulate_soft` (front-end-seam soft demod for retention); the additive
  combining block appended to `ota_decode_and_ack`. No daemon change — `server::run`'s `rx_ticker` already calls
  `ota_decode_burst` on its persistent engine.
- **Tests:** `crates/openpulse-modem/tests/ota_harq_combining.rs` — over 50 `moderate_f1` fade realisations at
  14 dB (SCFDMA52-16QAM + SoftConcatenated, hpx_hf SL12), a single engine retaining+combining across 3
  sequential `ota_decode_burst` bursts must decode more frames than 3 independent engines each decoding one
  burst standalone. Paired on identical fade seeds (combining is a superset of standalone), so the gap is
  deterministic and the gate non-flaky.
- **Test results (actually run):** standalone (any-of-3) **0.64** vs combining **0.78** — +0.14, clears the
  +0.08 gate. OTA/HARQ regressions green (`ota_rate_lockstep` 2, `ota_channel_adaptation` 1,
  `harq_fade_diversity` 3, `harq_retry_watterson_integration` 10); modem clippy `-D warnings` clean.

## 2026-07-09 — fix(channel): normalise Watterson total path power (drop the +3 dB hot bias)

- **Requirement/change:** audit (measurement layer) found Watterson delivers ~+3 dB more SNR than
  configured, so every fading SNR label / ladder-floor margin is ~3 dB optimistic. Reproduced (delivered
  signal power ≈ 2× input).
- **Root cause (verified):** both `apply` and `apply_complex` sum two rays each with `E[|h|²]=1` →
  summed signal power 2, while the additive noise is keyed to the *input* RMS (power 1). Delivered
  SNR = labelled + 3 dB, for delay 0 and delay > 0 alike.
- **Design decision:** normalise total path power to 1 by scaling each equal-power ray by
  `1/√(#rays) = 1/√2` (standard Watterson convention). Signal power out = input, so the input-keyed noise
  yields the labelled SNR.
- **Implementation:** `crates/openpulse-channel/src/watterson.rs` — `ray_scale = 1/√2` applied to both
  rays in `apply` and `apply_complex`.
- **Tests:** `total_path_power_normalized_to_unity` — at 60 dB SNR (noise negligible), mean(out²)/mean(in²)
  ∈ [0.75, 1.35] (≈1.0), catching the ≈2.0 regression.
- **Test results (actually run):** new guard passes (ratio ≈ 1.0); **zero cascade** — the full
  `cargo test --workspace --exclude pki-tooling --no-default-features` still passes (fading tests are
  outage/threshold-dominated with margin to absorb the 3 dB), so no decode-guard recalibration was needed;
  channel crate 46 tests pass; clippy `-D warnings` clean; fmt clean. NOTE: fading SNR *labels* in test
  reports are now ~3 dB harder (i.e. honest); ladder floors calibrated on AWGN are unaffected.

## 2026-07-09 — fix(dsp): level-normalise before PSK carrier recovery (no-AGC coupling)

- **Requirement/change:** audit found a quiet station with a small sub-deadband carrier offset fails with
  AGC off, because the PSK Costas loop bandwidth scales with receive amplitude. Reproduced independently
  (QPSK500 ×0.05 + 1.5 Hz: fails AGC-off, passes AGC-on).
- **Root cause (verified):** the carrier-loop discriminants are amplitude-dependent — CarrierPll order 1
  `q·sgn(i)` (BPSK) and order 2 `q·sgn(i)−i·sgn(q)` (QPSK), and 8PSK-plain's `dd_track_seeded`
  `Im(r·conj(d))` — and no PSK plugin normalised symbol level before the loop. A ×0.05 station gives the
  loop ~1/20 the gain, so it cannot acquire even a ~1 Hz residual over a short frame. (8PSK-RRC's CarrierPll
  order 3 is angle-based and already immune; 64QAM already normalises by `denom`.)
- **Design decision:** normalise the symbol stream to unit RMS before the loop — a **no-op at nominal
  amplitude** (unit-energy PSK sits at RMS ≈ 1, so the tuned acquisition is untouched), a single uniform
  scale (phase and the calibrated soft-LLR scale ∝ amp/σ² are invariant), and it restores level-invariant
  loop gain for weak signals. Preferred over re-deriving the discriminants (which would shift loop dynamics
  — the most churn-prone area in the repo). Confirmed load-bearing: neutralising the helper reintroduces the
  QPSK failure.
- **Implementation:** `crates/openpulse-dsp/src/constellation.rs::normalize_stream_rms`; called in the
  non-RRC carrier paths of `plugins/qpsk` (hard+soft), `plugins/psk8`, and inline in `plugins/bpsk` before
  its Costas loop.
- **Tests:** `crates/openpulse-modem/tests/carrier_level_invariance.rs` — QPSK500/8PSK500/BPSK250 at ×0.05
  + a sub-deadband offset, AGC off, must decode (with a full-amplitude control).
- **Test results (actually run):** new gate passes (and fails with the helper neutralised); PSK plugin
  suites unchanged (`bpsk-plugin` 24, `qpsk-plugin` 39, `psk8-plugin` 25 — nominal acquisition untouched);
  `cargo test --workspace --exclude pki-tooling --no-default-features` all pass; clippy `-D warnings` clean;
  fmt clean.

## 2026-07-09 — fix(engine): carrier-detect before the AGC so its boost can't wedge the squelch

- **Requirement/change:** audit found an AGC/DCD seam-ordering deadlock. Reproduced independently (weak
  burst → +26.5 dB gain → sub-squelch noise reads busy with AGC on, clear with AGC off).
- **Root cause (verified):** the AGC lives inside the `route_audio_stage(InputCapture)` seam, but every
  `dcd.update` ran on the samples the seam *returns* — i.e. POST-AGC. Once a weak burst ramps the gain and
  the active-span gate freezes it through silence, the held gain multiplies sub-squelch band noise back
  over the DCD busy threshold, so the channel reads "busy" forever and CSMA never releases TX. Self-
  sustaining (the same lock-gate that freezes the gain keeps it high). Same seam-gap class as the notch
  bug (#556/#557), reversed.
- **Design decision:** DCD is a squelch — it must measure the true channel level, not the AGC-normalised
  demod level. Move the DCD update into the single seam, positioned after DC-block+notch but **before**
  AGC, so every capture path gets pre-AGC carrier detect by construction; add a `dcd_blocks_processed`
  tripwire (per the seam checklist). The daemon burst-gate (`accumulate_routed`) now gates on the seam's
  pre-AGC `dcd.energy()` instead of recomputing a post-AGC RMS.
- **Implementation:** `crates/openpulse-modem/src/engine.rs` — `update_dcd_at_seam` (pre-AGC, event +
  tripwire) called in `route_audio_stage(InputCapture)`; removed the 18 redundant post-AGC DCD updates
  (16 direct + 2 `ota_update_dcd` calls + 3 inline `DcdChange` blocks folded into the seam helper);
  `accumulate_routed` gate switched to `dcd.energy()`.
- **Tests:** `crates/openpulse-modem/tests/dcd_pre_agc.rs` — after a weak burst boosts the gain, sub-
  squelch noise leaves `dcd_energy()` at the true ~0.001 level (< 0.01), not the boosted ~0.02; tripwire
  increments on the `accumulate_capture` path.
- **Test results (actually run):** new gate passes; `csma_loopback` 4, `agc_loopback` 4, `engine_events`
  8, `notch_loopback` 4 all unchanged; `cargo test --workspace --exclude pki-tooling --no-default-features`
  all pass; clippy `-D warnings` clean; fmt clean.

## 2026-07-09 — fix(ofdm): whiten the bit stream so CE-SSB can't crush low-entropy frames

- **Requirement/change:** audit found default-on CE-SSB breaks OFDM+FEC on a *clean* channel for
  low-entropy / RS-padded payloads. Independently reproduced (repeated-`0x5A`: fails CE-SSB on, passes off).
- **Root cause (verified):** a zero-run / repeated-byte payload maps every OFDM data subcarrier to the same
  constellation point; the IDFT of a constant spectrum is a time-domain impulse train (very high PAPR). The
  engine's CE-SSB peak-stretch conditioner (`stage_emit_output`, gated to QPSK-OFDM by `cessb_benefits`)
  then crushes it and the frame fails to decode even at ∞ SNR. RS padding of short frames triggers it in
  normal operation, so it is not merely a synthetic-payload edge case.
- **Design decision:** keep CE-SSB (a deliberately-tuned, on-air-validated +1.18 dB conditioner) and fix the
  *source* — whiten the modulated bit stream, the standard OFDM practice (DVB-T / 802.11). A fixed,
  position-indexed keystream decorrelates the subcarriers so no payload can produce the impulse train; it
  needs no negotiation (identical both ends) and is a pre-release wire-format change. Applied at the plugin's
  own `bytes_to_bits` seam (covers the low-entropy length prefix too), self-inverse on the hard path, LLR
  sign-flip on the soft path.
- **Implementation:** `plugins/ofdm/src/scramble.rs` (`scramble_bits`, `descramble_llrs`); wired in
  `modulate.rs` (after `bytes_to_bits`) and `demodulate.rs` (hard bits before `bits_to_bytes`; soft LLRs
  before `decode_len_prefix_llrs`).
- **Tests:** `plugins/ofdm/src/scramble.rs` units (self-inverse, whitens all-zeros, soft/hard agree);
  `crates/openpulse-modem/tests/cessb_ofdm_lowentropy.rs` — low-entropy OFDM52+Rs (4 payloads incl.
  all-zero/padded) and OFDM52+SoftConcatenated decode with CE-SSB on; high-entropy unaffected both states.
- **Test results (actually run):** new gates pass; `ofdm-plugin` 35/35; existing `cessb_engine` 4 and
  `channel_loopback` 12 unchanged (no OFDM regression); `cargo test --workspace --exclude pki-tooling
  --no-default-features` all pass; clippy `-D warnings` clean; fmt clean.

## 2026-07-09 — fix(dsp): correct the inverted LMS DFE feedback-update sign

- **Requirement/change:** an audit (Fable, 5-stream) flagged the LMS decision-feedback section as
  anti-adaptive. Independently reproduced and fixed here.
- **Root cause (verified):** `LmsEqualizer::filter()` returns `output = fwd − dfe` (the DFE output is
  **subtracted**), so `∂output/∂w_dfe = −d` and the steepest-descent step must be `w_dfe −= μ·e·conj(d)`.
  `lms_update` instead adapted the DFE taps with `+=` — the same sign as the forward taps — so the
  feedback section climbed the error gradient. On a pure post-cursor ISI channel `h=[1.0, 0.5]` (which a
  correct DFE cancels perfectly) this drove steady-state MSE from the forward-only **0.0001** up to
  **16.2 (dfe=1) / 29.0 (dfe=2)** until the `MAX_TAP_ENERGY` guard clamped it. Invisible to any
  identity-channel loopback (zero error → zero update), so every clean test stayed green; it silently
  injected noise on every DFE-enabled production profile (BPSK250-RRC, QPSK1000-HF/-RRC, 8PSK1000-HF/-RRC).
- **Design decision:** flip the DFE update sign only (the forward-tap update is correct — forward-only
  equalization already worked). The characterization-sweep lore ("DFE=2 sweet spot", "DFE≥3 hurts", "DFE
  removed as it hurts moderate_f1") was measured against the broken component and is now marked invalid;
  a post-fix re-run shows a correctly-signed DFE decisively cancels *static* post-cursor ISI but does NOT
  beat a forward-only equalizer on fast Watterson fading (decision-feedback error propagation), so the
  fading-profile DFE lengths are left unchanged pending a re-tune against the CODED metric (tracked
  follow-up). Comment corrected in `plugins/qpsk/src/demodulate.rs`.
- **Implementation:** `crates/openpulse-dsp/src/equalizer.rs` — `lms_update` DFE tap update `+=` → `−=`.
- **Tests:** `crates/openpulse-dsp/tests/dfe_postcursor_isi.rs` — new gate: adding DFE taps must not
  raise steady-state MSE on `h=[1.0,0.5]` (fails pre-fix at 16/29, passes post-fix).
- **Test results (actually run):** new gate passes; `openpulse-dsp` 90/90; DFE-enabled plugin guards
  unchanged (`bpsk-plugin` 24, `qpsk-plugin` 39, `psk8-plugin` 25, incl. all Watterson moderate/poor-F1
  guards); `cargo test --workspace --exclude pki-tooling --no-default-features` all pass; clippy
  `-D warnings` clean; fmt clean.

## 2026-07-09 — fix(psk8): cancel the rectangular-pulse crossfade ISI (and gate QPSK's cancellation)

- **Requirement/change:** the PR-#687 calibration probe found QPSK/8PSK `mean(|LLR|)` stops tracking SNR
  above ~12 dB. QPSK was traced and fixed in #695; this closes the 8PSK half.
- **Root cause (measured + derived):** 8PSK's rectangular modulator uses the *identical* raised-cosine
  crossfade as QPSK (`sym_k·w_tail + sym_{k+1}·w_head`, `w_tail = ½(1+cos πi/n)`), but its matched
  one-slot demod integrates against the **squared** window `w_tail²` (not QPSK's un-squared `w_tail`). So
  it recovers `A·(sym_k + β·sym_{k+1})` with `β = Σ w_head·w_tail² / Σ w_tail³` and the common scale
  `A = Σ w_tail³ / Σ w_tail⁴` dividing out. Unlike QPSK's `β = ⅓` (n-independent), the cubed/quartic
  weighting makes β **vary with oversampling**: 0.182 at n=16 (8PSK500), 0.167 at n=8 (8PSK1000). Measured
  recovered-symbol EVM floored at **−13.7 dB (8PSK500) / −12.2 dB (8PSK1000)** from ~16 dB SNR upward. The
  ISI is anti-causal (next symbol), so the DFE cannot reach it; hard decisions are unaffected (±22.5°
  8PSK margin), so no BER test caught it.
- **Design decision:** back-substitution `s_k = p_k − β·s_{k+1}` (bidiagonal, stable — error scales by
  `β < 0.2` per step, exact terminal since the modulator zeroes the last symbol's successor; noise gain
  `1/(1−β²) ≈ 1.03`). β is **computed from the actual window per-n** rather than hard-coded, because it is
  not n-independent for the squared window. Applied on the plain rectangular demod path only, after
  `demodulate_symbols`, before carrier/equalizer — **gated to `!cosine_overlap`**: the cosine-overlap
  (`-HF`) pulse is a per-symbol `sin²` bump with no crossfade, so cancelling there injects error. The same
  gate was added to QPSK #695, which ran the cancellation unconditionally on its non-RRC path (a latent
  soft-path corruption of the `QPSK1000-HF` cosine-overlap mode that its BER-tolerant round-trip test
  could not see).
- **Implementation:** `plugins/psk8/src/demodulate.rs` — `crossfade_isi_beta(n)`, `cancel_crossfade_isi`,
  called in `extract_data_symbols` under `if !cosine_overlap`; `extract_data_symbols_for_test` accessor.
  `plugins/qpsk/src/demodulate.rs` — both cancellation call sites now `if !cosine_overlap`.
- **Tests:** `plugins/psk8/tests/crossfade_isi.rs` — `evm_clears_the_crossfade_floor_at_high_snr`
  (8PSK500 < −18 dB, 8PSK1000 < −14 dB @40 dB), `cosine_overlap_hf_mode_stays_clean` (8PSK1000-HF < −30 dB).
- **Test results (actually run):** 8PSK EVM @40 dB: 8PSK500 **−13.7→−20.0 dB** (Δ6.3), 8PSK1000
  **−12.2→−14.7 dB** (Δ2.5; capped by the separate 8-sps timing residual, same as QPSK1000), 8PSK1000-HF
  −45.7 dB unchanged (correctly gated). `cargo test -p psk8-plugin --no-default-features` all pass;
  `cargo test -p qpsk-plugin` 39 pass (QPSK guard did not regress `QPSK1000-HF` round-trips);
  `openpulse-modem` `llr_calibration` 2/2 pass; `cargo test --workspace --exclude pki-tooling
  --no-default-features` all pass; clippy `-D warnings` clean; fmt clean.

## 2026-07-09 — fix(qpsk): cancel the rectangular-pulse crossfade ISI

- **Requirement/change:** the PR-#687 calibration probe found QPSK/8PSK `mean(|LLR|)` stops tracking SNR
  above ~12 dB — a receiver-internal residual, flagged as unexplained. Traced here for QPSK.
- **Root cause (measured + derived):** recovered-symbol EVM floors at **−9.7 dB** on every *rectangular*
  QPSK rung (QPSK250/500/1000) from 16 dB SNR upward, while the RRC rung tracks cleanly to −37 dB. The
  rectangular ("plain") modulator blends adjacent symbols with a raised-cosine crossfade — sample `i` of
  slot `k` is `sym_k·w_tail(i) + sym_{k+1}·w_head(i)`, `w_tail = ½(1+cos πi/n)`. A one-slot
  integrate-and-dump (`demodulate_symbols`) therefore recovers `p_k = sym_k + β·sym_{k+1}` with
  `β = Σ w_head·w_tail / Σ w_tail² = 1/3` exactly (independent of `n`). `β² = −9.55 dB` — the measured
  floor. The ISI is on the *next* symbol (anti-causal), so the downstream DFE, which feeds back past
  decisions, cannot reach it. Hard decisions are unaffected (45° QPSK margin), so no BER test caught it.
- **Design decision:** `p_k = sym_k + β·sym_{k+1}` is bidiagonal → `sym_k = p_k − β·sym_{k+1}` recovers it
  exactly by backward substitution. Stable (each step scales the running error by `β = 1/3`, so it decays
  backward) with an exact terminal (the modulator sets the last symbol's successor to 0). Noise gain is
  `1/(1−β²) = 1.125` (+0.5 dB), so low-SNR EVM improves too. Applied on the rectangular demod path only,
  after `demodulate_symbols` and before carrier/equalizer — **not** inside the timing search.
- **Implementation:** `plugins/qpsk/src/demodulate.rs` — `CROSSFADE_ISI_BETA`, `cancel_crossfade_isi`,
  called in `qpsk_demodulate` and `qpsk_demodulate_soft`.
- **Tests:** new `plugins/qpsk/tests/crossfade_isi.rs::evm_clears_the_crossfade_floor_at_high_snr` —
  EVM at 40 dB must be < −13 dB (past the −9.5 dB floor). **Fails on the pristine tree at −9.7 dB.**
  `notch_loopback::notch_recovers_decode_against_out_of_band_qrm` tightened (QRM tone amp 4.0 → 8.0): the
  cancellation made the receiver decode through the old tone, so the test's "baseline is corrupted"
  precondition needed a harsher one — a *positive* side-effect, recorded in the test.
- **Test results:** EVM at 40 dB, before → after: QPSK250 −9.7 → **−26.6**, QPSK500 −9.8 → **−20.9**,
  QPSK1000 −9.5 → **−15.5** (the QPSK1000 residue is a separate 8-sps timing effect). Downstream, this is
  a real *coded* win, not just EVM. Soft-concatenated FEC floor over AWGN, QPSK250: 4 dB 0.58 → **0.75**,
  5 dB 0.88 → **1.00**; **QPSK500 was stuck at 0.00 at 5–6 dB and now decodes** (5 dB → 0.04, 6 dB → 0.17).
  HARQ soft combining over Watterson `good_f1`, QPSK250: 6 dB 0.80 → **0.93**, 8 dB 0.85 → **0.95**;
  QPSK500 8 dB 0.70 → **0.80**. Full workspace `--exclude pki-tooling --no-default-features`: **1558
  passed, 0 failed**. clippy `-D warnings` + fmt clean.
- **Scope:** QPSK only. 8PSK's rectangular path shares the pattern and shows the same `mean(|LLR|)` stall
  (×1.8 over 8→24 dB in #687); a follow-up applies the same cancellation there.

## 2026-07-08 — fix(engine): decode each HARQ attempt standalone before combining them

- **Requirement/change:** the P7 rejection (PR #693) concluded that what limits SC-FDMA on HF is deep-fade
  outage over a frame, and that the only remaining levers are *diversity* levers above the plugin. The
  first of those is HARQ soft combining across retransmissions, whose machinery was already shipped
  (`combine_llrs_map`, PR #686) and calibrated (#687, #690). This measures it and fixes what it found.
- **Measurement — neither strategy dominates.** Frame success over Watterson `moderate_f1`, three
  independent fade realisations, RS FEC, 60 trials:

  | rung | SNR | plain ARQ retry | soft combining alone | union of both |
  |---|---|---|---|---|
  | SCFDMA52-16QAM | 20 dB | 0.28 | 0.40 | **0.50** |
  | SCFDMA52-16QAM | 28 dB | 0.43 | 0.48 | **0.67** |
  | SCFDMA52 | 12 dB | 0.87 | 0.88 | **0.95** |
  | SCFDMA52 | 20 dB | 0.97 | **0.95** | **1.00** |
  | SCFDMA52 | 28 dB | 0.97 | **0.97** | **1.00** |

  Combining wins where every attempt is partially ruined and they carry complementary information (the dense
  rungs in outage). Plain retry wins where one attempt is simply clean and summing it with two ruined ones
  dilutes it — note SCFDMA52 at 20 dB, where **combining alone is *worse* than not combining at all**.
- **Design decision:** take the union. `receive_with_llr_combining` now RS-decodes each attempt **on its
  own** first, and only if every attempt fails does it sum the LLRs and decode the combination. Each
  standalone trial is one RS decode over LLRs already in memory, so the union costs almost nothing and its
  success is a strict superset of either strategy. The trials must not move state: only the winner runs
  `route_decoded_stage(HpxStateUpdate)` and emits `FrameReceived`.
- **Implementation:** `crates/openpulse-modem/src/engine.rs` — `receive_with_llr_combining`, plus a shared
  `hard_decide` helper (the LLR→bytes packing was inline and is now used twice).
- **Tests:** new `crates/openpulse-modem/tests/harq_fade_diversity.rs`.
  `combining_beats_plain_retry_on_a_fading_channel` (SCFDMA52-16QAM, `moderate_f1` @28 dB: the engine must
  beat plain retry by >0.10) and `combining_never_loses_a_frame_plain_retry_would_have_kept` (SCFDMA52 at
  12/20/28 dB: the engine must never fall below plain retry). **Both fail on the pristine tree** — the first
  at 0.48 vs 0.45, the second at 0.95 vs 0.97, which is the regression the union removes.
- **Test results:** full workspace `--exclude pki-tooling --no-default-features`: **1557 passed, 0 failed**.
  clippy `-D warnings --all-targets` + fmt clean.
- **Method note:** "soft combining is better than retrying" is the kind of claim that reads as obviously
  true and is measurably false half the time. The union is only free because both baselines were measured
  separately first.

## 2026-07-08 — research: P7 (frequency-domain IBDFE) built, measured, rejected

- **Requirement/change:** research item P7, "frequency-domain iterative block DFE after MMSE — cancel
  residual ISI at spectral notches. 1–2 dB on frequency-selective (Watterson) channels for 16/64QAM."
  Unblocked by PR #690's LLR calibration, which its feedback-reliability estimate depends on.
- **Implementation (built, then reverted):** soft-feedback (Tüchler) IBDFE in `plugins/scfdma/src/demodulate.rs`
  — a shared `equalize_despread` seam for the hard, soft and both GPU paths; feedforward
  `C_k = Ĥ*/(σ² + v̄|Ĥ|²)`, bias `μ = (1/N)ΣC_kĤ_k`, feedback `B_k = C_kĤ_k − μ`, `Z_k = C_kY_k − B_kX̄_k`;
  posterior soft symbols `x̄_m, v_m` from `symbol_llrs` re-spread with the transmitter's DFT convention;
  two iterations; flat-channel (`var(α)/ᾱ² < 0.01`) and `v̄ > 0.5` entry gates; strict no-harm acceptance on
  the measured decision residual. Iteration 0 collapses exactly to the existing MMSE path.
- **Test results — the loop works.** Per-symbol decision residual drops ~5× on the first accepted iteration;
  uncoded BER on a static two-ray channel roughly halves (SCFDMA52-16QAM, `x[n] + 0.9·x[n−4]`, 16 dB:
  0.027 → 0.019). Coded frame success (`SoftConcatenated`):

  | channel | rung | before | after |
  |---|---|---|---|
  | static `1 + 0.9·z⁻⁴` @10 dB | SCFDMA52-16QAM | 0.42 | **0.62** |
  | static `1 + 0.9·z⁻⁴` @12 dB | SCFDMA52-32QAM | 0.04 | **0.12** |
  | Watterson `good_f1` @12 dB (100 frames) | SCFDMA52-16QAM | 0.77 | 0.78 |
  | Watterson `moderate_f1` @12 dB (100 frames) | SCFDMA52-16QAM | 0.26 | 0.29 |
  | Watterson `moderate_f1` @24 dB (100 frames) | SCFDMA52 | 0.79 | 0.78 |

  Every Watterson cell is inside Monte-Carlo noise (σ ≈ 0.05 at 100 frames). The gate fires on 26 % of
  symbols and accepts ~1.1 iterations on each, so it is running, not being skipped.
- **Decision: rejected, code reverted.** The 2.7–3.8 dB MMSE→matched-filter-bound headroom is real but is a
  *SINR* bound on symbols the equalizer can still see. Watterson's frame failures are **deep-fade outages**,
  not SINR-limited symbols. The static two-ray channel — deterministic notch, every frame sees it — is
  exactly where the bound converts, and there IBDFE delivers ~1 dB. HF does not look like that. Shipping it
  would add 2× per-symbol CPU on a quarter of symbols for no measurable gain on the channels the ladder runs.
- **The trap, recorded for any future attempt:** the refined pass's own model variance
  `[σ²Σ|C|² + v̄Σ|B|² + Σ|C|²ε²]/μ²` is **wrong in the dangerous direction**. It assumes `v̄` is the true
  posterior symbol variance and that the feedback error is independent of the noise; neither holds. Using it
  made the refined LLRs **90× over-confident** (caught by `plugins/scfdma/tests/llr_reliability.rs`, added in
  PR #690), and confidently-wrong bits cost the soft Viterbi more than the cancellation gained it — coded
  frame success did not move at all. Scaling iteration 0's variance by `δ_i/δ_0` halves the over-confidence
  to 20× and still fails the gate. The only calibration-safe choice: keep iteration 0's variance and claim
  the improvement in the symbol *estimates*, not in their confidence. Every coded number above was taken
  that way. Had `llr_reliability` not existed, this would have shipped as a 90×-miscalibrated equalizer with
  a plausible uncoded-BER story.
- **Roadmap consequence:** every equalizer-side item is now spent. What limits SC-FDMA on HF is deep-fade
  outage over a frame, and the remaining levers are *diversity* levers above the plugin — Memory-ARQ / HARQ
  soft combining across retransmissions (`combine_llrs_map`, already shipped and calibrated) and the ladder
  downshift `hpx_hf` already performs. P5 (second-pass decision-directed CE) shares P7's feedback structure
  and should be expected to share its result; measure coded frame success on Watterson before building it.

## 2026-07-08 — feat(profile): high-rate-LDPC top rungs SL16–SL19 (research item P6)

- **Requirement/change:** research item P6, "LDPC on the dense rungs — swap `SoftConcatenated` for
  `LdpcHighRate`/`Ldpc`, ~1–3 dB vs RS soft-concat at the same rate". Enabled by the multi-block LDPC of
  PR #691.
- **The premise was wrong, and the measurement says so.** LDPC is not "the same rate" as
  `SoftConcatenated` — it is 2.03× the rate (r≈8/9 against r≈0.437) and therefore *costs* SNR rather than
  saving it. Measured AWGN floors (62-byte payload, 90 % frame success, 32 frames/point, 1 dB grid):

  | mode | `SoftConcatenated` | `LdpcHighRate` | Δ |
  |---|---|---|---|
  | SCFDMA26-32QAM | 5 dB | 11 dB | +6 |
  | SCFDMA52-8PSK | 5 | 10 | +5 |
  | SCFDMA52-16QAM | 7 | 14 | +7 |
  | SCFDMA52-32QAM | 8 | 15 | +7 |
  | SCFDMA52-64QAM-P4 | 15 | 19 | +4 |
  | SCFDMA52-64QAM | 13 | 21 | +8 |

  ~6 dB for 2× the rate is a *worse* trade than climbing one modulation order (8PSK→16QAM buys 1.33× for
  ~2 dB). So wherever a denser constellation still exists, `SoftConcatenated` on it wins, and a swap
  would have *lowered* throughput at every rung's operating SNR.
- **Design decision:** the exception is the **top**. 64QAM is the densest constellation the SC-FDMA plugin
  has, so above SL15 the only remaining lever on throughput is code rate. Add SL16–SL19 as MODCOD pairs of
  SL12–SL15 at `LdpcHighRate` — exactly the pattern SL6/SL7 already use for QPSK250. Floors carry the same
  +9 dB fading margin over their measured AWGN floor that SL11–SL13 and SL15 do (14/15/19/21 → 23/24/28/30),
  and ceilings follow the uniform `ceiling(L) = floor(L+1) + 2` rule from PR #680. The ACK-UP admission
  gate moves from SL15 to the new densest rung SL19. Pre-release, so the `fingerprint()` change carries no
  ladder-interop concern.
- **Implementation:** `crates/openpulse-core/src/profile.rs` — `hpx_hf()` modes/fec_modes/floors/ceilings/
  gate, plus the measurement table and the reasoning in the body comment.
- **Tests:** new `crates/openpulse-modem/tests/ldpc_ladder_rungs.rs` —
  `ldpc_top_rungs_decode_at_their_calibrated_awgn_floor` (each new rung ≥ 85 % frame success at the AWGN
  SNR it was placed from) and `scfdma_rungs_never_lengthen_the_air_time_and_ldpc_shortens_it_sharply`
  (airtime monotone over SL10–SL19, and every LDPC rung ≥ 25 % shorter than SL15 — the claim that
  justifies their higher floors). New `session_profile::hpx_hf_floors_are_monotonic_and_ceilings_follow_the_hysteresis_rule`.
  `channel_loopback.rs` already round-trips every defined rung of every profile, so SL16–SL19 are covered
  there by construction.
- **Test results:** ladder top rate **3790 → 7710 bps**. New rungs (mode, FEC, gross×rate, floor):
  SL16 SCFDMA52-16QAM+LHR 5141 bps @23 dB, SL17 SCFDMA52-32QAM+LHR 6426 @24, SL18 SCFDMA52-64QAM-P4+LHR
  7265 @28, SL19 SCFDMA52-64QAM+LHR 7710 @30. Full workspace `--exclude pki-tooling
  --no-default-features`: **1555 passed, 0 failed**; quick test matrix 555/555. clippy `-D warnings
  --all-targets` + fmt clean.
- **Honest note:** SL18 and SL19 tie on airtime for any payload a `u8` length can express — 64QAM-P4 carries
  16 pilots to 64QAM's 13, so its gross rate is only 6 % lower, below the resolution of a whole number of
  SC-FDMA symbols. The pair earns two rungs on P4's fading robustness (denser pilot comb), exactly as
  SL14/SL15 already do. The test asserts non-increasing airtime rather than pretending otherwise.

## 2026-07-08 — feat(engine): multi-block LDPC (unblocks research item P6)

- **Requirement/change:** research item P6 is "LDPC on the dense rungs". It was blocked: the engine's LDPC
  path rejected any frame larger than one 128-byte information block
  (`"LDPC: encoded frame N B exceeds one-block limit"`), so no ladder rung could use it for a realistic
  payload.
- **Design decision:** split the wire frame into `codec.info_bytes()`-sized blocks, encode each
  independently and concatenate the codewords; zero-pad the last block. `Frame::decode` reads its own
  `payload_len` field and validates a CRC over the prefix, so the padding is discarded on receive. A
  `Frame`'s payload length is a `u8`, so the wire frame never exceeds 265 bytes — at most **three** blocks,
  bounded by construction. The receiver derives the block count from the LLR count, which is exact: the
  soft demodulators trim modulation padding with their own length prefix.
- **Implementation:** `crates/openpulse-modem/src/engine.rs` — `encode_ldpc_blocks`, `decode_ldpc_llrs`
  (now multi-block), `transmit_with_ldpc_codec`; the single-block guard and its error are gone.
- **Tests:** `crates/openpulse-modem/tests/ldpc_engine_loopback.rs` — the two tests that asserted the
  one-block *rejection* (`ldpc_rejects_frame_larger_than_one_block`,
  `ldpc_high_rate_shares_one_block_limit`) are replaced by
  `ldpc_round_trips_a_frame_spanning_several_blocks` and its high-rate twin, over payloads 117/118/125/200/255 B
  (one, two and three blocks). Engine round-trips through AWGN at 30 dB verified for every payload length
  from 1 to 255 B on both codec presets.
- **Test results:** full workspace `--exclude pki-tooling --no-default-features`: **1552 passed, 0 failed**.
  clippy `-D warnings --all-targets` + fmt clean.
- **Calibration data this unblocks** (AWGN, 90 % frame-success floor, SC-FDMA rungs, 20 frames/point).
  LDPC's *floors* are 2–5 dB worse than `SoftConcatenated` — but its airtime is 0.37–0.56×, because
  `SoftConcatenated` pads every frame to a 255-byte RS block before the rate-1/2 convolutional layer. The
  ladder cares about **throughput at a given SNR**, and on that axis LDPC dominates. At a 213-byte payload
  the frontier becomes:

  | SNR | throughput | rung |
  |---|---|---|
  | 3 dB | 557 bps | SCFDMA52 + SoftConcatenated |
  | 4 dB | 1052 bps | SCFDMA52 + Ldpc |
  | 6 dB | 1690 bps | SCFDMA52 + LdpcHighRate |
  | 10 dB | 2254 bps | SCFDMA52-8PSK + LdpcHighRate |
  | 12 dB | 2784 bps | SCFDMA52-16QAM + LdpcHighRate |
  | 14 dB | 3156 bps | SCFDMA52-32QAM + LdpcHighRate |
  | 18 dB | 3641 bps | SCFDMA52-64QAM-P4 + LdpcHighRate |

  Against the current all-`SoftConcatenated` dense rungs that is **2.0× the throughput at 12 dB** (2784 vs
  1392) and **2.5× at 18 dB** (3641 vs 1479). The `hpx_hf` retune is a separate change.

## 2026-07-08 — fix(scfdma): LLR noise variance must include CE error and residual ISI

- **Requirement/change:** research item #8. `mmse_llr_noise_var` modelled only the additive noise through
  the equalizer, `σ²·|C_k|²`, treating the channel estimate as exact and the DFT de-spread as ISI-free.
- **Measured defect:** a true LLR `L` predicts `P(bit wrong) = 1/(1 + e^{|L|})`. Binning the emitted LLRs
  by `|L|` and counting actual bit errors (SCFDMA52-16QAM, 12 dB, **flat** channel): `|L| ≈ 3.1` → 2.0×
  the promised error rate, `|L| ≈ 6.3` → 9.1×, `|L| ≈ 12.0` → **71.2×**. On a two-ray channel (a=0.9,
  d=4) the same bins gave 3.0×, 11.0× and 96.1×.
- **Design decision:** the post-despread error variance has three terms, not one.
  `σ²_LLR = [ mean(σ²·|C_k|²) + var(α_k) + mean(|C_k|²·ε²_k) ] / ᾱ²`, where `α_k = |Ĥ_k|²/(|Ĥ_k|²+σ²)`.
  (2) is residual ISI: the DFT de-spread averages the per-SC gains, so only their *spread* survives as
  self-interference — zero on a flat channel, dominant at a notch. (3) is channel-estimate error, read
  straight off the estimator: `ε²_k = σ²_h · Σ_j |recon[k][j]|²` with `σ²_h = σ² / PILOT_AMPLITUDE²`,
  exposed as `CeSolver::ce_error_var_per_sc`. Frame-constant, so it is computed once in `FrameFront` and
  shared by the hard, soft, constellation and both GPU paths at the single seam.
- **Implementation:** `plugins/scfdma/src/channel.rs` — `CeSolver::{recon_row_energy, ce_error_var_per_sc}`,
  `mmse_llr_noise_var` gains the `ce_error_var` argument and the two terms;
  `plugins/scfdma/src/demodulate.rs` — `ce_error_var()` helper, all five call sites.
- **Tests:** new `plugins/scfdma/tests/llr_reliability.rs` —
  `llrs_are_not_wildly_over_confident` bins `|L|` and compares the empirical bit-error rate against
  `1/(1+e^{|L|})`, bounding the worst bin at 4× (the residue is the max-log-MAP approximation, which no
  variance term can undo). **Fails on the pristine tree at 68.8×.**
- **Test results:** worst-bin over-confidence, SCFDMA52-16QAM at 12 dB: flat **9.1× → 2.2×**, two-ray
  a=0.9 **11.0× → 1.0×**; at `|L| ≈ 12` the 71× and 96× errors vanish entirely.
  **No measured decode or HARQ gain, and that is expected**: soft Viterbi, min-sum LDPC and max-log turbo
  are scale-invariant, and the missing terms are close to a per-frame constant. `scfdma_ce_sweep`
  Watterson `good_f1` sum 31.40 → 31.42 of 42; AWGN unchanged; HARQ thresholds on graded and deep-fade
  three-attempt sets move by ≤ 0.2 dB, inside the 0.5 dB search grid. Full workspace `--exclude
  pki-tooling --no-default-features`: **1552 passed, 0 failed**. clippy `-D warnings --all-targets` + fmt
  clean.
- **Why ship it anyway:** the LLR contract established in PRs #686/#687 says a soft demodulator emits true
  log-likelihood ratios. It did not. And P7 (IBDFE) derives its feedback reliability `v̄` as an
  expectation over the constellation *given the LLRs* — with 71×-over-confident LLRs that expectation
  drives the equalizer into an error-propagation spiral, which is precisely how IBDFE is known to fail.
- **Method note:** a 71× calibration error was invisible to every frame-success metric in the repo. The
  measurement that found it — empirical error rate versus the rate the LLR magnitude promises — is the
  only direct test of what an LLR *means*, and it is now a permanent gate.

## 2026-07-08 — fix(scfdma): acquire on the normalised correlation, not the unnormalised score

- **Requirement/change:** after PR #688 the SC-FDMA rungs still lost frames under fading in a way that
  scaled with Doppler. The research doc (item P2) attributed it to fade *dynamics* — the causal
  `smooth_ce` EMA lagging a moving channel.
- **Root cause — P2's premise was also wrong.** A **flat** Rayleigh fade (delay spread 0) at 60 dB SNR
  isolates dynamics from both noise and selectivity, and SCFDMA52-16QAM decoded only 0.47 of frames at
  0.5 Hz Doppler. Disabling `smooth_ce` entirely left that number **bit-identical**, so the EMA lag costs
  nothing. Tracing the failures: `demodulate_soft` was returning 784 LLRs instead of 4160 because
  `find_sync_offset` locked at offset **4896** — 17 symbols into the frame. `IqMatchedFilter::search`
  takes the argmax of the *unnormalised* correlation score, documented as favouring "high-correlation and
  high-energy alignment, so a deep-fade low-energy window cannot win". When the **preamble** is the faded
  part that is exactly backwards: measured ρ = 0.994 at the true offset (window energy 19.4) against
  ρ = 0.657 at +4896 (energy 83.0), and the higher-energy window won. SC-FDMA's preamble template is a
  full frame of SC-FDMA symbols carrying the same pilot comb as the data, so data-region windows correlate
  with it.
- **Design decision:** argmax over the **normalised** correlation ρ, which is amplitude-invariant and so
  unmoved by the fade. New `IqMatchedFilter::search_normalized(samples, bound, min_energy_frac)`;
  `search` is left alone for its existing callers. The energy floor (1 % of the mean window energy over
  the search range — admits a 20 dB preamble fade) is what keeps ρ meaningful: on a near-silent window
  both numerator and denominator vanish and ρ is numerical noise. No extra cost — the loop already
  computed both score and energy per offset.
- **Implementation:** `crates/openpulse-dsp/src/acquisition.rs` — `IqMatchedFilter::search_normalized`;
  `plugins/scfdma/src/demodulate.rs` — `find_sync_offset` + `MIN_WINDOW_ENERGY_FRAC`.
- **Tests:** `crates/openpulse-modem/tests/scfdma_multipath_timing.rs` —
  `acquires_a_frame_whose_preamble_is_faded`: flat Rayleigh fade at 60 dB SNR (no noise, no selectivity),
  so a lost frame can only be an acquisition bug. Fails on the pristine tree at 0.75 (SCFDMA52, 0.5 Hz).
  `scfdma52_rejects_noise_no_false_sync` still passes, so the ρ argmax has not weakened false-lock
  rejection.
- **Test results:** flat fade at 60 dB, 40 frames — SCFDMA52-16QAM 0.5 Hz **0.47 → 0.93**, 0.1 Hz
  0.77 → 0.98; SCFDMA52 0.5 Hz 0.75 → 1.00. `scfdma_ce_sweep` (60 frames/point): Watterson `good_f1` sum
  **29.57 → 31.40 of 42**; at 32 dB SCFDMA52-16QAM 0.72 → **0.97**, SCFDMA26-32QAM 0.97 → 1.00,
  52-8PSK 0.93 → 0.97, 52-64QAM 0.83 → 0.87. `moderate_f1` @32 dB: SCFDMA52 0.45 → 0.73, 16QAM 0.22 →
  0.45. The AWGN sweep is **bit-for-bit unchanged**. Full workspace `--exclude pki-tooling
  --no-default-features`: **1551 passed, 0 failed**. clippy `-D warnings --all-targets` + fmt clean.
- **Method note:** third accepted explanation falsified in a row, each time by removing the impairment the
  explanation depends on. Notch smearing dies at 60 dB SNR. CE-lag dies when `smooth_ce` is deleted and
  nothing changes. Delete the mechanism; if the number doesn't move, it was never the mechanism.

## 2026-07-08 — fix(scfdma): lock sync ahead of the correlation peak, not on it

- **Requirement/change:** after PR #685 the SC-FDMA rungs still decoded a *flat* 12–32 % of Watterson
  `good_f1` frames from 8 to 32 dB. The research doc attributed the residue to SC-FDE notch smearing
  (weak spot #4, item P7).
- **Root cause — the accepted explanation was wrong again.** A factorial experiment
  ({selective, flat} × {32 dB, 60 dB}, engine path, 60 frames) measured the selective-vs-flat frame-success
  gap at **0.50 at 32 dB and 0.51 at 60 dB**. Notch smearing is a noise-enhancement mechanism; it cannot
  survive the removal of noise. The real cause: `find_sync_offset` accepted `IqMatchedFilter::search`'s
  **argmax**, which on a two-ray channel sits on whichever ray is instantaneously stronger — the delayed
  one about half the time (matching the 0.50 gap). A late FFT-window start pulls samples of the next
  symbol in; the cyclic prefix only protects an **early** start, where the window merely begins inside
  the symbol's own prefix — a circular shift, i.e. the linear phase ramp `deramp_timing` already removes.
- **Design decision:** back the accepted offset off `SYNC_EARLY_BIAS = 8` samples. Budget is
  `CP − delay_spread` (32 − ~16); `DelayCe`'s basis is two-sided so its reach is unaffected; and both GPU
  paths call the same `find_sync_offset`, so they get it by construction. `ofdm::find_first_data_body`
  already solves this by scanning back for the earliest correlation tap above 0.20 × the peak — SC-FDMA
  was the outlier. That scan was tried and **rejected**: OFDM brackets it with a Schmidl–Cox coarse
  detection so its window always contains signal, whereas SC-FDMA searches from the front of a slice that
  may begin with silence, and a *normalised* correlation against a partially-silent window inflates. The
  earliest-tap rule then latches onto noise and broke the noiseless clean channel outright (BER 0.68).
- **Implementation:** `plugins/scfdma/src/demodulate.rs` — `find_sync_offset`.
- **Tests:** new `crates/openpulse-modem/tests/scfdma_multipath_timing.rs`.
  `decodes_a_stronger_delayed_ray_inside_the_cyclic_prefix` puts the **stronger** ray on the delay
  (0.5/1.0, −6 dB notch, d = 4 and 8) — the case the argmax gets wrong; `a_stronger_direct_ray_still_decodes`
  is the control that passed even with the bug, kept so a future change cannot fix one by breaking the
  other. This asymmetry is why a symmetric static two-ray test never caught it.
- **Test results:** the new gate decodes **0.00 of frames on the pristine tree** for every stronger-delayed-ray
  case (SCFDMA52 d=4/8, 8PSK, 16QAM d=4/8, 32QAM) and 0.95–1.00 after; the control passes both ways.
  `scfdma_ce_sweep` (60 frames/point, soft-concatenated FEC): Watterson `good_f1` sum **9.19 → 29.57 of
  42**, per rung at 32 dB — SCFDMA26-32QAM 0.12→0.97, 52-8PSK 0.32→0.93, 52-16QAM 0.27→0.72, 52-32QAM
  0.30→0.93, 52-64QAM-P4 0.32→0.87, 52-64QAM 0.28→0.83. The AWGN sweep is **bit-for-bit unchanged**, so
  the flat-channel floors in `profile.rs` are untouched. `scfdma_qam_modes_unsuitable_for_hf_watterson_profiles`
  still holds (uncoded hard demod: worst-scenario 16QAM 0.000 on `moderate_f1`). Full workspace
  `--exclude pki-tooling --no-default-features`: **1550 passed, 0 failed**. clippy `-D warnings
  --all-targets` + fmt clean.
- **Method note:** the tell was the *flatness*, again. `docs/dev/research/scfdma-improvements.md` now
  records the 60 dB cell as the one-run falsifier, and the revised P2 → #8 → P6 → P7 order.

## 2026-07-08 — fix(plugins): calibrate BPSK/QPSK/8PSK/64QAM soft LLRs

- **Requirement/change:** follow-up to PR #686, which found that only SC-FDMA and OFDM emit *calibrated*
  LLRs (magnitude ∝ 1/σ²). BPSK and QPSK emitted raw correlations/projections, 8PSK and 64QAM emitted
  max-log-MAP distance differences with `noise_var = 1.0`. Their `mean(|LLR|)` was flat in SNR (measured
  1.00× across 8→24 dB), so `combine_llrs_map` — which sums HARQ attempts — let a deeply-faded attempt
  vote exactly as loudly as a clean one.
- **Root cause of the *estimator* choice (measured, not assumed):** a demodulator's residual is not all
  thermal noise. Pulse-shaping ISI and equalizer misadjustment vary the symbol *amplitude* with no
  dependence on SNR, so both a moment estimator (M2/M4, tried) and a distance-to-nearest-point estimator
  (`estimate_decision_noise_var`, tried) stop tracking SNR: BPSK's coefficient of variation of |LLR| was
  0.443 at 8 dB **and 0.443 at 32 dB**. The component *orthogonal* to the hard decision is immune to
  radial variation, and for a differential detector the amplitude cancels exactly.
- **Design decision:** two new `openpulse-dsp::constellation` helpers.
  `differential_llr_scale(dots, crosses) = 2·mean|dot| / var(cross)` for BPSK — `cross = Im(z_k·conj(z_{k−1}))`
  is mean-zero, so `2A²/(2A²σ²) = 1/σ²` and the amplitude cancels. `psk_symbol_noise_var(symbols, bits)`
  for constant-modulus PSK — derotate by the hard decision, `Im(z·conj(ŝ))` carries only noise. 64QAM
  keeps `estimate_decision_noise_var` (its `normalize_to_constellation` already fixes the scale).
  All four are per-frame *uniform* scales, so single-frame decoding is bit-identical: soft Viterbi,
  min-sum LDPC and max-log turbo are scale-invariant. Only HARQ combining changes.
- **Implementation:** `crates/openpulse-dsp/src/constellation.rs` — `differential_llr_scale`,
  `psk_symbol_noise_var`, `estimate_decision_noise_var` doc; `plugins/{bpsk,qpsk,psk8,64qam}/src/demodulate.rs`.
- **Tests:** new `crates/openpulse-modem/tests/llr_calibration.rs` —
  `every_soft_demodulator_emits_llrs_that_grow_with_snr` (per-plugin floors on the 8→20 dB `mean|LLR|`
  growth) and `a_deeply_faded_extra_attempt_does_not_hurt` (adding a −14 dB attempt must not raise the
  BPSK250 threshold). **Both fail on the pristine tree**, by ×1.00 growth and a 9.0 dB penalty
  respectively. Two `openpulse-dsp` unit tests pin the estimators against amplitude jitter, which is the
  exact failure mode of the alternatives.
  `crates/openpulse-modem/tests/window_arq_watterson.rs` — its absent-symbol LLR injection was an
  absolute `0.6`, which happened to be 22 % of BPSK250's mean |LLR| (2.69) on that channel; with
  calibrated LLRs an absolute constant is silently negligible. Re-expressed as the same fraction and
  moved to the post-#686 `combine_llrs_map*` API. Verified the rewritten test passes with *both* the old
  and the new plugin, i.e. it is a faithful re-expression, not a relaxation.
- **Test results:** HARQ decode threshold, three attempts, mean over 5 seed triples on a 0.5 dB grid
  (before → after):

  | attempt SNRs | BPSK250 | QPSK250 | 8PSK500 | 64QAM1000 |
  |---|---|---|---|---|
  | `[0, 0, 0]` (equal) | −5.70 → −5.70 | 3.60 → 3.60 | 4.40 → 4.30 | 12.40 → 12.40 |
  | `[0, −4, −8]` (graded) | −0.10 → **−3.40** | 7.60 → **6.80** | 10.10 → **8.60** | 17.17 → **15.40** |
  | `[0, 0, −14]` (deep fade) | 4.90 → **−4.10** | 6.90 → **3.50** | 15.10 → **10.10** | 20.83 → **14.00** |

  Equal-SNR sets are unchanged (no regression); graded sets gain 0.8–3.3 dB; deep-fade sets gain
  3.4–9.0 dB. `mean(|LLR|)` growth over 8→24 dB, ideal ×39.8: BPSK **×36.3**, 64QAM ×16.3, 8PSK ×1.80,
  QPSK ×1.21 (the last two are limited by a receiver-internal residual, not by the estimator).
  Full workspace `--exclude pki-tooling --no-default-features`: **1548 passed, 0 failed**. clippy
  `-D warnings --all-targets` + fmt clean.

## 2026-07-08 — fix(engine): MAP LLR combining (the HARQ weight applied σ⁻² twice)

- **Requirement/change:** follow-up from PR #685. `receive_with_llr_combining` and
  `receive_with_window_arq` derived a per-attempt weight from a `noise_var = 1 / mean(|LLR|)` proxy and
  fed it to `combine_llrs_weighted` (weight `1/noise_var`). For a calibrated demodulator this applies
  **σ⁻⁴**: `openpulse_dsp::constellation::symbol_llrs` already divides every distance by its `noise_var`,
  so the emitted LLR is a true log-likelihood ratio and its magnitude is already ∝ 1/σ².
- **Root cause, measured:** `mean(|LLR|)` vs SNR, 8→24 dB (ideal for a calibrated plugin: 1×, 2.51×,
  6.31×, 15.8×, 39.8×). SC-FDMA: 1×, 2.51×, 6.36×, 16.05×, **40.42×** — exactly ∝1/σ², so the proxy
  weight is a second 1/σ². OFDM: 26.54× (also calibrated, sub-linear). 64QAM (`noise_var = 1.0`), 8PSK,
  BPSK, QPSK: **1.00× at every SNR** — flat, so the proxy conveys *no* reliability information there.
  The proxy was therefore harmful exactly where it "worked" and inert everywhere else.
- **Design decision:** for independent observations of the same bits, log-likelihood ratios **add**. The
  plain sum is the exact MAP combine, and for calibrated LLRs it *is* inverse-noise weighting. Add
  `combine_llrs_map` / `combine_llrs_map_in_ranges` and use them in the engine. `combine_llrs_weighted`
  is kept — it remains correct for LLRs that carry an arbitrary noise-blind scale — with its doc
  rewritten to state the contract (the weight is a *calibration correction*, `1` for a calibrated
  demodulator) and to point at `combine_llrs_map`. Calibrating the remaining plugins is left as a
  separate ~1 dB improvement, recorded in the research doc.
- **Implementation:** `crates/openpulse-core/src/fec.rs` — `combine_llrs_map`,
  `combine_llrs_map_in_ranges`, `combine_llrs_weighted` doc; `crates/openpulse-core/src/plugin.rs` —
  LLR *calibration* contract added to `demodulate_soft`; `crates/openpulse-modem/src/engine.rs` —
  both call sites, weight proxy deleted.
- **Tests:** `openpulse-core::fec::{map_combine_beats_double_inverse_noise_weighting_on_calibrated_llrs,
  weighted_combine_beats_map_sum_on_uncalibrated_llrs, map_combine_is_the_llr_sum}` — the first two pin
  *both* directions of the contract, so neither combiner can be misapplied again.
  `crates/openpulse-modem/tests/llr_combining_gain.rs` — new
  `llr_combining_extracts_diversity_gain_over_best_single_attempt`; the module doc's claim that the
  proxy and a pilot-derived σ² "give the same relative weighting" is corrected (true, and precisely the
  bug).
- **Test results:** engine threshold on a graded 0/−4/−8 dB three-attempt set, SCFDMA52, mean over 6 seed
  triples on a 0.5 dB grid: **4.83 → 4.08 dB (0.75 dB gain)**. Equal-SNR and single-deep-fade sets are
  unchanged within the grid step (a −14 dB attempt is nearly information-free, so over-suppressing it
  costs little). 64QAM (uncalibrated) unchanged except the deep-fade set, 20.83 → 20.42 dB. All existing
  HARQ/ARQ suites green (`llr_combining_gain`, `window_arq_*`, `scfdma_memory_arq`,
  `harq_retry_watterson_integration`, `harq_rate_selection_watterson`, `arq_retry_integration`,
  `two_way_arq`). Full workspace `--exclude pki-tooling --no-default-features`: **1544 passed, 0 failed**.
  clippy `-D warnings --all-targets` + fmt clean.

## 2026-07-08 — test(testmatrix): account for the SC-FDMA PAPR demonstrator modes

- **Requirement/change:** `every_registered_mode_is_covered_or_deferred` was red on `main` (verified by
  stashing): `SCFDMA52-LP` and `SCFDMA52-P2` are registered by `scfdma-plugin` but appear in no matrix
  mode list and no excused list. The test exists precisely to forbid that silent omission.
- **Design decision:** neither mode belongs in the sweep, and neither is a "known limitation" (both pass a
  clean round-trip). `SCFDMA52-LP` is a localized block-pilot demonstrator whose single-tap flat CE can
  *silently* mis-decode under frequency selectivity or a ±1-sample sync error — sweeping it over the
  matrix's propagation channels would assert behaviour the mode does not claim. `SCFDMA52-P2` is a
  PN-pilot low-PAPR variant of SCFDMA52 with identical geometry, rate and estimator. So: a new explicit
  `DEMONSTRATOR_MODES` list, chained into all three coverage tests and printed in the run summary
  alongside the other excused lists, rather than a fourth silent exclusion.
- **Implementation:** `apps/openpulse-testmatrix/src/cases.rs` — `DEMONSTRATOR_MODES` + the three
  coverage tests; `apps/openpulse-testmatrix/src/main.rs` — reported in the run summary.
- **Tests:** `cases::coverage_tests::{every_registered_mode_is_covered_or_deferred,
  deferred_and_known_limitation_modes_generate_no_cases, excused_modes_exist_in_registry}` — the last of
  these keeps the new list from rotting into naming a removed mode.
- **Test results:** `openpulse-testmatrix` 6/6 + 12/12 green (was 5/6). Quick matrix run: **555/555
  passed, 0 failed**, and the summary now lists the demonstrators. Full workspace
  (`--exclude pki-tooling --no-default-features`): **1540 passed, 0 failed**. clippy `-D warnings
  --all-targets` + fmt clean.

## 2026-07-08 — fix(scfdma): delay-basis Wiener channel estimator (DFT-CE was wrong on selective channels)

- **Requirement/change:** while building a before/after harness for research items P2/P3/P4
  (`docs/dev/research/scfdma-improvements.md`), the SC-FDMA demodulator was found to fail on a
  **noiseless, static, inside-the-cyclic-prefix two-ray channel** (`1 + a·z^-d`, d ≤ 8, CP = 32): hard BER
  floors of 0.20 (QPSK) / 0.26 (8PSK) / 0.36 (16QAM) at 90 dB SNR. Every SC-FDMA rung decoded only 2–7 %
  of Watterson `good_f1` frames, flat from 8 to 32 dB. The repo recorded that as "correct and by design".
- **Root cause:** `dft_ce_estimate` IDFT'd the 13 pilot-comb LS observations, kept the first `l_max = 9`
  taps, and re-evaluated. (1) Its delay grid is `N_FFT/(P·pilot_spacing) ≈ 3.94 samples` — the comb spans
  only the 65 occupied subcarriers, not all 256 FFT bins — so off-grid delays leak across every tap and
  truncation discards the leakage; `deramp_timing` re-centres the impulse response first, making the
  post-deramp delays essentially always off-grid. (2) Taps `l > P/2` are negative delays but were
  reconstructed as large positive ones. Measured channel-estimate MSE on a known two-ray response:
  −16.5 dB (d=1) / −14.3 dB (d=2) against −66/−71 dB for a physical delay basis.
- **Design decision:** replace with `channel::DelayCe` — `L ≤ 13` complex taps at fixed sample delays
  (spacing 5/3, symmetric about zero), evaluated at the true period `N_FFT`. Three deliberate choices:
  (a) **f64 construction** — adjacent-delay steering vectors are near-collinear over a 65-subcarrier
  aperture (`AᴴA` off-diagonals reach 0.98 of the diagonal) and the normal equations lose an f32 mantissa;
  (b) a **Wiener ridge with an exponential delay-power prior** (`ridge_j = σ²_h·Σw/(w_j·P_ch)`,
  `w_j = exp(−|τ_j|/1.5)`) — plain LS on that basis amplifies pilot noise and cost 4–6 dB of AWGN frame
  success, and a *flat* prior costs ~6 dB at reach ±10, while the exponential prior removes the cost so
  reach and AWGN stop trading; (c) a σ² **no channel estimate can bias** — the minimum of a comb
  out-of-window-tap estimator (guard-banded) and a CPE-removed adjacent-symbol pilot difference, which
  fail in opposite directions (delay spread vs Doppler/CFO). Folded in: research P3 (frame-mean σ²) and
  the remainder of P4 (both GPU paths now enter the shared `FrameFront::from_spectra`, so they can no
  longer skip `deramp_timing`; the GPU hard path gained the `/alpha_avg` de-bias).
- **Implementation:** `plugins/scfdma/src/channel.rs` — `DelayCe`, `CeSolver`, `delay_taps`,
  `pilot_comb_noise_var`, `pilot_diff_noise_var`, `ridge_pseudo_inverse`, `residual_debias`;
  `estimate_noise_var` now takes an explicit debias and serves only the localized layout.
  `plugins/scfdma/src/demodulate.rs` — new `FrameFront` two-pass front end shared by the hard, soft,
  constellation and both GPU paths; `SoftFrameMetrics.mean_pilot_noise_var` added.
- **Tests:** new `crates/openpulse-modem/tests/scfdma_ce_sweep.rs` (`#[ignore]` before/after harness:
  decode rate vs SNR for every SC-FDMA rung of `hpx_hf`, AWGN + Watterson good_f1).
  `plugins/scfdma/tests/llr_weighting_adaptation.rs` re-stated on invariants that hold for a *correct*
  receiver: `pilot_noise_variance_is_proportional_to_injected_noise_power` (σ̂² measured against the noise
  actually injected, not the nominal SNR — the harness's Box–Muller draws correlated uniforms from one
  LCG, so a realisation's power and spectrum drift a few percent), `decision_noise_variance_tracks_awgn_
  monotonically_without_over_reporting` (a nearest-point residual saturates and a Wiener CE's MSE is
  sub-linear in σ², so only the upper bound is a receiver property), and
  `soft_combining_beats_best_single_attempt_and_double_weighting_is_a_wash` (`symbol_llrs` already divides
  by σ̂², so the equal-weight sum *is* the MAP combine; the old `weighted ≥ equal` assertion passed only
  because the pre-Wiener per-symbol σ² left the LLR scale wrong enough for a second weighting to help).
  Stale "fails Watterson by design" commentary corrected in `snr_floor_calibration.rs` and
  `pilot_channel_estimation.rs` (their assertions still hold and still pass).
- **Test results:** `cargo test --workspace --exclude pki-tooling --no-default-features` → **1325 passed,
  0 failed** (one pre-existing failure, `openpulse-testmatrix::every_registered_mode_is_covered_or_deferred`,
  reproduces on a pristine tree). clippy `-D warnings --all-targets` clean; fmt clean. Sweep
  (60 frames/point, soft-concatenated FEC), old → new: static two-ray BER sum 10.4 → **1.90**; AWGN frame
  success sum 39.00 → **41.32** of 54 (SCFDMA52-8PSK 90 % floor 8→6 dB, 16QAM 10→8 dB, others unchanged);
  Watterson good_f1 sum 1.58 → **9.19** of 42 (dense rungs 0.03 → 0.27–0.32 at 32 dB).
- **Known follow-ups (not in this change):** `mmse_llr_noise_var` omits channel-estimate error, so LLRs are
  ~1.5 dB over-confident; and `combine_llrs_weighted` (plus the `1.0/mean_abs` proxy in
  `openpulse-modem/src/engine.rs`) applies σ⁻⁴ because `symbol_llrs` already carries 1/σ̂² — a pre-existing
  shipped defect that costs HARQ diversity gain when attempts differ in SNR.

## 2026-07-08 — feat: finer hpx_hf ladder (research #2, granularity)

- **Requirement/change:** fill the `hpx_hf` throughput cliffs and SNR dead-zones. Rewrote the SL2→SL11
  ladder into SL2→SL15 by inserting existing (previously unused) modes plus a MODCOD rung: **BPSK100**
  (SL4, breaks the 62→250 bps cliff), **QPSK250+Rs** (SL6, fills the 5→9 dB dead-zone), **SCFDMA26-32QAM**
  (SL10, ~1 kHz FDE-robust rung), **SCFDMA52-64QAM-P4** (SL14, splits the 17→22 dB gap).
- **Design decision:** floors placed monotonic between neighbours from the AWGN sweeps
  (`calibrate_ladder_gap_fillers` + `calibrate_snr_floors_hpx_hf`, lower bounds) with the low-order
  rungs' fading margin; ceilings = `floor(L+1)+2`. SL6/SL7 are QPSK250 coded/uncoded — a proper MODCOD
  pair, not a duplicate. The ACK-UP admission gate moved to the new top rung SL15. **Pre-release, so the
  SL re-index carries no ladder-interop concern** (the profile fingerprint changes intentionally).
- **Implementation:** `crates/openpulse-core/src/profile.rs` — `hpx_hf()` (modes, fec_modes, floors,
  ceilings, gate).
- **Tests:** `session_profile` (mode mapping + MODCOD-pair FEC), `cli_mode_advisor` (integration + the
  `recommend_hf_level` unit test), `adaptive_profile_integration` (7-ACK climb to 8PSK500), `cli_adaptive`
  (clean climb to SL8) all updated. **End-to-end:** a clean adaptive climb decodes every rung SL2→SL15
  (14/14 frames, including the inserted SCFDMA26-32QAM and SCFDMA52-64QAM-P4).
- **Test results:** openpulse-core + openpulse-cli + full openpulse-modem suites green; clippy
  `-D warnings` + fmt clean.

## 2026-07-08 — fix: recalibrate the LLR-combining-gain baseline test (restore green `main`)

- **Requirement/change:** `weighted_llr_combining_at_least_2_db_gain_over_equal_weight` was failing on
  `main` (weighted only 0.5 dB better than equal-weight, not the asserted ≥2 dB) — a pre-existing red
  baseline, not introduced by the P4 or ceiling PRs (verified by stashing).
- **Root cause (instrumented):** the ≥2 dB gate is aspirational. Weighted per-frame LLR combining now
  beats equal-weight *sample*-averaging by only ~0.5 dB, because the SC-FDMA soft demod matured
  (DFT-CE/MMSE/calibrated LLRs) so both methods decode the faded 3-frame set at nearly the same SNR.
  Independently verified that substituting the pilot-derived per-frame noise variance for the
  `1/mean(|LLR|)` weight proxy leaves the threshold **unchanged** (within one mode the two metrics give
  the same *relative* weighting and the combiner normalizes by the sum) — so the small gap is not a
  weighting deficiency to fix. That experimental metric-plumbing change was reverted as neutral/
  unvalidated (it added public trait API for no measured gain).
- **Design decision:** assert the robust invariant — weighted combining never costs SNR
  (`gain_db >= 0`) — instead of the unachievable ≥2 dB gap; rename to
  `weighted_llr_combining_not_worse_than_equal_weight`. Pre-release, so correcting an aspirational gate
  is appropriate. No production-code change.
- **Implementation:** `crates/openpulse-modem/tests/llr_combining_gain.rs` (doc + assertion + name).
- **Test results:** the test passes; **full openpulse-modem suite green (60 result groups, 0 failed)**.

## 2026-07-08 — improve: hpx_hf ceiling hysteresis normalization (research #2, agility)

- **Requirement/change:** `hpx_hf` upshift ceilings were inconsistent (+4 dB over the next rung's floor
  at SL2/3/9, +1 dB elsewhere), so the lowest-throughput rungs over-dwelt before climbing. From the
  Fable ladder review (`docs/dev/research/ladder-granularity.md`, agility #4).
- **Design decision:** normalize to a uniform `ceiling(L) = floor(L+1) + 2 dB`. Pure data change to
  `snr_ceilings`; the mode-advisor is floor-based (unaffected); **pre-release, so no ladder-interop
  concern**. Reachability preserved (ceiling > next floor); ceilings stay monotonic.
- **Implementation:** `crates/openpulse-core/src/profile.rs` — `hpx_hf` `snr_ceilings` (SL2 8→6, SL3
  9→7, SL5 12→13, SL6 13→14, SL7 15→16, SL8 17→18, SL9 21→19, SL10 23→24; SL4 unchanged).
- **Tests:** openpulse-core 7 lib + session_profile + rate suites green; openpulse-modem rate/OTA/
  adaptive suites green; clippy `-D warnings` clean.
- **Test results:** core + modem green. **Pre-existing unrelated failure noted:**
  `weighted_llr_combining_at_least_2_db_gain_over_equal_weight` (an SNR-threshold gate on SC-FDMA *soft*
  combining) fails on clean `main` independent of this change and of the P4 hard-path fix — flagged for
  separate investigation, not introduced here.

## 2026-07-08 — fix: SC-FDMA hard-demod MMSE amplitude bias (research #1 / P4)

- **Requirement/change:** SC-FDMA hard-demod QAM was biased toward the origin — the soft path divides
  equalized symbols by the MMSE attenuation `alpha_avg` to restore unit-constellation scale, but the
  hard path did not, so 16/32/64QAM outer-ring hard decisions were systematically wrong near threshold.
  Found by the Fable SC-FDMA review (`docs/dev/research/scfdma-improvements.md`, P4).
- **Design decision:** mirror the soft path in the hard demod — compute `alpha_avg` via
  `mmse_llr_noise_var` and divide before demap. PSK is angle-only (unaffected); QAM benefits. RX-only,
  no wire change; pre-release so no interop concern regardless.
- **Implementation:** `plugins/scfdma/src/demodulate.rs` hard-demod path (also `.max(1e-6)` on the
  noise var to match the soft path).
- **Tests:** new `scfdma52_16qam_hard_demod_no_amplitude_bias` (20 dB AWGN, 3 seeds); the full
  scfdma suite still green.
- **Test results:** scfdma-plugin **58 + 12 loopback + others all pass**; clippy `-D warnings` + fmt clean.

## 2026-07-07 — feature: panel Noise transport wiring (REQ-SEC-CTL-01/02, CAP-68 slice 4-panel)

- **Requirement/change:** complete the client half — the panel's TCP transport performs the PSK Noise
  handshake + encrypted framing, so the operator↔daemon control link works end to end with auth on.
- **Design decision:** the panel's `try_recv` is a **non-blocking poll** (50 ms read timeout),
  incompatible with `SyncNoise`'s blocking `read_exact` framing. So the panel keeps its own **resumable
  partial-frame reader**: `connect()` does a bounded blocking initiator handshake over the raw stream,
  then switches to the 50 ms poll; `try_recv_noise` accumulates ciphertext across polls and a pure
  `take_frame` helper assembles a complete `u32`-length-framed message, which is decrypted and demuxed
  (`OPSP` magic → binary, else text). Sends encrypt one message each. Activated by `OPENPULSE_CONTROL_PSK`
  (same env as the daemon); WebSocket stays plaintext (matches the daemon's WS).
- **Implementation:** `apps/openpulse-panel/src/transport.rs` — `NoiseCtx`, `control_psk_from_env`,
  `framed_write`/`framed_read_blocking`, `take_frame`, `demux_message`, the `connect` handshake and
  `try_recv_noise`; `openpulse-linksec` dep.
- **Tests:** transport unit tests — `take_frame_assembles_across_partial_reads` (prefix + body split
  across polls, plus two back-to-back frames), `take_frame_rejects_oversized_length`,
  `demux_classifies_opsp_and_text`. (The panel is a binary crate — no integration-test target — so the
  end-to-end path is covered by the daemon-side `control_auth` integration test, the linksec real-TCP
  tests, and live validation.)
- **Test results:** panel **8/8** (5 theme + 3 transport); clippy `-D warnings` + fmt clean; workspace
  check green.
- **End-to-end:** with `OPENPULSE_CONTROL_PSK` set on both sides + `require_auth` (or a non-loopback
  bind), the panel and daemon now speak the encrypted Noise channel over TCP. WebSocket + keystore-backed
  PSK loading remain as follow-ups.

## 2026-07-07 — feature: daemon control-channel Noise wiring + fail-closed gate (REQ-SEC-CTL-01/02, CAP-68 slice 4-daemon)

- **Requirement/change:** enforce PSK auth + encryption on the daemon's **TCP** control channel, and
  fail closed on a non-loopback bind. The security-critical server half of slice 4.
- **Design decision:** mode-aware `ClientWriter`/`ClientReader` in `handle_client` — plaintext
  (loopback default, unchanged) or an `AsyncNoise` responder (authenticated); a failed/absent handshake
  drops the connection before any command runs. The gate is `openpulse_linksec::auth_required(bind,
  require_auth)`; when auth is required but no PSK is provided the daemon **refuses to start**. PSK
  currently from `OPENPULSE_CONTROL_PSK` (64 hex) — keystore-backed loading is the follow-up. `ws.rs`
  is independent (it uses `dispatch_command`, not `handle_command`), so it was untouched — **WebSocket
  stays plaintext for now** (distinct framing; TCP is the default/primary path).
- **Implementation:** `lib.rs` `ClientWriter`/`ClientReader` + `handle_client` handshake/split + all
  writes via `send_json`/`write_frame` + `ControlServerConfig.control_psk`; `server.rs` gate +
  `load_control_psk`; `[control_security]` config comment.
- **Tests:** `crates/openpulse-daemon/tests/control_auth.rs` — a **real `AsyncNoise` client ↔ real
  `ControlServer`** over TCP: `noise_client_exchanges_encrypted_messages` (encrypted round-trip,
  decrypt-command + encrypt-response) and `wrong_psk_client_is_dropped` (fail closed).
- **Test results:** daemon **44 lib + 2 auth + 13 control_port + 5 spectrum** (+ others) all green — the
  plaintext path is unbroken; clippy `-D warnings` + fmt clean.
- **Behavior change (intended):** a non-loopback bind **without** a PSK now refuses to start (was:
  silent plaintext) — the fail-closed guarantee of REQ-SEC-CTL-02.
- **Remaining:** panel `transport.rs` (its non-blocking poll needs a resumable framed-Noise reader),
  WebSocket, and keystore-backed PSK loading — the panel/GUI path can't be runtime-validated here.

## 2026-07-07 — feature: sync + async Noise socket channels (REQ-SEC-CTL-01/02, CAP-68 slice 4-transport)

- **Requirement/change:** turn the byte-buffer Noise core into actual socket channels the daemon
  (async) and panel (sync) can use, and prove them over real sockets. Continues slice 4.
- **Design decision:** two thin adapters over the same `NoiseHandshake`/`NoiseTransport` core, sharing a
  `u32`-BE-length-prefixed, message-oriented wire framing (fits the control protocol's NDJSON lines +
  binary spectrum frames): `sync_channel::SyncNoise` (blocking `Read`+`Write`, for the std panel/CLI)
  and `async_channel::AsyncNoise` (tokio `AsyncRead`+`AsyncWrite`, behind a non-default `tokio` feature
  so CI `--no-default-features` stays lean). `AsyncNoise::into_split` yields concurrently-usable
  write/read halves sharing the transport via a brief per-message `Mutex` (Noise send/recv nonces are
  independent) — matching the daemon's `select!` loop that writes events while reading commands.
- **Implementation:** `crates/openpulse-linksec/src/sync_channel.rs`, `.../async_channel.rs`; `tokio`
  optional dep + feature; `MAX_FRAME`, `LinkSecError::Io`/`FrameTooLarge`.
- **Tests:** over **real TCP sockets** (127.0.0.1:0) — sync: handshake + round-trip, wrong-PSK rejected
  (server fails closed), frame helpers; async: same + `split_halves_round_trip`.
- **Test results:** openpulse-linksec **8/8** (`--no-default-features`), **11/11** (`--features tokio`);
  clippy `-D warnings` both feature states + fmt clean; workspace check green.
- **Remaining:** wire these into the daemon `handle_client`/`handle_command` connection loop (a wide,
  `ws.rs`-mirrored, concurrency-sensitive refactor) + the panel `transport.rs`, load the PSK from the
  keystore, and enforce fail-closed drop on a failed handshake — done with a live daemon+panel, since
  the GUI/live path can't be runtime-validated here. WebSocket is a distinct framing (TCP is the
  default/primary).

## 2026-07-07 — feature: control-channel PSK link-security core (REQ-SEC-CTL-01/02, CAP-68 slice 4-core)

- **Requirement/change:** the control channel needs PSK mutual auth + on-wire encryption, mandatory on
  a non-loopback bind. Fourth slice of the control-channel security plan (backlog item 11).
- **Design decision — pivot:** the confirmed plan was TLS-PSK-via-rustls, but **rustls has no
  external/raw TLS-PSK support** (it is certificate-focused), and OpenSSL (K4remote's route) would add a
  C dependency to an otherwise pure-Rust workspace. Pivoted to the **Noise protocol**
  (`Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s` via `snow`): both endpoints prove knowledge of the 32-byte
  PSK during the handshake (mutual auth) and then exchange ChaCha20-Poly1305 messages with forward
  secrecy — no certificates, no C dep. Implemented **transport-agnostically** (byte-buffer
  `NoiseHandshake`/`NoiseTransport`) so the same primitive serves the sync CLI and the async
  daemon/panel. The `auth_required(bind, require_auth)` gate encodes REQ-SEC-CTL-02 (mandatory on
  non-loopback). Config `[control_security]` (`require_auth`, `psk_key_id`) added.
- **Implementation:** new crate `crates/openpulse-linksec` (`NoiseHandshake`, `NoiseTransport`,
  `is_loopback_bind`, `auth_required`, `LinkSecError`); `ControlSecurityConfig` + `[control_security]`
  template in `openpulse-config`; design doc updated with the pivot rationale.
- **Tests:** `matching_psk_handshakes_and_round_trips` (bidirectional), `mismatched_psk_fails_handshake`,
  `tampered_ciphertext_is_rejected`, `oversized_plaintext_is_rejected`, `loopback_detection_and_gate`
  (IPv4/IPv6/`localhost`/`[::1]:port` + the gate policy).
- **Test results:** openpulse-linksec **5/5**; openpulse-config still 15/15; clippy `-D warnings` + fmt
  clean; workspace check green. **Remaining:** wire the handshake+framing into the daemon TCP/WS server
  + panel client and enforce fail-closed PTT — a separate, live-validated change (the wire is still
  plaintext until then, safe on the default loopback bind).

## 2026-07-06 — feature: OS keychain secret-store backend (REQ-SEC-CTL-03, CAP-68 slice 3)

- **Requirement/change:** prefer the OS secret store for the control-channel PSK + identity material
  when available, with the file keystore as fallback. Third slice of the control-channel security plan.
- **Design decision:** a `SecretStore` trait (`get`/`set`/`delete`) with two backends: `FileStore`
  (wraps the slice-2 `FileKeystore`, re-saves on each mutation) and `KeychainStore` (`keyring` 3;
  `sync-secret-service` + `crypto-rust` on Linux, `apple-native`/`windows-native` elsewhere). `keyring`
  is an **optional** dep behind a default-on `keychain` feature, so CI's `--no-default-features` keeps
  the D-Bus/secret-service dep out of headless workspace builds. `KeychainStore::available()` probes
  reachability so a caller can fall back to a file store on a headless host.
- **Implementation:** `crates/openpulse-keystore/src/store.rs` (`SecretStore`, `FileStore`,
  `KeychainStore`); `keyring` target deps + `keychain` feature; `KeystoreError::Keychain`.
- **Tests:** `file_store_get_set_delete_round_trip` (persists through the master password);
  `keychain_round_trip` is `#[ignore]` (needs a live secret service).
- **Test results:** openpulse-keystore **5/5** (`--no-default-features`); builds + clippy `-D warnings`
  clean with `--features keychain` too; fmt clean. *Rot note:* the keychain path isn't exercised by
  CI's `--no-default-features` build (feature off) — a `--features keychain` build/clippy job would
  prevent rot (cf. the `gpu` feature).

## 2026-07-06 — feature: master-password file keystore (REQ-SEC-CTL-04, CAP-68 slice 2)

- **Requirement/change:** secrets (the control-channel PSK, and eventually identity material) need
  at-rest encryption on hosts without a usable system secret store. Second slice of the
  control-channel security plan (backlog item 11).
- **Design decision:** a **new crate `openpulse-keystore`** so the crypto deps stay out of the
  headless TNC binaries (only the daemon/panel/cli will depend on it). `FileKeystore` encrypts a JSON
  map of `key-id → secret` under a master password: **Argon2id** KDF (default params) →
  **ChaCha20-Poly1305** AEAD; fresh random salt + nonce per save; the master is held in memory only
  and never persisted. The file is owner-only (reuses `secret_file` from slice 1). Layout:
  `OPKS | v1 | salt(16) | nonce(12) | ciphertext(+16-byte tag)`. Confirmed primitives (TLS-PSK;
  Argon2id + ChaCha20-Poly1305).
- **Implementation:** `crates/openpulse-keystore` (`FileKeystore` create/open/get/set/remove/save,
  `KeystoreError`); workspace member + `argon2`/`chacha20poly1305` deps.
- **Tests:** `round_trip_with_correct_master`, `wrong_master_is_rejected` (AEAD auth failure →
  `Decrypt`), `tampered_ciphertext_is_rejected`, `saved_file_is_owner_only`.
- **Test results:** openpulse-keystore **4/4**; clippy `-D warnings` + fmt clean.

## 2026-07-06 — feature: shared owner-only secret-file permission checks (REQ-SEC-CTL-05, CAP-68 slice 1)

- **Requirement/change:** secret files weren't uniformly permission-checked on load — the daemon's
  identity-key read path accepted a group/world-readable key. First slice of the control-channel
  security plan (backlog item 11); the only REQ-SEC-CTL slice not blocked on the auth-scheme decision.
- **Design decision:** lift the owner-only `0600`/`0700` validate+enforce logic (previously inline in
  `openpulse-cli`'s trust store) into a shared `openpulse_config::secret_file` module used by **both
  server and clients**; validate-on-load refuses group/world-accessible secret files, enforce-on-write
  sets `0600`. New `ConfigError::InsecureSecretPermissions`. Non-Unix = documented no-op.
- **Implementation:** `crates/openpulse-config/src/secret_file.rs` (`validate_owner_only`,
  `enforce_owner_only`); wired into `load_identity_from`'s read path — covers the **daemon (server)**
  via `load_or_generate_identity` (`server.rs:444`) and the CLI; `openpulse-cli`'s
  `validate_trust_store_permissions` / `enforce_trust_store_permissions` now **delegate** to the shared
  helper (removes the duplicated cfg-gated logic).
- **Tests:** `secret_file` unit tests (accepts `0600`, rejects `0640`/`0604`; enforce sets `0600`);
  `load_identity_refuses_group_readable_key`; existing `trust_store_persist_enforces_owner_only_mode`
  still passes.
- **Test results:** openpulse-config **15/15**, openpulse-cli suites green; clippy `-D warnings` + fmt
  clean; `cargo check --workspace --exclude pki-tooling --no-default-features` green.

## 2026-07-05 — reassess: hpx_hf SL7 (11→16 dB) gap-filler — keep 8PSK500+RS; fix stale mode-advisor test

- **Requirement/change:** revisit (with a Fable second opinion) the SL7 rung that fills the 11→16 dB
  throughput gap in `hpx_hf`. A prior finding (a mode needing ~6 retries in linksim) had motivated the
  FEC-protected upper ladder + floor recalibration, which opened the gap; `8PSK500+RS @ 12 dB` was
  chosen to fill it. The `cli_mode_advisor` integration test was also failing on `main`.
- **Design decision:** measured `8PSK500+RS` vs the cycle-slip-immune, pilot-aided `PILOT-8PSK500`
  across AWGN + Watterson fading (new `calibrate_pilot_gap_candidate` sweep). Result: pilot wins on
  AWGN (7 vs 9 dB) but **loses good_f1 fading** — it fails where 8PSK500+RS decodes at 7 dB; both fail
  moderate_f1, as does the whole single-carrier segment (QPSK250/500), which the ladder downshifts past
  by design. So **keep 8PSK500+RS @ 12.0 dB** (no profile change). Separately, the `cli_mode_advisor`
  cases `(12.0→SL6)` and `(15.0→SL7)` were **stale** against the recalibrated floors (SL7=12, SL8=14):
  the advisor correctly returns SL7/8PSK500 at 12.0 and SL8/SCFDMA52-8PSK at 14.0.
- **Implementation:** no profile logic change; `profile.rs` comment records the pilot eval+rejection;
  `tests/snr_floor_calibration.rs` gains `PilotPlugin` registration + the `calibrate_pilot_gap_candidate`
  sweep; `cli_mode_advisor.rs` cases corrected and extended into SL8/SL9.
- **Tests:** `cli_mode_advisor` 3/3 pass (verified against live advisor output at 11.5/12/14/16 dB). Manual
  sweeps: hpx_hf AWGN (SL7 meas 9), Watterson baseline (8PSK500+RS gF1 7, mF1 fail), gap candidate
  (8PSK500+RS 9/7/fail vs PILOT-8PSK500+RS 7/fail/fail). **Firmed up at 60 frames** via a good_f1
  point-rate probe: 8PSK500+RS decodes **62/70/70 %** at 10/14/18 dB vs PILOT-8PSK500+RS **28/32/32 %** —
  both plateau above ~14 dB (irreducible fading outage), and the pilot never reaches the 50 % majority
  threshold, so its swap is decisively rejected.
- **Test results:** `cli_mode_advisor` green; fmt clean; openpulse-core builds (comment-only). The four
  `#[ignore]` sweeps re-run on demand.

## 2026-07-05 — feature: `openpulse audit-bundle` command (REQ-OBS-03, CAP-67 slice 4)

- **Requirement/change:** the audit artifacts (`events.ndjson`, `snapshot.json`, rolled logs) were
  produced but had to be collected by hand for handoff. Final slice of the observability/audit-mode
  plan (backlog item 10); generalises the RF-test-only `onair-bundle-evidence.sh` to everyday runs.
- **Design decision:** a new CLI subcommand packages everything into a single `.tar.gz` (via `tar` +
  `flate2`) with a `metadata.json` manifest (schema, timestamp, version, file list + sizes). The
  packaging core `create_bundle(BundleSpec, dest)` takes injected version/timestamp so it is pure and
  round-trip-testable; the `run` wrapper resolves the archive dir + rolled log files from config
  (`[observability] archive_dir`, `[logging] file`), both overridable by flags. Handled as an
  early-return command (no engine/audio backend needed).
- **Implementation:** `crates/openpulse-cli/src/commands/audit_bundle.rs` (`BundleSpec`,
  `create_bundle`, `collect_log_files`, `run`); `AuditBundle` variant in `cli.rs` + early dispatch in
  `main.rs`; `flate2`/`tar` added to the CLI (+ `tar` to workspace deps).
- **Tests:** `bundle_contains_archive_files_and_metadata` — builds a bundle from a temp archive, then
  **gz-decodes the tar** and asserts `metadata.json` + `events.ndjson` + `snapshot.json` entries and
  the `audit-bundle-<ts>-<label>.tar.gz` name.
- **Test results:** `cargo test -p openpulse-cli --no-default-features audit_bundle` → **1 passed**;
  full CLI suite otherwise green (30/30 unit + others); clippy `-D warnings` + fmt clean; workspace
  check green. (Pre-existing, unrelated: `cli_mode_advisor` fails on `main` — hpx_hf@12 dB yields SL7,
  the test expects SL6 — a profile/test drift, not touched here.)

## 2026-07-05 — feature: audit-mode startup snapshot.json (REQ-OBS-01, CAP-67 slice 3)

- **Requirement/change:** the captured event stream had no anchoring context — which config, version,
  and host produced it. Third slice of the observability/audit-mode plan (backlog item 10).
- **Design decision:** on daemon startup in audit mode, write `<archive_dir>/snapshot.json` with
  version/build/runtime metadata (version, git SHA via `OPENPULSE_GIT_SHA` build env, OS/arch, capture
  time) plus the running config **with secret string values redacted**. Redaction walks the serialized
  config JSON and blanks values under secret-looking keys (`*_key`, `secret`, `password`, `passphrase`,
  `token`, `seed`) while preserving public identifiers like `key_id`/`pubkey`. Metadata is injected
  into `build_snapshot` so the builder is pure/testable; the write wrapper is best-effort (warns, never
  fatal).
- **Implementation:** `crates/openpulse-daemon/src/audit.rs` — `is_secret_key`, `redact_secrets`,
  `build_snapshot`, `write_startup_snapshot`; wired in `server::run` (audit-mode block, before the
  event recorder).
- **Tests:** `redact_blanks_secret_keys_but_keeps_identifiers` (redacts `signing_key`/`api_key`/nested
  `password`/`seed`, keeps `key_id`/`pubkey`/`port`); `snapshot_has_metadata_and_config` (schema +
  version + git SHA + config section).
- **Test results:** `cargo test -p openpulse-daemon --no-default-features audit` → **4 passed / 0
  failed**; clippy `-D warnings` + fmt clean.

## 2026-07-05 — feature: audit-mode control-event capture (REQ-OBS-01, CAP-67 slice 2)

- **Requirement/change:** the daemon's `ControlEvent` stream (engine events, metrics, PTT/RF/QSY/OTA
  state) was only observable by a live connected client — nothing recorded it, so a run couldn't be
  replayed for analysis. Second slice of the observability/audit-mode plan (backlog item 10).
- **Design decision:** add an opt-in `[observability]` config section (`audit_mode`, `archive_dir`)
  and, when on, spawn a recorder task that **subscribes to the same `broadcast` channel clients use**
  (`handle.event_tx.subscribe()`) and appends each event as NDJSON to `<archive_dir>/events.ndjson`.
  Tapping the existing broadcast means no new event plumbing and no live client required. Open-failure
  is non-fatal (warn + disable); each line is flushed so an abrupt exit keeps prior events; a lagged
  receiver logs the skip count rather than crashing. `~` in `archive_dir` reuses
  `openpulse_config::logging::expand_tilde`.
- **Implementation:** `crates/openpulse-daemon/src/audit.rs` (`EventRecorder::open`/`record`,
  `spawn_event_recorder`); `pub mod audit` in `lib.rs`; wired in `server::run` right after the control
  ports bind; `ObservabilityConfig` + `observability` field in `openpulse-config`; `[observability]`
  documented in both config templates.
- **Tests:** `openpulse-daemon` unit tests — `recorder_writes_one_json_line_per_event` (one valid-JSON
  line per event) and `recorder_appends_across_reopens` (reopen appends, does not truncate).
- **Test results:** `cargo test -p openpulse-daemon --no-default-features audit` → **2 passed / 0
  failed**; `openpulse-config` 12/12; `clippy -D warnings` + fmt clean on both crates.

## 2026-07-05 — feature: persistent rolling file logging (REQ-OBS-02, CAP-67 slice 1)

- **Requirement/change:** users had no way to persist logs to disk — `tracing` went to stdout only,
  so a run's logs vanished on restart and couldn't be handed to a developer. First slice of the
  REQ-OBS observability/audit-mode plan (backlog item 10).
- **Design decision:** put the log-init helper in `openpulse-config` (owner of `LoggingConfig`,
  depended on by every binary) instead of duplicating subscriber setup per binary. Opt-in via
  `[logging] file`; a daily-rolled, non-blocking (`tracing-appender`) file layer is composed
  alongside the existing stdout layer; level precedence unchanged (`RUST_LOG` > `cfg.level`). The
  non-blocking appender returns a `WorkerGuard` the caller must hold — flagged `#[must_use]` so a
  dropped guard (which would lose buffered lines) is a compile-time warning. Wired the daemon first
  (the long-running process); config load moved before tracing init so the path is config-driven.
- **Implementation:** `crates/openpulse-config/src/logging.rs` (`expand_tilde`, `file_writer`,
  `init_tracing`); `LoggingConfig.file: Option<String>` (default None) in `lib.rs`; daemon `main.rs`
  loads config then calls `init_tracing` and binds the guard; `tracing`/`tracing-subscriber`/
  `tracing-appender` added to the config crate (+ `tracing-appender` to workspace deps); `[logging]
  file` documented in both config templates.
- **Tests:** `openpulse-config` unit tests — `logging_config_file_defaults_to_none`,
  `expand_tilde_expands_home_and_passes_through`, `file_writer_creates_dir_and_writes_a_line`
  (writes a marker through a scoped subscriber, drops the guard, asserts the rolled file has it).
- **Test results:** `cargo test -p openpulse-config --no-default-features` → **12 passed / 0 failed**.
  Daemon builds; `clippy -D warnings` + fmt clean on openpulse-config + openpulse-daemon;
  `cargo check --workspace --exclude pki-tooling --no-default-features` green.

## 2026-07-01 — fix: 2D-Gray remap of cross-32QAM constellation (CAP-20, fixes SL10>SL11 inversion)

- **Requirement/change:** the calibration sweeps found SCFDMA52-32QAM (SL10) AWGN-measured *harder*
  (17 dB) than the denser SCFDMA52-64QAM (SL11, 15 dB) — physically backwards, dominating SL10. Root
  cause: `qam32()` applied a **1D Gray code over a 2D raster** (`QAM32_SPATIAL[gray5_to_natural(label)]`),
  which is not a 2D-Gray mapping — Euclidean-adjacent points differed by ~2 bits vs 64QAM's clean
  product-Gray (~1). Pre-release, so remapped in place (user's call — no ladder diversity before release).
- **Design decision:** replace with a **direct label→point table** (`QAM32_BY_LABEL`) optimised by
  simulated annealing (`tests/qam32_gray_optimizer.rs`) to minimise total Hamming distance between
  Euclidean-nearest-neighbour points: **avg 2.04 → 1.36 bits/neighbour** (near-optimal for a
  non-rectangular cross constellation; bit-4 cleanly splits the I half-planes). Dropped the
  `gray5_to_natural`/`natural5_to_gray` indirection for QAM32 (map/demod index the table directly). The
  soft demod follows automatically via `constellation_points`.
- **Implementation:** `QAM32_BY_LABEL` + `qam32`/`qam32_demod` rewrite in
  `crates/openpulse-dsp/src/constellation.rs`; SL10 floor 20→17 in `crates/openpulse-core/src/profile.rs`
  (32QAM is now honestly, not forcibly, below SL11); optimizer tool + a low-Hamming regression test.
- **Tests:** `qam32_nearest_neighbours_are_low_hamming` (locks avg < 1.6); existing round-trip
  (`hard_demap_round_trips_all_constellations`) + bijection (`all_points_distinct`) still pass; scfdma
  plugin suite green. Calibration re-run: **SCFDMA52-32QAM 17 → 9 dB AWGN** — now more robust than
  64QAM, inversion fixed. Clippy `-D warnings` + fmt clean, workspace builds.
- **Test results:** dsp/scfdma/core suites green; 32QAM AWGN floor 17→9 dB (8 dB gain); ladder now
  physically monotonic (SL9 16QAM < SL10 32QAM < SL11 64QAM) instead of forced.

---

## 2026-07-01 — feature: handshake ladder-compatibility guard (CAP-01/CAP-33, backward compat inc. 2)

- **Requirement/change:** increment 2 of the freeze+version+handshake-guard strategy — detect a
  diverged OTA rate ladder on air and fall back to fixed mode instead of silently desyncing on
  `recommended_level`.
- **Design decision:** the signed `ConReq`/`ConAck` now advertise `(profile_name, profile_fingerprint)`
  in the **signature-covered** body, using the existing `skip_serializing_if` pattern so an
  un-advertised frame (name "", fp 0) produces byte-identical canonical JSON → **full signature
  compatibility with legacy/pre-guard peers**. Threaded via new `create_full` constructors; the old
  `create`/`create_with_grid` delegate with empty profile (zero caller churn). On handshake completion
  the daemon compares the peer's fingerprint to `local_ota_ladder` and stores
  `VerifiedPeer.profile_compatible`; a **positive mismatch** (both advertised, differ) flips
  `ota_suppressed_by_peer()` which gates BOTH the OTA send and OTA decode branches → fixed-mode
  fallback. Undetermined (either side un-advertised) or matching → OTA unaffected, so
  OTA-without-handshake keeps working.
- **Implementation:** `profile_name`/`profile_fingerprint` on `ConReq`/`ConReqBody`/`ConAck`/`ConAckBody`
  + `create_full` + `canonical_bytes` in `crates/openpulse-core/src/handshake.rs`; `VerifiedPeer.profile_compatible`,
  `RuntimeControlState.local_ota_ladder` + `ota_suppressed_by_peer()`, `record_verified_peer` compat
  check, and `create_full` at the connect/accept sites in `crates/openpulse-daemon/src/lib.rs`;
  `local_ota_ladder` set at OTA start + both OTA branches gated in `crates/openpulse-daemon/src/server.rs`.
- **Tests:** `handshake_integration` — profile round-trips wire, tamper of the fingerprint fails the
  signature, un-advertised frame signs identically to legacy (3 new; 20 total). Daemon
  `ladder_fingerprint_mismatch_suppresses_ota` — match→compatible, differ→suppressed, un-advertised /
  no-local→undetermined. Core all green, daemon all green, clippy `-D warnings` + fmt clean, workspace builds.
- **Test results:** handshake 20/20, daemon guard test pass + suites green; no regression to the signed
  handshake (all pq/compression/handshake tests still pass) — un-advertised frames remain byte-identical.

---

## 2026-07-01 — feature: rate-ladder fingerprint + freeze-versioning discipline (CAP-37, backward compat)

- **Requirement/change:** the OTA ACK carries a bare `recommended_level`; its meaning depends on both
  stations running the same `SessionProfile` mapping. Any mode/FEC/step change (e.g. #611 adding FEC
  to hpx_hf) silently breaks interop across code versions. Establish the "freeze + version + handshake
  guard" strategy the user chose.
- **Design decision:** (increment 1 of 2) add a **ladder fingerprint** — `SessionProfile::fingerprint()`,
  an FNV-1a hash over ONLY the wire-relevant `(level → mode, level → FEC)` mapping. Local policy (SNR
  floors/ceilings, nack_threshold) is excluded by construction, so a floor recalibration does NOT break
  compatibility — only a mode/FEC/step change does. Adopt the **freeze discipline** (a published ladder
  is a wire contract; changes ship as a new named profile, never mutate in place) documented in
  `docs/dev/design/ladder-versioning.md`. Consumed now via observability: the daemon logs its active
  `ladder_fingerprint` at OTA startup so operators can diff it across stations. **Increment 2 (next):**
  advertise `(profile_name, fingerprint)` in the signed handshake and disable OTA (fixed-mode fallback)
  on a positive mismatch — detection instead of silent desync, while OTA-without-handshake keeps working.
- **Implementation:** `SessionProfile::fingerprint()` in `crates/openpulse-core/src/profile.rs`;
  `ladder_fingerprint` log at OTA start in `crates/openpulse-daemon/src/server.rs`; design doc
  `docs/dev/design/ladder-versioning.md`.
- **Tests:** `crates/openpulse-core/tests/session_profile.rs` — fingerprint deterministic + distinguishes
  profiles + tracks mode/FEC mapping (2 new; 29 total pass); clippy `-D warnings` + fmt clean; daemon builds.
- **Test results:** session_profile 29/29 (2 new fingerprint tests), daemon builds, gates clean.

---

## 2026-07-01 — fix: hpx_hf FEC-protected upper ladder + calibrated floors (CAP-37)

- **Requirement/change:** raising SL7's floor to 18 dB (prior entry) exposed a duplicate (SL7=SL9=18)
  and a gap (nothing usable 11→16 dB). Root cause: **`hpx_hf` assigned no per-level FEC at all**
  (`fec_modes: [None; 21]`) — the OTA ladder ran raw, so 8PSK500 needed 18 dB and the dense SCFDMA
  rungs were effectively unusable. Do the two follow-ups (calibrate the SCFDMA rungs; give 8PSK500 a
  FEC variant) and rebuild the upper ladder coherently.
- **Design decision:** assign per-level FEC and recalibrate floors from the AWGN sweep with the FEC in
  place. **SL7 8PSK500 → light RS** (measured 9 dB; net ~1312 bps stays *above* QPSK500's 1000 bps, so
  it remains a faster rung — chosen over SoftConcatenated which measured 6 dB but would make it
  net-slower than QPSK500 and misplace it). **SL8–11 SCFDMA → SoftConcatenated** (dense modes only run
  FEC-protected; measured 8PSK 9 / 16QAM 12 / 32QAM 17 / 64QAM 15 dB). Floors set monotonic with ~2–3
  dB fading margin (SL7 12, SL8 14, SL9 16, SL10 20, SL11 22) → kills the 18 dB duplicate, fills the
  11→16 gap (8PSK500 now a real 12 dB rung), keeps the ladder monotonic; ceilings lowered so each new
  rung is reachable on the cautious upshift. **Findings surfaced by the sweep, documented in-code:**
  (1) cross-32QAM (SL10) AWGN-measures *harder* than 64QAM (SL11) — a soft-demod weakness; floors
  forced monotonic since 64QAM is denser and needs more on fading. (2) With FEC, the wideband SCFDMA
  rungs decode below the narrowband PSK rungs on AWGN (bandwidth advantage) — expected; `level_for_snr`
  picks the highest adequate rung so dominated rungs are simply never selected.
- **Implementation:** `fec_modes` + recalibrated `snr_floors`/`snr_ceilings` in
  `crates/openpulse-core/src/profile.rs::hpx_hf`; calibration harness extended to sweep the FEC rungs
  (SCFDMA plugin registered, per-rung FEC) in `crates/openpulse-modem/tests/snr_floor_calibration.rs`.
- **Tests:** core lib **255/255**, adaptive_profile_integration **13/13**, ota_rate_lockstep **3/3**,
  modcod_ladder **10/10**, full openpulse-modem suite green (58 result lines, 0 failed); clippy
  `-D warnings` + fmt clean; workspace builds. Calibration sweeps run manually to derive the floors.
- **Test results:** all suites green; the FEC assignment is transparent to the lockstep/adaptive tests
  (they exercise stepping via SNR extremes). hpx_hf upper ladder is now FEC-protected, gap-free, and
  duplicate-free with data-derived floors.

---

## 2026-07-01 — feature: asymmetric fast-downshift + SNR-floor calibration (CAP-33)

- **Requirement/change:** at the HamRadio-2026 demo the receiver-led rate ladder took **up to 6
  retries** to reach a decodable mode after an SNR drop — it only stepped down one rung per
  `nack_threshold` failures. Make the downshift instant and SNR-directed, keep the upshift cautious,
  and validate the SNR/step thresholds the jump relies on.
- **Design decision:** (a) **asymmetric stepping** in `OtaRateController::on_rx_frame`: add a
  `level_for_snr(snr)` direct lookup (highest mapped rung whose `snr_floor ≤ SNR`); on a marginal
  decode *or* a low-SNR NACK, jump `rx_recommended` straight to that rung (possibly several steps
  down) instead of crawling; **upshift unchanged** — one proven mapped step, gated on the confirmed
  level's ceiling, never trusting an optimistic SNR to leap up. A transient failure at *good* SNR
  keeps the existing consecutive-NACK hysteresis (a blip can't drop the rate). (b) **Desync-safe:**
  the fast-down only moves `rx_recommended`; `rx_confirmed` (the just-decoded level) stays in
  `rx_candidates`, so a lost downshift ACK can't desync — the decode-path lockstep theorem holds
  unchanged. (c) **Calibration harness** (`tests/snr_floor_calibration.rs`, `--ignored`) sweeps AWGN
  SNR per rung to derive the empirical floors — the "quick simulation run." Running it caught **SL7
  (8PSK500, no FEC) mis-set at 14 dB vs 18 dB measured** — exactly the optimistic floor that made the
  controller recommend a mode ~4 dB below where it decodes; fixed to 18 dB. Lower rungs measured
  at/below their configured floors (conservative = fading margin), so only SL7 was wrong.
- **Implementation:** `level_for_snr` + asymmetric `on_rx_frame` in `crates/openpulse-core/src/ota_rate.rs`;
  SL7 floor 14 → 18 in `crates/openpulse-core/src/profile.rs`; new
  `crates/openpulse-modem/tests/snr_floor_calibration.rs`.
- **Tests:** `cargo test -p openpulse-core --lib ota_rate` — **15 pass** (new: fast-downshift on first
  low-SNR NACK, multi-step low-SNR downshift, cautious one-step upshift, transient-failure hysteresis;
  updated: asymmetric invariant; preserved: never-desync-under-ACK-loss, climb, clamps, lock). Full
  core lib **255 pass**; all openpulse-modem test binaries green; calibration sweep run manually
  (produced the SL7 finding); clippy `-D warnings` + fmt clean; workspace builds.
- **Test results:** ota_rate 15/15, core 255/255, modem suites green, calibration sweep ran. The
  6-retry down-stepping is replaced by a single-round jump to the SNR-adequate rung.

---

## 2026-07-01 — feature: ARDOP station ID + real SENDID/CWID (REQ-REG-10, CAP-39/CAP-66)

- **Requirement/change:** the auto-ID from CAP-66 covered only the daemon path; the **ARDOP TNC**
  (Pat → `openpulse-tnc`) had no auto-ID and its `CWID`/`SENDID` commands were honest warn-logged
  stubs — a REQ-REG-10 gap for that entry point. Wire the ID timer into the ARDOP worker and make the
  two commands real, without breaking ARDOP host compatibility.
- **Design decision:** (a) run the CAP-66 `StationIdTimer` (interval + sign-off) inside the ARDOP
  worker loop, firing **only at a frame boundary** (empty TX queue) so an ID never splits an
  in-progress transfer — the discipline real ARDOP TNCs use; armed via the engine `frames_transmitted`
  delta. (b) `SENDID` → arm a one-shot ID the worker sends at the next boundary; `CWID TRUE/FALSE` →
  toggle a Morse-CW-append flag — **host responses unchanged**, so the command surface *fulfils* its
  ARDOP contract instead of no-opping (more compatible, not less). The emitted ID is OpenPulseHF's own
  PHY (`DE <call>` in the active mode, + optional CW): the ARDOP layer is a TCP shim, never an ARDOP
  waveform, so there is no on-air ARDOP interop to break. (c) CW ID is a real feature, not a dead flag:
  a pure `cw_id::CwId` Morse generator (keyed-sine, PARIS timing, click-free ramps) + an engine
  `emit_cw_id` that routes through the single TX seam. (d) Reuse `[station] auto_id_*`; plumb via the
  ardop `ArdopConfig` (+ `bridge.set_auto_id`).
- **Implementation:** `crates/openpulse-core/src/cw_id.rs` (+ `pub mod`); `emit_cw_id` +
  `frames_transmitted` in `crates/openpulse-modem/src/engine.rs`; ARDOP worker frame-boundary ID +
  `set_auto_id` + `id_requested`/`cwid_enabled` in `crates/openpulse-ardop/src/bridge.rs`; `SENDID`/
  `CWID` handlers in `command.rs`; `ArdopConfig` fields (`lib.rs`, populated from `[station]` in
  `main.rs`); `..Default::default()` in the testmatrix/integration `ArdopConfig` literals.
- **Tests:** `cw_id` 8/8 (Morse table, PARIS unit/dash/inter-char/word timing, amplitude bounds,
  empty); ardop `command` 2/2 (SENDID arms one-shot + response unchanged; CWID toggles flag + response
  unchanged); `station_id_txcount` 2/2 (incl. `emit_cw_id` counts one frame, no-op CW doesn't);
  ardop integration 22/22 (compatibility intact); clippy `-D warnings` + fmt clean; full workspace
  builds.
- **Test results:** cw_id 8/8, ardop lib 2/2 + integration 22/22, station_id_txcount 2/2, no
  regressions in core/modem/config. REQ-REG-10 now holds on the ARDOP TNC path as well as the daemon.

---

## 2026-07-01 — feature: end-of-exchange (sign-off) station ID (REQ-REG-10, CAP-66)

- **Requirement/change:** §97.119 requires ID **at the end** of a communication as well as every 10
  minutes during it. The initial CAP-66 (below) covered only the interval trigger; add the sign-off ID.
- **Design decision:** extend `StationIdTimer` with an **idle-based** end-of-exchange trigger rather
  than hooking a specific teardown path (DISCONNECT / OTA-end): after the station has transmitted,
  once the channel is quiet for `signoff_idle_ms` the exchange has wound down → send a final ID. Idle
  detection is mode-agnostic (covers plain sends, ARQ, OTA, handshakes uniformly) and reuses the same
  TX-armed/`mark_identified` state as the interval trigger, so the two never double-fire and a
  pure-receive station still never keys up. `note_tx` now stamps the last-TX time; the interval and
  sign-off triggers coexist (a long continuous exchange fires the interval ID; going quiet fires the
  sign-off). New knob `[station] auto_id_signoff_idle_secs` (default 10 s; 0 disables sign-off only,
  keeping interval ID). Daemon checks `id_due` then `signoff_due` each tick and logs the `kind`.
- **Implementation:** `crates/openpulse-core/src/station_id.rs` — `signoff_idle_ms` field,
  `with_signoff_idle_ms` builder, `signoff_due`, `note_tx(now_ms)` (records last-TX time);
  `auto_id_signoff_idle_secs` in `crates/openpulse-config/src/lib.rs` (+ Default + template);
  `crates/openpulse-daemon/src/server.rs` computes `now_ms` first, arms via `note_tx(now_ms)`, fires
  on `id_due || signoff_due`.
- **Tests:** `cargo test -p openpulse-core --lib station_id` — **12 pass** (7 interval + 5 sign-off:
  disabled-when-idle-0, due-after-idle, later-TX-pushes-deadline, disarms-after-ID, interval+sign-off
  coexist on a long-then-quiet exchange); `station_id_txcount` 1/1; daemon 2/2; config 9/9; clippy
  `-D warnings` + fmt clean; full workspace builds.
- **Test results:** station_id 12/12, all gates green. REQ-REG-10 now covers both interval and
  end-of-exchange ID.

---

## 2026-07-01 — feature: periodic station-ID timer (REQ-REG-10, new CAP-66)

- **Requirement/change:** REQ-REG-10 (station identification at required intervals in the digital
  mode) was reclassified a ⚠ gap by the gap rescan — the ARDOP `CWID`/`SENDID` commands are stubs and
  no auto-ID path existed. Implement the missing periodic auto-ID so a running station identifies
  itself on air at the regulatory interval while transmitting.
- **Design decision:** (a) a **pure `StationIdTimer` state machine** in `openpulse-core` (interval +
  TX-armed + reset over an injected ms clock) so the regulatory semantics are deterministic and
  unit-testable — ID fires only when the station has transmitted since the last ID *and* the interval
  elapsed, so a pure-receive station never keys up. (b) Arm it from the **engine's `frames_transmitted`
  counter** incremented once at the single TX seam (`stage_emit_output`, where regulatory TX logging
  already lives) — the daemon polls the delta rather than threading a `note_tx()` through every
  transmit call site, and re-baselines after IDing so the ID frame itself doesn't re-arm the timer.
  (c) On due, the daemon keys PTT, sends `DE <callsign>` in the **active mode** (guaranteed-registered,
  and the mode the peer is already decoding), releases PTT — mirroring the OTA-ACK PTT turnaround.
  (d) `[station] auto_id_interval_secs` (default 600 s = US Part 97 10-minute rule; 0 disables; never
  auto-ID as the default `N0CALL`).
- **Implementation:** `crates/openpulse-core/src/station_id.rs` (+ `pub mod` in `lib.rs`);
  `frames_transmitted` field/increment/getter in `crates/openpulse-modem/src/engine.rs`;
  `auto_id_interval_secs` in `crates/openpulse-config/src/lib.rs` (+ Default + template);
  rx-ticker wiring in `crates/openpulse-daemon/src/server.rs`.
- **Tests:** `cargo test -p openpulse-core --lib station_id` — 7 pass (disabled/0-interval, not-due
  without TX, not-due-early, due-at-interval, reset-disarms, re-arm-after-next-interval,
  repeated-TX-one-ID-per-interval); `cargo test -p openpulse-modem --test station_id_txcount` — 1 pass
  (counter bumps per emit, never on receive); `cargo clippy -p openpulse-core -p openpulse-config
  -p openpulse-modem -p openpulse-daemon -- -D warnings` + `cargo fmt --all --check` clean; daemon
  suite 2/2, config suite green.
- **Test results:** station_id 7/7, station_id_txcount 1/1, daemon 2/2, clippy+fmt clean. REQ-REG-10:
  ⚠ gap → ✅ covered (CAP-66).

---

## 2026-07-01 — traceability fix: station-ID coverage corrected (gap rescan)

- **Requirement/change:** a project-wide gap rescan (TODO/FIXME/stub/dead-code/deferred sweep) found
  the matrix overstated station-ID coverage: CAP-39 claimed the ARDOP `CWID`/`SENDID` commands
  "provide station ID" and covered REQ-REG-05 (decodable ID) + REQ-REG-10 (interval ID), but both
  commands are honest warn-logged stubs (`crates/openpulse-ardop/src/command.rs:249,257`) — they
  accept+echo but transmit no CW-ID/ID-frame, and no independent auto-ID path exists in the codebase.
- **Design decision:** correct the record, not the code (the code is a deliberate, documented stub —
  not a defect). REQ-REG-05 is genuinely met on air by the **signed-handshake callsign** (the ConReq
  `station_id` rides RF, decodable by the receiver, #584) → re-attribute REQ-REG-05 to **CAP-01**.
  REQ-REG-10 needs a **periodic auto-ID timer that does not exist** → reclassify ✅ covered → ⚠ **gap**
  and add it to the deferred REQ-REG regulatory set (Phase 5.5-reg). Clear CAP-39's `Implements` to
  `—` and document the stub status inline.
- **Implementation:** `docs/dev/project/traceability-matrix.md` — REQ-REG-05 row → CAP-01; REQ-REG-10
  row → `—`/⚠ gap; CAP-01 `Implements` += REQ-REG-05 (+ design note on the on-air callsign);
  CAP-39 `Implements` → `—` (+ stub note); REQ-REG gap bullet gains `/10`; "Resolved 2026-07-01"
  subsection bullet. Also cleared a stale TODO in `docs/dev/archive/backlog-fec-improvements.md`
  (8PSK max-log-MAP soft demod was shipped in #187–#192, not still a TODO).
- **Tests:** docs-only; structural re-check confirms REQ↔CAP agree both directions (REQ-REG-05↔CAP-01,
  REQ-REG-10 uncovered like the other ⚠ REG rows) and no dangling CAP-39 REG links remain.
- **Test results:** matrix structural invariants hold; no code touched, so no behaviour delta.

---

## 2026-07-01 — housekeeping: rustfmt cleanup, FreeDV-diversity flag, traceability sync (docs/chore)

- **Requirement/change:** two follow-ons after the CE-SSB reference-mining PR (#602). (a) `cargo fmt
  --all -- --check` was failing on three files unrelated to any recent feature, so the workspace
  format gate was red on main. (b) The FreeDV 700D symbol-diversity idea surfaced by the mining pass
  needed a durable home, and the traceability matrix needed to reflect #602/#603.
- **Design decision:** (a) reformat the three offending files with `cargo fmt --all` (pure whitespace/
  line-wrapping, no logic touched) to restore the gate. (b) Record FreeDV 700D-style frequency
  diversity (repeat each carrier's symbol on a band-separated carrier + combine before slicing — a
  fading-margin lever distinct from FEC, a candidate sub-floor rung) as an unscheduled *Far-future
  item* in the roadmap so the lever isn't lost; no target date, no CAP yet. (c) Keep the matrix
  current per the standing rule: refine CAP-24's rationale, add a "Resolved 2026-07-01" subsection.
- **Implementation:** `cargo fmt` on `apps/openpulse-linksim/src/gui.rs`,
  `crates/openpulse-channel/src/lib.rs`, `pki-tooling/src/verification.rs` (PR #603); a deferred
  design entry in `docs/dev/project/roadmap.md` (PR #603); CAP-24 row + "Resolved 2026-07-01"
  subsection + `last_updated` bump in `docs/dev/project/traceability-matrix.md` (PR #604).
- **Tests:** no code-logic change. `cargo fmt --all -- --check` — clean on main after #603.
- **Test results:** fmt gate green; PRs #603 and #604 merged. Docs/chore only, no behaviour delta.

---

## 2026-07-01 — CE-SSB: principled per-mode gate rationale + reference mining (no behaviour change)

- **Requirement/change:** a reference-mining pass (Hershberger CE-SSB QEX 2014/2016, `drmpeg/gr-cessb`,
  Kahn EER 1952, "The Polar Explorer" QEX 2017, PE1NNZ direct-SSB, *Dave's Hacks* 2025 polar
  modulation) asked whether we should adopt the iterative clip→filter→overshoot-compensate loop, and
  found the `cessb_benefits` gate documented by contradictory comments (header + two tests claimed
  8PSK "benefits/stays on", while the code and `cessb_power_evm` assert 8PSK is gated OFF).
- **Design decision:** (a) **keep** the single-pass look-ahead peak-stretch limiter and **reject** the
  Hershberger/gr-cessb iterative clip-filter loop for the *data* path — it injects more in-band EVM,
  the exact quantity that breaks our dense constellations (tuned for voice, not tight slicers).
  (b) Convert the empirical gate into a **principled** one using the equal-amplitude-singularity
  derivation (*Dave's Hacks*): CE-SSB benefits ⇔ high-PAPR envelope (rare hard nulls) **and** loose
  decision margins — true for QPSK-subcarrier OFDM, false for 8PSK/QAM/APSK and single-carrier QAM
  (constellations transit the origin → envelope nulls → phase discontinuity → EVM). Gate *logic*
  unchanged; only its documentation is corrected and grounded. (c) Catalogue the sources in
  `references.md`, including the polar/EER family as inspiration for a possible future direct-RF
  (QMX/uSDX Class-E) backend — explicitly out of scope for the current soundcard→linear-rig path.
- **Implementation:** rewrote the `ModemEngine::cessb_benefits` doc comment
  (`crates/openpulse-modem/src/engine.rs`) and corrected the stale 8PSK claims in
  `apps/openpulse-linksim/tests/cessb_ab.rs` and `crates/openpulse-modem/tests/cessb_power_evm.rs`;
  added a "CE-SSB and polar-SSB transmit conditioning" section to `docs/dev/research/references.md`.
- **Tests:** `cargo test -p openpulse-modem --no-default-features --test cessb_power_evm` — 3 pass;
  `cargo test -p openpulse-linksim --no-default-features --test cessb_ab` — 3 pass;
  `cargo clippy -p openpulse-modem --no-default-features -- -D warnings` clean. Comment-only change;
  the gate's behaviour (and thus every other CE-SSB test) is unaffected.
- **Test results:** cessb_power_evm 3/3, cessb_ab 3/3, clippy clean. No behaviour delta — the edits
  align the docs with the already-validated gate and record the rejected alternative.

---

## 2026-06-30 — testmatrix: full-tier completes + OFDM52 raw-framing exclusion generalised

- **Requirement/change:** the `--full` tier must run end-to-end and its Clean/high-SNR failures must
  reflect only genuine results, not case-generator artifacts. Two defects blocked that: the B2F
  runner hung at low SNR, and the `OFDM_RAW_FRAMING_ONLY` exclusion covered only one of three
  case-gen sites that pair plain OFDM52 with RS-family FEC.
- **Design decision:** (a) give the B2F driver streams a socket read/write timeout so a non-decoding
  channel fast-fails instead of blocking forever (the driver maps `TimedOut`/`WouldBlock` →
  `DriverError::Timeout`); (b) lift the per-site `if OFDM_RAW_FRAMING_ONLY.contains(..)` check into a
  single `raw_framing_excludes(mode, fec)` predicate and apply it at *every* site that pairs a
  raw-framing-only mode with FEC, so the padded-frame × 255-byte-RS-block limitation can never leak a
  spurious RS-decode failure again. Keep the legitimate OFDM52 no-FEC-at-floor failure visible.
- **Implementation:** `connect_with_timeout` + `B2F_IO_TIMEOUT = 8 s` in
  `apps/openpulse-testmatrix/src/runners/b2f.rs` (PR #600); `raw_framing_excludes` predicate applied
  at the multicarrier×prop and large-payload sections in `apps/openpulse-testmatrix/src/cases.rs`.
- **Tests:** `apps/openpulse-testmatrix` unit suite (coverage regression test still requires OFDM52
  to appear in a raw case) — 12 pass; clippy `-D warnings` + `cargo fmt --all --check` clean.
- **Test results:** quick tier **555/555, 0 fail** (gate at `docs/test-reports/latest`, unchanged).
  Full tier **6022 cases, 3465 pass, 2557 fail, 21.3 s**, exit 0 — completes without hanging; the
  regression-suspect zone (Clean / AWGN ≥ 20) dropped from 5 failures to 1 (the kept no-FEC
  OFDM52 large-payload point); the lone B2F failure is now the intended fast-timeout at 0 dB.
  Characterization snapshot committed under `docs/test-reports/full` (with a README marking it as
  non-gate data).

---

## 2026-06-29 — CLI: `daemon set-tx-attenuation` (control-surface parity)

- **Requirement/change:** a wiring-gap audit found `SetTxAttenuation` reachable from the panel but
  not the CLI — the one remaining single-client control-surface asymmetry (not a dead command; the
  daemon handles it). Add the CLI subcommand for headless/scripted operation.
- **Design decision:** mirror the established `simple(addr, ControlCommand::…)` pattern — a
  `SetTxAttenuation { db, band }` `DaemonCommands` variant (`band` optional, `--band`) forwarding the
  existing control command. No daemon/protocol change.
- **Implementation:** `crates/openpulse-cli/src/cli.rs` (variant) + `crates/openpulse-cli/src/commands/daemon.rs` (arm).
- **Tests:** covered by the existing CLI command-dispatch suite (29 + integration); no new logic to unit-test beyond the forward.
- **Test results (run):** `cargo build/clippy -p openpulse-cli --no-default-features --all-targets -D warnings` clean; `cargo test -p openpulse-cli --no-default-features` all green; `fmt` clean.

## 2026-06-29 — Generic serial CAT backend wired into the daemon

- **Requirement/change:** make the already-built-but-unreachable generic serial CAT backend
  (`GenericSerialCat`, FF-13) selectable from the daemon. The machinery (TOML rig definitions,
  `CatController` trait, serial transport) existed and was tested, but the daemon only ever built a
  concrete `RigctldController` and honoured `cat_backend = "rigctld"`.
- **Design decision:** give the daemon a backend-agnostic CAT handle. `RigctldController` gained the
  `CatController` impl its trait doc already claimed (delegating to its inherent methods). The daemon
  selects via a concrete `CatBackend` enum (`Rigctld` | `Generic`, the latter feature-gated) rather
  than `Box<dyn CatController>` — a boxed trait object tripped the `Drop`-in-loop borrow checker at
  the per-tick reborrow sites, whereas the concrete enum reborrows cleanly like the original
  `Option<RigctldController>`. `server::build_cat_controller` maps `[radio] cat_backend` →
  `none`/`generic`/`rigctld`; the generic arm reads top-level `[radio] serial_port` + `rig_file` and
  is gated by the daemon `generic-serial` feature (→ `openpulse-radio/generic-serial`, Unix). The 4
  daemon rig-control signatures became `Option<&mut (dyn CatController + Send)>`. Meter polling stays
  rigctld-only (its own connection). `rig_a` stays reserved for the multi-rig refactor (docs
  corrected to stop implying the generic fields there are wired — the top-level ones are).
- **Implementation:** `crates/openpulse-radio/src/rig_controller.rs` (`impl CatController for
  RigctldController`); `crates/openpulse-radio/src/cat_controller.rs` (doc); `crates/openpulse-config/
  src/lib.rs` (`RadioConfig.serial_port` / `rig_file` + template + corrected `rig_a`/`RigConfig`
  docs); `crates/openpulse-daemon/Cargo.toml` (`generic-serial` feature); `crates/openpulse-daemon/
  src/server.rs` (`CatBackend` enum + `CatController` impl + `build_cat_controller`; call sites →
  `as_mut().map(... as &mut dyn ...)`); `crates/openpulse-daemon/src/lib.rs` (rig param types +
  import).
- **Tests:** `server.rs` `cat_backend_tests` — `cat_backend = "none"` yields no controller;
  `cat_backend = "generic"` with no rig file yields no controller and no panic (covers feature-on
  open-failure and feature-off branches). `GenericSerialCat` itself is covered by its existing
  `MockTransport` tests.
- **Test results (run):** `cargo build -p openpulse-daemon --no-default-features` and
  `--features generic-serial` both green; `cargo test -p openpulse-daemon --no-default-features
  cat_backend_tests` 2/2; config tests green. `clippy -D warnings` clean; `fmt` clean on touched
  crates.

## 2026-06-29 — ARDOP adaptive ARQ session + real ARQBW/ARQTIMEOUT

- **Requirement/change:** make the ARDOP host hints `ARQBW` and `ARQTIMEOUT` real instead of
  stored-and-ignored. Blocked because the TNC never started an adaptive session (so the rate ladder
  was dormant and there was nothing to bound), and the only bandwidth lever targeted the OTA
  controller, not the rate_policy one the worker reads.
- **Design decision:** (1) opt-in `[ardop] enable_adaptive_arq` (+ `adaptive_profile`) → `main.rs`
  calls `start_adaptive_session`, flipping the worker to its existing adaptive `transmit_arq` /
  `receive_with_ack_hint` branch (default off = unchanged fixed-mode behaviour). (2) A rate_policy
  bandwidth cap distinct from the OTA bounds: `RateAdapter::clamp_to` never raises the level;
  `RateAdaptationPolicy::set_max_tx_level` clamps the active session immediately and re-clamps after
  every ack so AckUp can't climb past the cap (an `Increased` event past the cap is reported as
  `Maintained`). The Hz→level map lives in `openpulse-qsy::bandplan` (`max_speed_level_for_bandwidth`)
  — kept out of `openpulse-modem` (no qsy dep / cycle); the ARDOP worker owns the mapping. (3) The
  worker applies the ARQBW cap when it changes and disconnects an idle connection after ARQTIMEOUT
  seconds, using non-blocking `try_read`/`try_write` on the tokio RwLocks from the sync worker.
- **Implementation:** `crates/openpulse-core/src/rate.rs` (`clamp_to` on `RateAdapter` +
  `BiDirRateAdapter`); `crates/openpulse-modem/src/rate_policy.rs` (`max_tx_level`,
  `set_max_tx_level`, `defined_modes`, clamp in `apply_ack_internal`); `crates/openpulse-modem/src/
  engine.rs` (`set_arq_max_tx_level`, `adaptive_profile_modes`); `crates/openpulse-qsy/src/
  bandplan.rs` (`max_speed_level_for_bandwidth`); `crates/openpulse-config/src/lib.rs`
  (`ArdopConfig.enable_adaptive_arq` / `adaptive_profile` + template);
  `crates/openpulse-ardop/src/main.rs` (start session), `bridge.rs` (worker cap + ARQTIMEOUT +
  activity tracking), `command.rs` (comment now reflects applied behaviour); ardop gains an
  `openpulse-qsy` dep.
- **Tests:** `adaptive_profile_integration.rs` — `arq_max_tx_level_caps_the_adaptive_ladder`
  (AckUp×8 can't pass an SL4 cap; clearing it climbs again) + `arq_max_tx_level_clamps_an_already_high_session`
  (cap below current level clamps immediately); `bandplan.rs` —
  `max_speed_level_for_bandwidth_maps_hz_cap_to_a_level` (500/700/2000 Hz caps, below-floor `None`,
  unknown-mode skip).
- **Test results (run):** `cargo test -p openpulse-core -p openpulse-modem -p openpulse-qsy -p
  openpulse-config -p openpulse-ardop --no-default-features` → green (modem-adaptive 13, qsy 28,
  ardop 22, core lib 226, modem lib 45, …; 0 failed). `cargo clippy …-D warnings` 0 warnings; `fmt`
  clean on touched crates; full workspace (sans pki) builds.

## 2026-06-29 — Signed handshake over RF into the daemon connect (+ verified logbook grid)

- **Requirement/change:** wire the Ed25519 signed `ConReq`/`ConAck` handshake into the daemon's
  `ConnectPeer`/RF path (it was a tested library primitive the daemon never exchanged — `ConnectPeer`
  was a local trust eval), store the verified peer identity, and feed the verified grid to the ADIF
  logbook (closing logbook item B). The keystone that also unblocks host-driven ARQ bounds.
- **Design decision:** *additive* exchange, not a rewrite of connect — `begin_secure_session` still
  runs; `ConnectPeer` additionally signs+sends a `ConReq` and records a `PendingHandshake`. Frames
  are ~530 B > the 255 B modem-frame cap, so they're **SAR-fragmented** (`sar_encode`) on TX and
  reassembled (`SarReassembler`) on RX; the reassembly is a fall-through after relay/QSY dispatch
  (handshake fragments are binary, not QSY ASCII / relay envelopes) and is confirmed by the
  reassembled `HSCQ`/`HSAK` magic, so no wire marker is needed (which wouldn't fit anyway). The
  responder verifies + replies `ConAck` + records the peer; the initiator verifies the `ConAck`
  against its in-flight `ConReq` (session-id gated) + records the peer + clears pending. Grid is a
  `skip_serializing_if`-empty signed field on `ConReq`/`ConAck` so legacy zero-grid frames and their
  signatures stay byte-identical; added `create_with_grid` constructors leaving the 25 existing
  `create` callers untouched. Station key from `[station] identity_key_path` (default
  `~/.config/openpulse/identity.key`, auto-generated; explicit path lets the twin rig hold distinct
  identities). New `ControlEvent::PeerVerified`; 30 s CONACK timeout via `expire_pending_handshake`.
  Verification uses `PolicyProfile::Permissive` (signature proves key possession; first-seen peers
  still connect, mirroring the optimistic `ConnectPeer`).
- **Implementation:** `crates/openpulse-core/src/handshake.rs` (grid field + `create_with_grid`);
  `crates/openpulse-config/src/lib.rs` (`StationConfig.identity_key_path` + template);
  `crates/openpulse-daemon/src/lib.rs` (`PendingHandshake`/`VerifiedPeer`, `RuntimeControlState`
  fields incl. `handshake_sar`, `transmit_handshake_frame`, `try_reassemble_handshake`,
  `handle_inbound_conreq`/`handle_inbound_conack`, `record_verified_peer`,
  `expire_pending_handshake`, `ConnectPeer` CONREQ send, RX dispatch);
  `crates/openpulse-daemon/src/logbook.rs` (`set_pending_peer_grid`);
  `crates/openpulse-daemon/src/server.rs` (load identity seed at startup; expiry tick);
  `crates/openpulse-daemon/src/protocol.rs` + `apps/openpulse-panel/src/connection.rs`
  (`PeerVerified` event + panel log).
- **Tests:** `crates/openpulse-core/src/handshake.rs` inline (grid round-trip, grid is
  signature-covered, empty-grid byte-identical to legacy); `crates/openpulse-daemon/src/lib.rs`
  `handshake_rf_tests` (responder reassembles+verifies+records; initiator verifies+stamps logbook
  grid into the ADIF record; mismatched-session CONACK ignored; ConnectPeer initiates; full-size SAR
  fragment survives BPSK250; pending-handshake timeout).
- **Test results (run):** `cargo test -p openpulse-core -p openpulse-config -p openpulse-daemon
  --no-default-features` → all green (core lib 226, handshake_integration 17, daemon lib incl.
  `handshake_rf_tests` 6, config 16, …; 0 failed). `cargo clippy -p openpulse-core -p
  openpulse-config -p openpulse-daemon -p openpulse-panel --no-default-features --all-targets -D
  warnings` → 0 warnings. `cargo fmt` clean on the touched crates.

## 2026-06-29 — Panel: AGC on/off toggle (control-surface parity)

- **Requirement/change:** close the last open control-surface parity gap — the receiver streaming
  AGC (`ControlCommand::SetAgc`, shipped 2026-06-28 in the daemon + CLI) had no panel button, so the
  GUI operator couldn't toggle it. (Re-audit found the CLI `SendMessage`/`SetMode`/PTT/QSY-accept/
  reject gaps and the panel Squelch gap from the 2026-06-27 audit were already closed; AGC was the
  only one left.)
- **Design decision:** mirror the existing Notch/CE-SSB/Logbook toggle pattern exactly — a single
  `AGC: ON/OFF` button in the right-hand controls column that flips local `agc_enabled` and sends
  `ControlCommand::SetAgc { enabled }`. Default off, matching the daemon's `[modem] agc_enabled`
  default and the engine's opt-in AGC. No new state machinery; one bool field + one button.
- **Implementation:** `apps/openpulse-panel/src/app.rs` — `agc_enabled: bool` field (default false),
  `AGC: ON/OFF` button next to the Notch toggle in `draw_controls`, with hover text noting the
  active-span gating. Stale docs corrected: roadmap §10.6 "panel toggle parity" marked done; the
  control-surface parity table updated to show all flagged gaps closed.
- **Tests:** GUI toggle (no unit test — the `SetAgc` daemon/engine path is covered by
  `crates/openpulse-modem/tests/agc_loopback.rs`).
- **Test results:** `cargo build -p openpulse-panel --no-default-features` green;
  `cargo clippy -p openpulse-panel --no-default-features --all-targets -- -D warnings` 0 warnings;
  `cargo fmt -p openpulse-panel --check` clean. Visual confirmation pending (held before merge per
  the GUI-change rule).

## 2026-06-29 — Docs: sort `docs/dev/` into topic subfolders + fix all references

- **Requirement/change:** the loose files under `docs/dev/` were moved into four topic
  subfolders — `design/` (architecture, design, freq-acquisition-design, hpx-waveform-design,
  testbench-design), `pki/` (the 11 `pki-tooling-*` docs), `research/` (ardop/freedv-auth/js8call/
  ofdm/pactor research, reference-mining-plan, references, vara-research, wsjtx-analysis), and
  `project/` (backlog, changelog, roadmap, traceability). All inbound and outbound references had
  to follow the move so no link or doc-path breaks.
- **Design decision:** keep the moves as `git mv` renames (history-preserving) and rewrite every
  reference in two classes: (1) full-path mentions `docs/dev/<base>` → `docs/dev/<subdir>/<base>`
  across the whole tree (markdown link text, `doc:` frontmatter self-pointers, and `.rs` doc/line
  comments); (2) relative markdown link *targets* fixed per linking-file location — the dev README
  index (`](base.md)` → `](subdir/base.md)`), the manual's `dev/<base>` targets, and the moved
  files' own outgoing relative links that gained/needed a directory level
  (`../mode-fec-ladder.md` → `../../mode-fec-ladder.md`, siblings via `../`).
- **Implementation:** 29 `git mv` renames; a 29-substitution `sed` script applied to all tracked
  text files for class (1); targeted per-file `sed` for class (2) in `docs/dev/README.md`,
  `docs/openpulse-manual.md`, `docs/{features,mode-fec-ladder}.md`,
  `docs/dev/{hpx-session-state-machine,vara-parity-execution-board}.md`,
  `docs/dev/reviews/review-26050{8,17}.md`, and the moved `design/`/`project/` files. Five `.rs`
  doc-comment path mentions updated (`openpulse-ardop`, `openpulse-config`, `openpulse-dsp`,
  `openpulse-kiss`, `pilot` plugin) — comment-only, no code change.
- **Tests:** a Python link-integrity walker that resolves every relative `.md` link target against
  the filesystem; a basename grep for any surviving old `docs/dev/<base>` path.
- **Test results:** old-path grep → 0 hits. Link walker → only 3 broken links remain
  (`docs/README.md` → `marketing/{banner,flyer,presentation}.md`), which are pre-existing (those
  targets were never committed) and unrelated to this move. `cargo fmt --all --check` shows 5
  pre-existing deviations, all in files outside this change set (`gui.rs`, `channel/lib.rs`,
  `agc_loopback.rs`, `verification.rs`); no new fmt regression (rustfmt does not reflow comments).

## 2026-06-28 — Panel: controls to a right side-panel; status below the waterfall

- **Requirement/change:** move the right-column (session status) elements below the waterfall; make
  the waterfall as wide as the spectrum; move all controls except connection / PTT / callsign+Connect-RF
  into the right column.
- **Design decision:** keep only connection (transport/server/Connect), PTT, RF-connect
  (callsign/Connect RF), and the connection indicator in the top toolbar. Everything else (Mode,
  Freq/Tune, Repeater, CE-SSB, Notch, Logbook, OTA, TX Atten, Squelch, Config, Messages, QSY) moves
  to a resizable right `SidePanel` rendered by a new `PanelApp::draw_controls`. The `CentralPanel`
  drops its 2-column split and stacks the spectrum pane (now full width) then the session status
  below it, inside a vertical `ScrollArea`. Waterfall widened to the full pane width (was capped at
  512 px) in `draw_spectrum_pane`.
- **Implementation:** `apps/openpulse-panel/src/app.rs` — toolbar slimmed; `draw_controls` method;
  right `SidePanel` + stacked `CentralPanel`. `apps/openpulse-panel/src/ui.rs` — waterfall size to
  `available_width × 96`.
- **Tests:** GUI layout (no unit test).
- **Test results:** `cargo fmt -p openpulse-panel --check` clean; `cargo build -p openpulse-panel`
  green; `cargo clippy -p openpulse-panel --all-targets -- -D warnings` 0 warnings. Visual
  confirmation pending (held before merge per the GUI-change rule).

## 2026-06-28 — Linksim: regroup Station B views + waterfall/constellation toggles

- **Requirement/change:** swap the Station B RX spectrum/waterfall with the ACK/NACK
  spectrum/waterfall; keep Station B's constellation at the far right; add controls to
  enable/disable the waterfalls and the constellation diagrams.
- **Design decision:** swap the middle and far-right signal columns so the column order is
  `[A TX | ACK (B→A) | B RX]` — grouping all of Station B's RX views (spectrum, waterfall, and the
  far-right I/Q constellation in the branding band) on the right, with the ACK in the middle. Two
  toolbar checkboxes (`ui_show_waterfall`, `ui_show_constellation`, both default on): `draw_panel`
  gains a `show_waterfall` flag (early-returns after the spectrum when off); the branding band
  conditionally renders the two `constellation_plot`s and recomputes the flanking text width
  (`3×qr_side` → `1×qr_side`) so the QR stays centered when constellations are hidden.
- **Implementation:** `apps/openpulse-linksim/src/gui.rs` — toolbar checkboxes; column swap
  (panels[2] ACK middle, panels[1] B RX far right); `draw_panel(.., show_waterfall)`; gated
  branding band.
- **Tests:** GUI layout/visualization (no unit test); existing gui unit tests unaffected.
- **Test results:** `cargo build/clippy/test -p openpulse-linksim --features gui` green (3/3 gui
  unit tests). Visual confirmation pending (held before merge per the GUI-change rule).

## 2026-06-28 — Linksim: symbol-spaced (crisp-dot) constellations

- **Requirement/change:** sharpen the I/Q constellations (#574) from a full-rate cloud to discrete
  per-symbol dots so the clean-TX vs noisy-RX contrast reads as a real constellation.
- **Design decision:** parse samples/symbol from the mode's trailing baud (`samples_per_symbol`,
  order/suffix-stripped; `None` for OFDM/SCFDMA/PILOT/FSK which have no PSK symbol grid), then
  sample the Hilbert baseband once per symbol at the **best timing phase** (peak mean magnitude —
  symbol centers carry full amplitude, transitions dip). No full timing/carrier recovery — a cheap
  estimate that's honest about being a viz, not a demod. Multicarrier/FSK keep the full-rate cloud.
- **Implementation:** `apps/openpulse-linksim/src/gui.rs` — `samples_per_symbol()`, `baseband_iq()`
  best-phase symbol-spaced path, `PanelView::push(samples, sps)`, `sps` threaded from `fs.mode`.
- **Tests:** `apps/openpulse-linksim/src/gui.rs` unit tests (gui feature): baud parsing
  (order/suffix/multicarrier cases), and symbol-spaced-vs-cloud (far fewer points + tighter
  Q-spread on synthetic BPSK).
- **Test results:** `cargo test -p openpulse-linksim --features gui --bin openpulse-linksim-gui`
  3/3 pass; `cargo clippy -p openpulse-linksim --features gui --all-targets` 0 warnings.

## 2026-06-28 — Streaming AGC rollout to the PSK ladder (active-span gated)

- **Requirement/change:** roadmap 10.6 — roll the existing `openpulse-dsp::agc::Agc` out as a
  receiver front-end level normaliser for the PSK/QAM ladder, with active-span gating.
- **Design decision:** place it at the **single `route_audio_stage(InputCapture)` seam** (after the
  notch: remove interference, then normalise) so every capture path — `receive*` family and the
  daemon's `accumulate_capture` streaming path — gets it by construction. Opt-in (default off), like
  the notch, so dense-mode canaries can't regress unless enabled. The AGC's own docs forbid running
  on the raw capture (leading silence ramps the gain to its clamp); satisfied via **active-span
  gating** — `Agc::lock()` freezes the gain on sub-squelch (silent) blocks, `unlock()` adapts on
  carrier-present blocks (RMS ≥ DCD threshold). Tripwire counter mirrors the notch's. Exposed for the
  running daemon (no dead capability): `ControlCommand::SetAgc` + CLI `daemon set-agc`.
- **Implementation:** `crates/openpulse-modem/src/engine.rs` (`agc`/`agc_enabled`/
  `agc_blocks_processed` fields, `enable_agc`/`disable_agc`/`configure_agc`/`is_agc_enabled`/
  `agc_gain_db`/`agc_blocks_processed`, `apply_rx_agc`, seam wiring); `openpulse-daemon`
  `protocol.rs` + `lib.rs` (`SetAgc`); `openpulse-cli` `cli.rs` + `commands/daemon.rs` (`SetAgc`).
- **Tests:** `crates/openpulse-modem/tests/agc_loopback.rs` — off-by-default+toggle; tripwire on the
  `accumulate_capture` (daemon) path; active-span gating (gain ~0 dB through silence, boosts a weak
  carrier); decode of a ~30 dB-attenuated QPSK500 frame with AGC on.
- **Test results:** `agc_loopback` 4/4; full `openpulse-modem --no-default-features` suite green;
  `notch_loopback` 4/4 (notch path unchanged); workspace build (excl. pki-tooling) green; clippy
  (modem/daemon/cli) 0 warnings.

## 2026-06-28 — Host-driven TNC control (ARQBW/ARQTIMEOUT): blocked, finding recorded

- **Requirement/change:** wire the ARDOP `ARQBW`/`ARQTIMEOUT` host hints into the engine for real,
  replacing the accepted-but-ignored no-ops the #571 audit flagged.
- **Investigation:** same blocked class as the signed handshake (B). `crates/openpulse-ardop/src/
  main.rs` never calls `start_adaptive_session`/`start_ota_session`, so `current_tx_level()` is
  always `None` and `worker_loop` always runs the **fixed-mode** path — the adaptive ARQ ladder is
  dormant. `ARQBW` has no ladder to cap; `ARQTIMEOUT` has no ARQ connection to time out (the worker
  does single-shot `receive(mode, None)`). The only bandwidth-cap lever (`ota_set_level_bounds`)
  targets the **OTA** controller, but the worker's adaptive path reads the **rate_policy** controller
  — different mechanisms; no rate_policy bandwidth cap exists.
- **Decision:** wiring no-ops into the dead fields would re-create the "defined-but-not-consumed" gap
  the audit removed, so it was deliberately NOT done. Real fix is a feature (TNC runs an adaptive ARQ
  session + rate_policy bandwidth cap + connection timeout), recorded in `docs/dev/project/roadmap.md` under
  the TNC command-surface audit.
- **Implementation:** none (no speculative surface); roadmap finding only.
- **Test results:** docs-only; workspace gates unaffected.

## 2026-06-28 — Linksim: I/Q constellation views flanking the QR branding band

- **Requirement/change:** show a constellation diagram for Station A to the left of the QR code and
  one for Station B to the right, keeping the text closest to the QR.
- **Design decision:** the `FrameStep` carries only real passband waveforms, so derive baseband I/Q
  via the existing `openpulse_core::iq::hilbert_iq` (fc=1500 Hz, fs=8 kHz — the `ModemEngine`
  defaults), trim the 31-sample group-delay edges, RMS-normalize, and decimate to ≤700 points — the
  same "viz straight from passband samples" approach the spectrum/waterfall already use. Map
  Station A = `forward_tx` (clean TX, panel 0), Station B = `forward_rx` (post-channel RX, panel 1),
  giving a clean-vs-noisy contrast that matches the app's "Station A | Channel | Station B" framing.
  Branding band reordered to `[const A | wordmark | QR | tagline | const B]` so the text stays
  nearest the QR and the constellations sit on the outer edges.
- **Implementation:** `apps/openpulse-linksim/src/gui.rs` — `PanelView.iq`, `baseband_iq()`,
  `constellation_plot()` (egui_plot `Points`, fixed unit bounds, no axes/grid), branding-band
  rewrite.
- **Tests:** GUI visualization (no unit test); `baseband_iq` reuses the unit-tested `hilbert_iq`.
- **Test results:** `cargo build -p openpulse-linksim --features gui` green; `cargo clippy -p
  openpulse-linksim --features gui --all-targets` 0 warnings. Visual confirmation pending (held for
  the user before merge, per the GUI-change rule).

## 2026-06-27 — Logbook peer GRIDSQUARE via handshake (B): blocked, finding recorded

- **Requirement/change:** carry the worked station's grid in the signed handshake so the logbook
  fills `GRIDSQUARE` from a verified, on-air source (the richer-fields item B, follow-on to A).
- **Investigation:** the Ed25519 signed handshake (`ConReq`/`ConAck`, `openpulse-core/src/
  handshake.rs`) is a tested library primitive that the **daemon never exchanges**. The
  `ConnectPeer` path runs `ModemEngine::begin_secure_session`, a *local* trust evaluation
  (`evaluate_handshake` over locally-supplied params) — it sends no `ConReq` and verifies no peer
  `ConAck` over RF. `ConReq`/`ConAck` are referenced only by the handshake lib + its tests.
- **Decision:** B is blocked on a larger prerequisite — wiring the over-the-air signed
  `ConReq`→`ConAck` exchange into the daemon connect — not a field add. Adding a grid field to a
  primitive the daemon never exchanges would create a fresh "defined-but-not-consumed" gap (the
  exact anti-pattern the TNC/config audits just removed), so it was deliberately NOT done. The
  config `[logbook.peer_grids]` map (A, shipped) remains the interim source.
- **Implementation:** none (no speculative surface). Finding + real-fix path recorded in
  `docs/dev/project/roadmap.md` ("Signed handshake not wired into the daemon connect").
- **Test results:** docs-only; workspace gates unaffected (no code change).

## 2026-06-27 — Logbook peer GRIDSQUARE via config map (A)

- **Requirement/change:** populate the ADIF `GRIDSQUARE` (worked station's grid). The audit found
  the grid is NOT carried by the handshake/peer-cache/engine today, so "from the handshake" needs a
  protocol change (tracked as B). Deliver the outcome now via a config lookup.
- **Design decision:** a `[logbook.peer_grids]` callsign→grid map (case-insensitive), consulted at
  `begin_qso` by peer callsign — what most logging software does. Composes with B later (the
  handshake-exchanged grid would take precedence over the map).
- **Implementation:** `openpulse-config` `LogbookConfig.peer_grids`; `logbook.rs` (lookup at
  begin_qso → `Pending.gridsquare` → `GRIDSQUARE`); `server.rs` passes the map; TOML template.
- **Tests:** logbook unit (GRIDSQUARE from a lowercase-key map + uppercase connect), daemon
  integration (`connect_then_disconnect…` asserts `<GRIDSQUARE>`), config default (empty map).
- **Test results:** logbook 5/5, daemon integration passes, config 9/9, clippy 0.

## 2026-06-27 — ARDOP/KISS TNC command-surface audit

- **Requirement/change:** audit the ARDOP + KISS TNCs for the "accepted/advertised but not applied"
  gap class (a command the TNC accepts but no-ops, or a doc claim the code doesn't honour).
- **Finding:** ARDOP — `GRIDSQUARE`/`ARQBW`/`ARQTIMEOUT` are validated + echoed but never read by
  the engine (the modem self-manages bandwidth/timeout via its adaptive ladder); `CWID`/`SENDID`
  are honest warn-logged stubs. KISS — only `KISS_DATA` is applied; the 6 control frames
  (TXDELAY/P/SlotTime/TXtail/FullDuplex/SetHardware) were *silently* dropped.
- **Design decision:** the no-ops are defensible (self-managed rate/PTT) but were silently
  misleading. Make them honest, don't implement host-driven control speculatively, track the real
  wiring. KISS: log dropped control frames (`debug!`) instead of silent. ARDOP: code comment +
  corrected `docs/non-gpl-interfacing.md` (split "implemented" vs "accepted-not-applied" vs "stub").
  Roadmap "TNC command-surface audit" records the real-wiring follow-ups.
- **Implementation:** `crates/openpulse-kiss/src/server.rs` (log); `crates/openpulse-ardop/src/
  command.rs` (comment); `docs/non-gpl-interfacing.md`; `docs/dev/project/roadmap.md`.
- **Test results:** ardop + kiss build; clippy 0; no behavior change beyond a debug log.

## 2026-06-27 — Adaptive-profile FEC audit (+ a permanent gate)

- **Requirement/change:** audit every adaptive profile's FEC assignment for the `cli_adaptive`
  bug class (a profile assigning no/wrong FEC to a mode that needs it — `hpx_ofdm_hf` had OFDM52-8PSK
  with no FEC).
- **Finding:** all 12 profiles are now **correct** — every modulatable rung decodes a clean loopback
  with its assigned FEC. The only rungs that don't decode are `hpx_narrowband_hd`'s SL8/SL9
  (QPSK9600-RRC / 8PSK9600-RRC), which can't modulate at 8 kHz — but `profile.rs` already documents
  that profile as **requiring a 48 kHz audio path**, so that's by design, not a gap.
- **Design decision:** promote the audit probe into a permanent CI gate rather than a one-off — it
  would have caught the `cli_adaptive` bug. The gate iterates every profile × rung, asserts clean
  decode with the assigned FEC, and pins the count of known-unmodulatable (48 kHz) rungs at 2 so a
  new unreachable rung trips it.
- **Implementation:** `crates/openpulse-modem/tests/channel_loopback.rs`
  `every_profile_rung_decodes_clean_with_its_fec` (no source change — the profiles were correct).
- **Test results:** gate passes; clippy 0.

## 2026-06-27 — ADIF logbook follow-ups (runtime toggle + parity + richer fields)

- **Requirement/change:** complete the ADIF logbook — a runtime `SetLogbook` control with CLI/panel
  parity (config-only before), and richer fields (RST/COMMENT from the RX SNR).
- **Design decision:** mirror the `SetNotch`/`SetCessb` pattern (control command + thin CLI
  `simple()` wrapper + panel toggle). `Logbook::set_enabled` for runtime control. At disconnect,
  read `engine.last_rx_snr_db()` → `RST_RCVD` (coarse SNR→RST bucket) + a `COMMENT` carrying the
  mode and SNR. Peer `GRIDSQUARE` from the handshake deferred — not exposed on the engine yet.
- **Implementation:** `crates/openpulse-daemon/src/logbook.rs` (`set_enabled`/`is_enabled`,
  `end_qso(now_ms, rx_snr_db)`, `rst_from_snr`); `protocol.rs` `SetLogbook`; `lib.rs` handler +
  disconnect passes the SNR; CLI `daemon set-logbook`; panel `Logbook: ON/OFF` toggle.
- **Tests:** logbook unit (runtime-toggle writes, RST/COMMENT present, `rst_from_snr` buckets);
  existing connect→disconnect integration still passes; CLI parse.
- **Test results:** daemon lib + logbook **all pass**; CLI `set-logbook` parses; clippy 0; full
  workspace green. Panel button → held for visual confirm.

## 2026-06-27 — WS-vs-TCP control-port parity audit (no gap)

- **Requirement/change:** audit another surface — does a `ControlCommand` reach the daemon on the
  TCP control port but not the WebSocket port (or vice versa)?
- **Finding:** **parity holds.** Both `lib.rs::handle_command` (TCP) and `ws.rs` parse the same
  `ControlCommand` enum, handle the identical 6 request-response commands inline (SubscribeSpectrum,
  GetConfig, ListMessages, GetMessage, SendMessage, DeleteMessage), and route everything else
  through the same `dispatch_command` → `apply_command_to_engine`. No command is reachable on one
  transport but not the other.
- **Design decision:** no code gap to fix; the only risk is *future* divergence (the two inline
  chains are duplicated). Added cross-referencing "keep in sync" comments to both handlers as a
  tripwire; a full consolidation into one shared request-response handler is noted as future
  hardening (low priority — no current gap).
- **Implementation:** comments in `crates/openpulse-daemon/src/lib.rs` and `ws.rs`.
- **Test results:** daemon builds; fmt clean; no behavior change.

## 2026-06-27 — CE-SSB gated off for OFDM-HOM (8PSK+) — a real ~6 dB regression

- **Requirement/change:** investigate the CE-SSB-on-OFDM cost surfaced while greening the baseline
  (CE-SSB clipping corrupted OFDM52+full-RS on a clean channel). Does it hurt any *shipped*
  OFDM-HOM+RS rung at marginal SNR?
- **Finding:** yes — CE-SSB was **net-harmful on OFDM-HOM**. `cessb_benefits` gated off 16QAM+ but
  still applied CE-SSB to **OFDM52-8PSK** (the shipped `hpx_ofdm_hf` SL7 rung, default-on). The
  peak-fair `cessb_power_evm` shows OFDM52-8PSK BER **0.0000 → 0.0026** (power gain doesn't recover
  the in-band clipping distortion), and a marginal-SNR AWGN sweep has it fail entirely with CE-SSB
  on (**12/12 → 0/12 at 12–16 dB**), decoding once gated off. CE-SSB is genuinely zero-cost only on
  the QPSK-subcarrier OFDM (OFDM16/OFDM52, BER 0→0). The team's own gating principle —
  "favourable raw BER notwithstanding, real-path decode breaks" — applies to 8PSK too.
- **Design decision:** add `8PSK` to the `cessb_benefits` exclusion (CE-SSB now applies only to
  QPSK-OFDM). The on-air +1.2 dB power result was on QPSK-OFDM and is unaffected.
- **Implementation:** `crates/openpulse-modem/src/engine.rs` (`cessb_benefits`).
- **Tests:** updated `cessb_power_evm::cessb_benefits_hold_*` and `cessb_engine::benefits_only_*`
  to assert 8PSK gated off; new real-path guard
  `channel_loopback::ofdm52_8psk_rs_decodes_at_operating_snr_with_default_cessb`.
- **Test results:** new guard 8/8 at 16 dB; cessb suites pass; full workspace **no failures**;
  clippy 0.

## 2026-06-27 — Config-schema completeness audit (defined-but-not-consumed)

- **Requirement/change:** audit another surface for the seam-gap class — config fields that exist
  (and are in the TOML template) but are never read, so setting them does nothing.
- **Design decision:** 72/79 fields consumed; the 7 dead ones are all in `[radio]` —
  `[radio.rig_a]` (never read; the primary rig is the top-level `[radio]`) and the `"generic"` CAT
  backend (`backend`/`serial_port`/`rig_file`, documented in the manual but unimplemented). Don't
  remove documented/planned schema and don't undertake the feature in an audit; instead mark them
  accurately so the config stops looking wired, and record the real fixes in the roadmap.
- **Implementation:** `crates/openpulse-config/src/lib.rs` (field docs + TOML template mark
  rig_a "currently unused" and the generic-backend fields "reserved — not yet implemented";
  corrected the repeater comment); `docs/dev/project/roadmap.md` "Config/feature gaps" entry.
- **Tests/results:** `openpulse-config` 9/9 (template still parses), clippy 0. The recently-added
  `[modem] notch_*`, `[qsy] auto_qsy_on_interference`, `[logbook] *` fields were each confirmed
  consumed by the daemon during the audit.

## 2026-06-27 — Auto-QSY end-to-end validation

- **Requirement/change:** validate the notch → in-band-interferer → auto-QSY loop end to end
  (the capstone of the notch arc, previously only unit-tested piecewise).
- **Design decision:** the TCP twin daemon harness can't inject a *standalone* interferer (channel
  models transform a signal, they don't generate a tone into B's silence), so validate the full
  logical loop deterministically via `ChannelSimHarness`: Station A confirms a persistent in-band
  tone through `accumulate_capture`, auto-QSY transmits a real `QSY_REQ`, it crosses the channel,
  Station B decodes it and `process_received_bytes` opens a responder session (+ `QsyIncoming`).
- **Implementation:** test only — `crates/openpulse-daemon/src/lib.rs`
  `auto_qsy_end_to_end_initiator_to_responder_over_rf`.
- **Tests/results:** the new test passes; daemon lib **29/29**; clippy 0; fmt clean. Remaining: a
  two-station **on-air** run (rpi53 + FT-991A / SDR) — genuine hardware, not reproducible here.

## 2026-06-27 — Automatic ADIF logbook (opt-in)

- **Requirement/change:** the roadmap-recorded feature — a per-QSO station log in ADIF for import
  into logging software / LoTW / eQSL, opt-in.
- **Design decision:** per-QSO (a connect→disconnect session), distinct from the per-frame
  `TxSessionLog`. ADIF writer in `openpulse-core` (pure, no time crate — Hinnant civil-date for
  UTC); a daemon `Logbook` helper holds in-flight QSO state and appends on disconnect, decoupled
  from the RF loop (io errors are logged, never propagated). Sourced from the `ConnectPeer`
  callsign, the active mode (→ `SUBMODE`, `MODE=DYNAMIC`), the last `SetFreq` (→ `FREQ`/`BAND`),
  UTC connect/disconnect timestamps, and station callsign/grid from config.
- **Implementation:** `crates/openpulse-core/src/adif.rs` (`AdifRecord`/`to_adif`, `utc_date_time`,
  `band_for_mhz`, header); `crates/openpulse-config` (`[logbook] enabled`/`adif_path`);
  `crates/openpulse-daemon/src/logbook.rs` (`Logbook`); daemon `ConnectPeer`/`DisconnectPeer`/
  `SetFreq` hooks + `server.rs` build from config.
- **Tests:** ADIF unit tests (record render, band map, UTC format, header); `Logbook` unit tests
  (write/append-no-dup-header, disabled/no-pending no-op); daemon integration
  (`connect_then_disconnect_writes_an_adif_logbook_record`); config default.
- **Test results:** core adif 4/4, daemon logbook+integration pass, full workspace **no failures**,
  `clippy --all-targets` **0 errors**. Follow-up: a control command + CLI/panel toggle/export
  (config-driven for now).

## 2026-06-27 — Green the test/clippy baseline (3 red items)

- **Requirement/change:** make `cargo test --workspace` and `clippy --all-targets` green (they had
  red items all session, undermining the "real green results" traceability rule).
- **Design decisions + findings (each probed by clean loopback before fixing):**
  - `cli_adaptive::adaptive_ofdm_hf_reaches_top_rung`: the `hpx_ofdm_hf` profile had `fec_modes:
    [None; 21]` and the `adaptive` command decoded with no FEC — but OFDM52-8PSK fails unprotected
    even on clean. Per-level FEC measured: OFDM16/OFDM52 base decode unprotected and *break* under
    full RS (padded 255-byte block spans too many OFDM symbols); OFDM52-8PSK+ need RS. → assign RS
    to SL7–SL10 only, and make the command apply `profile.fec_for(level)` via
    `transmit_with_fec_mode`/`receive_with_fec_mode`.
  - `repro::ofdm52_rs_clean_128b_engine`: red because **CE-SSB** (default-on PAPR conditioner,
    #521) clips OFDM52-base+full-RS past RS t=16. That combo is used by no profile (zero
    operational impact) and the shipped OFDM-HOM+RS rungs survive CE-SSB; the guard predates
    CE-SSB (#185) and tests the OFDM modulator path → disable CE-SSB in the guard, documenting the
    finding that CE-SSB is *not* zero-cost on every OFDM mode.
  - 3 testbench clippy `field_reassign_with_default` lints → struct-update syntax.
- **Implementation:** `crates/openpulse-core/src/profile.rs` (hpx_ofdm_hf fec_modes);
  `crates/openpulse-cli/src/commands/adaptive.rs` (per-level FEC); `apps/openpulse-testmatrix/
  tests/repro.rs` (CE-SSB off in the guard); `apps/openpulse-testbench/src/signal_path.rs` (lints).
- **Tests:** `cli_adaptive` (6), the repro guard, full workspace test + `clippy --all-targets`.
- **Test results:** `cli_adaptive` 6/6; the OFDM ladder climbs SL5→SL10 (6/6 frames, ~1153 bps);
  full `cargo test --workspace --exclude pki-tooling`: **no failures**; `clippy --all-targets`: **0 errors**.

## 2026-06-27 — SetFreq panel control + CLI rig-default fix

- **Requirement/change:** make CAT `SetFreq` reachable from the panel (the one parity item left
  panel-only after the prior round), and fix the CLI `set-freq` default that the daemon rejects.
- **Design decision:** the daemon's `SetFreq` handler only accepts `rig == "rigctld"` (single CAT
  target), not the display rig_a/rig_b labels — so no rig selector is needed. Panel: a `Freq:`
  DragValue in **kHz** (operator-ergonomic, HF-ranged 1500–30000) + a `Tune` button sending
  `freq_hz = round(kHz × 1000)` with `rig = "rigctld"`, placed next to the Mode selector. CLI:
  change the `set-freq --rig` default from the invalid `a` to `rigctld`.
- **Implementation:** `apps/openpulse-panel/src/app.rs` (Freq DragValue + Tune → `SetFreq`; new
  `freq_khz` field, default 14070.0); `crates/openpulse-cli/src/cli.rs` (`set-freq` default rig).
- **Tests:** panel build + clippy/fmt (GUI confirmed visually before merge); CLI build + `set-freq`
  parse/connection-stage reachability.
- **Test results:** panel builds, **0 clippy errors**, fmt clean; CLI builds, fmt clean, `set-freq`
  parses and reaches the connect stage. (PR #562.)

## 2026-06-27 — Control-surface parity (CLI + panel)

- **Requirement/change:** the control-surface audit (`docs/dev/project/roadmap.md` → "Control-surface
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

## 2026-07-13 — fix(js8/discovery): audit #2 — decode real off-air overs (time search)

- **Requirement/change:** the Fable loose-ends audit found (finding #2, confirmed) that the JS8 discovery
  decoder only ever searched slot-start offset 0, but a conforming over starts ~500 ms (`start_delay_ms`)
  into the slot — so real off-air JS8 could not decode **at any SNR**; the RX-MVP acceptance test passed
  only because it injected the signal at buffer offset 0. Regression links REQ-DISC-01/02, CAP-70.
- **Design decision:** add a **two-stage acquisition** to `decode_window` — a coarse time×freq grid then a
  per-candidate refine to full precision (`base_step_coarse` > `base_step` enables it; `0` keeps the old
  single-pass behaviour byte-identical, so every other caller is unchanged). `DecodeCfg` gains
  `base_step_coarse` + `min_offset`. Discovery's `decode_slot` now searches a ±0.75 s window around the
  expected `start_delay` (NTP is required, D5) at coarse freq — a naive fine full-slack scan measured
  ~19 s/decode (Pi-fatal); the two-stage version is ~1.4 s release / 3 s debug for the same decode.
- **Implementation:** `plugins/js8/src/decoder.rs` (`refine_sync`, two-stage `decode_window`, `DecodeCfg`
  fields), `crates/openpulse-discovery/src/runtime.rs` (`decode_slot` bounded window).
- **Tests:** `hears_a_station_that_starts_partway_into_the_slot` (heartbeat at 500 ms offset — fails at
  every SNR before the fix); the existing offset-0 decoder/discovery/daemon tests still pass unchanged.
- **Test results:** `cargo test -p js8-plugin -p openpulse-discovery --no-default-features` green (81 + 51);
  daemon discovery tests green; clippy + fmt clean.

## 2026-07-13 — fix(discovery): audit #4 — rendezvous timing/RxOnly cluster (Phase F)

- **Requirement/change:** the audit (finding #4, all three confirmed) found the shipped FF-15 Phase-F
  rendezvous non-functional: (a) `RENDEZVOUS_TIMEOUT_SLOTS = 8` was shorter than the Propose+Accept
  round-trip (the initiator aged during its own Propose TX and timed out before any reply could arrive);
  (b) the responder emitted `RendezvousAgreed` at Propose-recognition while its Accept was still queued,
  so the daemon's QSY schedule preempted and truncated the Accept's final frame; (c) `TxMode::RxOnly`
  transmitted a Propose because `maybe_transmit` popped the rendezvous queue before the RxOnly gate.
  Refs REQ-DISC-04/07, CAP-70.
- **Design decision:** (a) age the initiator only once its Propose has drained (`rendezvous_tx` empty), so
  the timeout counts genuine reply-wait slots; raise it to 16. (b) the responder withholds the agreement
  (`pending_responder_agreement`) until its Accept over has fully transmitted, then emits it — aligning
  the responder's QSY schedule with the initiator's Accept decode and never preempting the Accept. (c)
  move the RxOnly gate before the rendezvous-queue pop, and refuse `start_rendezvous` in RxOnly.
- **Implementation:** `crates/openpulse-discovery/src/runtime.rs`; `crates/openpulse-daemon/src/lib.rs`
  (`RENDEZVOUS_TIMEOUT_SLOTS`).
- **Tests:** new `rx_only_start_rendezvous_transmits_nothing` (c); `responder_accepts_a_proposal_and_agrees`
  rewritten to assert the agreement is deferred until the Accept is sent (b); `initiator_times_out_without_a_reply`
  now covers Propose-drain-then-wait (a); the two-runtime end-to-end updated for the deferred responder
  agreement. The prior F-3c-iv test drained TX fully under a manual clock, which is why it missed all three.
- **Test results:** `cargo test -p openpulse-discovery -p openpulse-daemon --no-default-features` green
  (52 discovery lib + 2 e2e; daemon discovery/rendezvous suites); clippy + fmt clean.

## 2026-07-13 — fix(js8): audit #3 — jsc_decompress u32 overflow on long high-group runs

- **Requirement/change:** the audit (finding #3, confirmed) found `jsc_decompress`'s inner fold
  (`j = j * C + …`) had no in-loop bound; ~11+ consecutive high JSC groups — reachable from the free-text
  of any CRC-12-valid decoded frame, on the discovery RX library path — overflow `u32` and panic in
  overflow-checks (debug/test/CI) builds, violating the no-panic-in-library-production-paths rule.
- **Design decision:** saturate the multiply/add and break as soon as the running index exceeds `SIZE`
  (an over-`SIZE` index is invalid and already handled after the loop). Byte-identical output for valid
  inputs (their `j` stays far below `u32::MAX`); safe termination for crafted/garbage inputs.
- **Implementation:** `plugins/js8/src/jsc.rs`.
- **Tests:** `long_high_group_run_does_not_overflow` (256 all-ones bits → one long high-group run; panics
  before the fix). Full JSC + ground-truth decode tests unchanged.
- **Test results:** `cargo test -p js8-plugin --no-default-features --lib jsc::` green; clippy + fmt clean.

## 2026-07-13 — fix(daemon): audit #5 — arm the PTT watchdog on every automatic TX path

- **Requirement/change:** the audit (finding #5, confirmed) found `ptt_asserted_at` was set only by the
  manual `PttAssert` command; the five automatic keying paths (station-ID, OTA ACK, OTA send, discovery
  beacon, filexfer drain) asserted/released directly and only `warn!`ed on release failure — so a transient
  rigctld/serial fault during any unattended transmission leaves the transmitter keyed with the software
  watchdog permanently blind to it. Safety backstop; §97.221 automatic-control control-point relevance.
- **Design decision:** arm `ptt_asserted_at` (the watchdog clock) at every automatic keying, and disarm it
  only on a **successful** release — a failed release leaves it armed so the next `check_ptt_watchdog` tick
  force-releases the still-keyed transmitter (the watchdog caller already re-releases hardware on fire). The
  two keying helpers (`transmit_beacon_with_ptt`, `ota_send_with_ptt`) take the watchdog clock as a param;
  the three inline sites use `runtime_state.ptt_asserted_at` directly.
- **Implementation:** `crates/openpulse-daemon/src/server.rs`.
- **Tests:** `automatic_tx_arms_the_watchdog_and_disarms_only_on_clean_release` with a `FlakyPtt` double
  (a failing-release PTT — also closes the audit's "no failing-PTT test double" gap): a failed release keeps
  the watchdog armed, a clean release disarms it.
- **Test results:** `cargo test -p openpulse-daemon --no-default-features` → 83 lib + integration green;
  clippy + fmt clean.

## 2026-07-13 — fix(daemon): audit #7 — fail-closed WebSocket control port

- **Requirement/change:** the audit (finding #7, confirmed) found the WebSocket control endpoint carries
  the *same* command protocol as the TCP port (`PttAssert`/`SendMessage`/`EnableRepeater`/…) but with **no
  authentication path**, and `spawn_ws` was called unconditionally — so with `require_auth = true` (or a
  non-loopback WS bind) any client reaching the WS port bypassed the Noise/PSK gate the TCP port enforces.
  The startup fail-closed check only inspected `tcp_bind_addr`. REQ-SEC-CTL-02.
- **Design decision:** WS has no auth transport, so the fail-closed action is to **not spawn** the
  unauthenticated WS listener whenever control auth is required for either bind (TCP needs auth, or the WS
  bind is itself non-loopback), with a clear warning pointing the operator at the Noise/PSK TCP port.
  Full Noise-over-WebSocket is a documented follow-up. Decision extracted to `ws_disabled_for_auth`.
- **Implementation:** `crates/openpulse-daemon/src/server.rs`.
- **Tests:** `ws_auth_gate_tests` — disabled when TCP requires auth (even if WS is loopback), disabled when
  the WS bind is non-loopback, enabled only when both are loopback and no auth is configured.
- **Test results:** `cargo test -p openpulse-daemon --no-default-features` green (86 lib); clippy + fmt clean.

## 2026-07-13 — fix(repeater): audit #6 — station-ID the cross-band repeater's transmitting rig

- **Requirement/change:** the audit (finding #6, confirmed) found the daemon's §97.119 station-ID timer
  only watches the *main* engine's `frames_transmitted`; `CrossBandRepeater` builds and transmits from a
  wholly separate `engine_tx`/rig-B pair with **zero ID logic** (PTT held for the whole full-duplex
  session) — exactly the automatically-controlled-station case (§97.221) the regulatory doc calls out.
  REQ-REG-04, CAP-70-adjacent regulatory.
- **Design decision:** give `CrossBandRepeater` its own `StationIdTimer` keyed off rig-B transmits.
  `RepeaterConfig` gains `callsign` + `id_interval_secs` (wired from `[station] callsign` /
  `auto_id_interval_secs`); an empty callsign or 0 interval disables auto-ID. After each relayed frame the
  timer is noted; when the interval elapses, `maybe_identify` transmits `DE <callsign>` on rig-B (keying
  its own PTT in half-duplex; reusing the held session PTT in full-duplex), then marks identified. Clock is
  injectable via `relay_one_frame_at(now_ms)` for deterministic tests.
- **Implementation:** `crates/openpulse-repeater/{src/lib.rs,src/config.rs,Cargo.toml}` (adds
  `openpulse-core` dep); `crates/openpulse-daemon/src/server.rs` + `lib.rs` build sites.
- **Tests:** `transmitting_rig_is_station_identified_when_the_interval_elapses` — a plain relay keys once;
  a relay after the interval keys a second time (the ID's own PTT). Existing repeater tests updated with
  `..Default::default()` (empty callsign → no ID → behaviour unchanged).
- **Test results:** `cargo test -p openpulse-repeater --no-default-features` → 7 green;
  `cargo build --workspace --no-default-features` clean; clippy + fmt clean.

## 2026-07-13 — fix(ardop): audit #8 — data port frame loss (backpressure + Lagged handling)

- **Requirement/change:** the audit (finding #8, confirmed, both directions) found the ARDOP data port
  silently dropped frames. **TX:** `try_send` on the 64-deep SyncSender dropped the frame with only a
  server-side `warn!` (the client never learns), so any >64-frame burst (a normal Winlink message) lost
  data. **RX:** the `Ok(data) = rx_data.recv()` select arm did not handle `Err(Lagged)`, so a broadcast
  overflow stalled the receive loop until the client next sent, instead of skipping + warning.
- **Design decision:** mirror the already-tested `openpulse-kiss` pattern. TX: replace `try_send` with a
  blocking `SyncSender::send` on `spawn_blocking` — natural backpressure throttles the client's TCP reader
  so the burst is delivered in full (worker-gone → close the client). RX: match `recv()` explicitly,
  handling `RecvError::Lagged(n)` (log + continue) and `Closed` (return).
- **Implementation:** `crates/openpulse-ardop/src/data.rs`.
- **Tests:** `data_port_backpressure_burst_stays_connected_and_ordered` — a 128-frame burst (> queue depth)
  produces no client-side write error and the frames that arrive are strictly in-order and intact (no
  reordering/corruption; broadcast lag may thin the echo, which is by-design). Existing round-trip tests
  unchanged.
- **Test results:** `cargo test -p openpulse-ardop --no-default-features` → 23 green; clippy + fmt clean.

## 2026-07-13 — fix(bpsk): audit #1 — cancel crossfade ISI on the differential demod

- **Requirement/change:** the audit (finding #1, confirmed by verification here) found BPSK — the primary
  weak-signal fallback family (SL2–SL4 on every HF ladder) — has the uncancelled crossfade-ISI defect
  already fixed for QPSK (#695) and 8PSK (#696), never ported. The overlapping half-Hann modulator is a
  crossfade, so the one-slot matched filter recovers `r_k = a_k + β·a_{k+1}` with β = Σ(w_head·w_tail)/
  Σw_tail² = **1/3** (same integrals as rectangular QPSK). Because BPSK is NRZI-**differential**, that
  `+β` term becomes a constant positive bias in the dot product `r_k·r_{k-1}` (a_k²=1), eroding the
  flip-bit margin by several dB. Refs REQ-DISC-adjacent modem; primary fallback.
- **Design decision:** port `cancel_crossfade_isi` (stable backward substitution `a_k = r_k − β·a_{k+1}`;
  +0.5 dB noise enhancement, far less than the margin recovered) to the BPSK IQ stream *before*
  differential detection, gated to the **crossfade (non-RRC) path only** — the `-RRC` modes use Gardner+LMS
  and do not crossfade. Applied on both the hard `bpsk_demodulate` and the soft `bpsk_demodulate_soft`.
- **Implementation:** `plugins/bpsk/src/demodulate.rs`.
- **Tests:** `crossfade_cancellation_lowers_awgn_ber` — a deterministic fixed-seed AWGN BER measurement;
  **A/B-verified during development: BER 3.33 % with the cancellation disabled → well under the 2 % guard
  with it** (a genuine fail-without-fix guard). All existing BPSK plugin (25) + modem bpsk_hardening (18)
  + fec_loopback (12) + channel_loopback (32) tests pass unchanged.
- **Test results:** `cargo test -p bpsk-plugin --no-default-features` → 25 green; modem BPSK paths green;
  clippy + fmt clean.

## 2026-07-13 — fix(discovery): audit #9 — make the clock-skew TX gate live

- **Requirement/change:** the audit (finding #9, confirmed) found `Js8Clock::set_drift_bias_ms` had **no
  production caller** (the documented `clock_mut()` seam was never used), so `drift_bias_ms` was permanently
  0 — the advertised "±2 s clock-skew" beacon-TX safety gate (D5) could never trip and the operator-facing
  `DiscoveryStatus.drift_bias_ms` was always a false 0. Refs REQ-DISC-05.
- **Design decision:** feed each decoded frame's timing error into an EWMA (`Js8Clock::observe_dt_ms`,
  α=1/8). `dt = (start_delay − sample_offset)/8` ms: a conforming station starts `start_delay` into the
  slot, so a decode placed later means our clock is fast (negative bias). Smoothing averages per-station
  timing error + capture jitter, leaving our systematic offset; the magnitude drives `tx_allowed`. Small
  values barely affect `corrected()` (15 s slots), so RX slot alignment is unaffected. **Documented
  coupling:** the observable dt range is bounded by the decoder's ±0.75 s slot-start search window (the
  #2 fix, Pi-CPU-bounded) — a skew beyond that yields no decodes rather than a reading; full ±2 s
  detection would need a wider (Pi-costlier) search and is deferred.
- **Implementation:** `crates/openpulse-discovery/src/scheduler.rs` (`observe_dt_ms`),
  `crates/openpulse-discovery/src/runtime.rs` (`decode_slot` feeds it).
- **Tests:** `observe_dt_converges_toward_the_offset_and_can_trip_the_gate` (EWMA converges to −600 ms and
  a sustained >2 s skew trips the gate); the offset regression test now asserts the drift readout is live
  and ~0 for an on-time station.
- **Test results:** `cargo test -p openpulse-discovery --no-default-features` → 53 green; clippy + fmt clean.

## 2026-07-13 — fix(daemon): audit #14 — validate SetMode/SetConfig before mutating shared state

- **Requirement/change:** the audit (finding #14, confirmed) found `dispatch_command` (the per-client
  TCP/WS handler) wrote `active_mode` **unconditionally** for `SetMode`/`SetConfig` and returned
  `CommandResponse::ok()`; the only validation (`apply_command_to_engine`) ran later and merely logged a
  `CommandError` without rollback. A typo'd mode string silently deafened RX + station-ID until a valid
  `SetMode` arrived, with the client told its bad command succeeded.
- **Design decision:** capture the registered plugin mode names once at `ControlServer::spawn` (from
  `engine.plugins().list()`) as a read-only `ValidModes` set, thread it to `dispatch_command` (both the
  TCP and WebSocket paths), and reject an unknown mode with `CommandResponse::err` **before** any shared
  write and **before** forwarding the command. An empty set (tests with no registry) skips validation.
- **Implementation:** `crates/openpulse-daemon/src/lib.rs` (`ValidModes`, `ClientCtx`,
  `ControlServerHandle`, `dispatch_command`), `src/ws.rs` (`WsShared`/`WsClientCtx`), `src/server.rs`
  (WsShared wiring).
- **Tests:** `dispatch_rejects_unknown_mode_without_mutating_state` — an unknown mode returns an error and
  leaves `active_mode` untouched; a registered mode applies. `control_port` mode-switch test's engine now
  registers QPSK so it selects a genuinely-registered mode.
- **Test results:** `cargo test -p openpulse-daemon --no-default-features` → all green (7 groups); clippy
  + fmt clean.

## 2026-07-13 — fix(daemon/filexfer): audit #13 — reject inconsistent offer geometry

- **Requirement/change:** the audit (finding #13, confirmed) found the receive-side size gate + per-peer
  quota key only on `offer.file_size`, while `offer.block_count` is a raw uncross-checked `u16`. A crafted
  offer (e.g. `file_size = 100`, `block_count = 65535`) decouples the size/quota check from the bytes
  actually reassembled and written. Feature is off by default (`require_verified_peer = true`), but the
  geometry was never validated.
- **Design decision:** validate the offer's block geometry up front in `on_offer`, before the quota check
  and `decide()`: reject (`Reason::TooLarge`) unless `block_size` is within the protocol window
  `[MIN_BLOCK_SIZE, MAX_BLOCK_SIZE]` and `block_count == block_count(file_size, block_size)` (the existing
  helper). With geometry consistent, the `file_size` gate now also bounds the reassembled/written bytes.
- **Implementation:** `crates/openpulse-daemon/src/filexfer.rs` (`offer_geometry_ok`, wired into the
  decision chain).
- **Tests:** `offer_geometry_check_rejects_inconsistent_block_count` — a well-formed offer passes; an
  inflated `block_count` and a sub-minimum `block_size` are both rejected.
- **Test results:** `cargo test -p openpulse-daemon --no-default-features filexfer` → green; twin +
  filexfer suites unchanged; clippy + fmt clean.

## 2026-07-13 — docs: audit #43/#46/#47 — fix documentation drift

- **Requirement/change:** the audit flagged several stale docs. #47: `CLAUDE.md`'s audio-backend note gave
  a failing flag (`--features cpal`) for `openpulse-cli` and an inverted default — the CLI's feature is
  `cpal-backend`, **on by default**. #43: the root `CHANGELOG.md` is frozen at 0.2.0 (the v0.4.0 roll only
  touched `docs/dev/project/changelog.md`), so two changelogs diverged. #46: the README repository-layout
  table omitted 6 shipped crates (`openpulse-discovery`, `-filexfer`, `-keystore`, `-linksec`,
  `-freedv-auth`, `apps/openpulse-linksim`) and 2 shipped plugins (`js8`, `pilot`).
- **Design decision:** #47 correct the CLI exception in the sharp-edge note. #43 add a prominent
  canonical-changelog pointer at the top of root `CHANGELOG.md` (directing to `docs/dev/project/changelog.md`
  + `docs/releasenotes.md` for v0.2.1–v0.4.0) and mark its `[Unreleased]` as superseded, rather than
  hand-reconciling four versions of diverged content. #46 add the missing crate/plugin rows.
- **Implementation:** `CLAUDE.md`, `CHANGELOG.md`, `README.md`.
- **Tests:** docs only — no code change. (#44 cli-guide daemon section + #45 README ladder nuance left as
  lower-value follow-ups; the README feature text already reflects the SC-FDMA→OFDM re-seat.)

## 2026-07-13 — fix(modem): audit #11 — don't re-apply the InputCapture seam per decode_burst slice

- **Requirement/change:** the audit (finding #11, confirmed + A/B-verified) found `accumulate_routed`
  already routes each captured window through the `InputCapture` front-end seam (DC-block / notch / DCD /
  AGC) before appending to `rx_burst`; `decode_burst`'s per-offset `decode_attempt` → `receive_from_samples`
  then routes the *already-processed* burst through the same seam **again per scan slice**. Harmless with
  AGC/notch off (both default off), but with AGC on the stateful gain loop re-normalises already-normalised
  audio each slice — distorting the noise-var-calibrated LLR scale QAM/OFDM/SC-FDMA depend on — and the DCD
  re-latches from mid-burst slices.
- **Design decision:** add an `input_prerouted` flag; `decode_burst` sets it around its scan (restored on
  every exit via an inner helper) so the nested `route_audio_stage(InputCapture)` becomes a pass-through
  (skips the DC/notch/DCD/AGC block). With AGC/notch off the behaviour is bit-identical.
- **Implementation:** `crates/openpulse-modem/src/engine.rs` (`input_prerouted`, `decode_burst`/
  `decode_burst_inner`, InputCapture guard).
- **Tests:** `decode_burst_does_not_reapply_agc_per_scan_slice` (`agc_blocks_processed` stays flat across a
  `decode_burst` scan on a pre-routed burst) — **A/B-verified: fails with the guard removed**.
- **Test results:** `cargo test -p openpulse-modem --no-default-features` agc/notch/channel/fec loopbacks
  green; clippy + fmt clean.

## 2026-07-13 — fix(mesh): audit #10 (part) — refuse to run the mesh daemon as N0CALL

- **Requirement/change:** the audit (finding #10, confirmed) found `openpulse-mesh` beacons + relays
  automatically (60 s cadence) with **no N0CALL startup refusal** — unlike `openpulse-daemon` / `-tui`,
  which exit if the configured callsign is the `N0CALL` placeholder. §97.119: a station must transmit its
  own valid call sign.
- **Design decision:** add the same N0CALL guard the daemon uses, after the `[mesh] enabled` check (only an
  enabled mesh transmits) — `anyhow::bail!` before building the engine/daemon.
- **Implementation:** `crates/openpulse-mesh/src/main.rs`.
- **Scope note:** the finding's ARDOP/KISS half (transmit before `MYID` is set) is **deferred** — those
  TNCs take their callsign from the host `MYID` command at runtime, so a hard config-time refusal risks
  breaking legitimate Pat/Winlink workflows; enforcing MYID-before-TX belongs with the TNC session logic
  (the "front-ends don't drive sessions" area).
- **Test results:** `cargo build/clippy -p openpulse-mesh --no-default-features` clean (a `main()` startup
  guard, consistent with the daemon's untested guard).

## 2026-07-13 — fix(panel): audit (low tier) — panel mode list omitted 12 PILOT modes

- **Requirement/change:** the audit found the panel's hardcoded `MODES` list carried only the 8 PILOT-500
  variants while the `pilot` plugin registers 20 (adding the PILOT-{QPSK,8PSK,16QAM,32APSK}1000, -1000-RRC,
  and -2000-RRC families) — so an operator could not select 12 registered modes. Same drift class as the
  earlier mode-list fix (PR #321).
- **Design decision:** add the 12 missing PILOT-1000/2000 mode strings to `apps/openpulse-panel/src/ui.rs`
  `MODES`, matching the plugin's registration.
- **Test results:** `cargo build/clippy -p openpulse-panel --no-default-features` clean.
- **Scope note:** the `SetTxAttenuation { band }` drop is **deferred** — the command carries an optional
  `band`, but the engine has only a single global `tx_attenuation_db` (no per-band store), so honoring it
  is a feature, not a fix.

## 2026-07-13 — chore(audit): low-tier cleanup batch (stubs / parity / doc honesty)

- **Requirement/change:** a grouped pass over confirmed low-severity audit findings — dead/misleading APIs,
  control-surface parity, and doc honesty — each too small for its own PR.
- **Changes:**
  - **Panel `ecc_rate` fabricated 0.00 %** — the daemon reports `ecc_rate = None` (computes none), but the
    panel showed a fabricated `0.00 %`. Made `PanelState.ecc_rate` `Option<f32>`, render `"—"` when `None`,
    and only push real samples into the trend sparkline. Test
    `ecc_rate_none_is_not_fabricated_and_leaves_the_trend_empty`.
  - **`queue_message_type_c` misleading doc** — the pub API's doc claimed "Winlink-compatible" while
    `compress.rs` states external Type C compat is UNVERIFIED (LH5 vs FBB's Okumura LZHUF). Corrected the
    doc to match reality and note it has no in-tree caller yet.
  - **Config template stale mode list** — the `[modem]` "Available:" comment omitted OFDM52-HOM, PILOT-*,
    RRC, and SC-FDMA-HOM families. Replaced the unmaintainable literal list with the mode families +
    a pointer to `docs/mode-fec-ladder.md` (authoritative).
  - **`[discovery]` reserved fields** — `query_new_stations` / `max_queries_per_10min` are accepted-but-
    unused (directed INFO queries unimplemented); the template now says so.
- **Test results:** `cargo test -p openpulse-panel -p openpulse-b2f -p openpulse-config
  --no-default-features` green; clippy + fmt clean.
- **Still deferred (feature-not-a-fix / substantial):** `SetTxAttenuation { band }` (no per-band engine
  store); route-discovery wire codecs with no originator; `transmit_iq` seam bypass (test-only); discovery
  `server::run`-level test (#15); cli-guide daemon section (#44).

## 2026-07-13 — fix(bpsk): audit #1 follow-up — keep crossfade cancellation off the soft path

- **Requirement/change:** the full-workspace test gate caught a regression from the #1 crossfade-ISI fix
  (#821): `llr_calibration::a_deeply_faded_extra_attempt_does_not_hurt` failed (plus_faded −2.0 dB vs
  two_good −3.0 dB, needs ≤ −2.5). #821 applied `cancel_crossfade_isi` to **both** the hard and the soft
  BPSK demod. BPSK is *differential*, so on the soft path the backward-substitution recursion **inflates**
  the noise LLRs of a deeply-faded attempt instead of suppressing them — breaking the 1/σ² LLR calibration
  HARQ MAP combining depends on (`receive_with_llr_combining`).
- **Design decision:** apply the cancellation on the **hard differential path only** (`bpsk_demodulate`),
  where it restores the decision margin (the A/B-verified BER win in #821 uses that path), and **not** on
  the soft path (`bpsk_demodulate_soft`), preserving the soft-LLR combining scale. (Contrast QPSK/8PSK,
  which are coherent and correctly cancel on both paths.)
- **Implementation:** `plugins/bpsk/src/demodulate.rs` (revert the soft-path `cancel_crossfade_isi` call).
- **Tests:** `llr_calibration` (2/2) now green; the #1 hard-path `crossfade_cancellation_lowers_awgn_ber`
  still passes; bpsk_hardening (18) / fec_loopback (12) / channel_loopback (32) green.
- **Test results:** full `cargo test --workspace --no-default-features` now 0 failures (was 1); fmt +
  clippy clean; benchmark gate `true` (10/10, mean_transitions 5.1).
