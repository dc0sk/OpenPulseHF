---
project: openpulsehf
doc: docs/requests/code-review-output.md
status: complete
created: 2026-05-11
last_updated: 2026-05-11
reviewer: GitHub Copilot (GPT-5.4)
request_reference: docs/requests/code-review.md
---

# OpenPulseHF Review — 2026-05-11

Reference: [docs/requests/code-review.md](code-review.md)

## Scope and method

This review is based on source inspection across the areas requested in [docs/requests/code-review.md](code-review.md), plus focused validation runs:

- `cargo test -p openpulse-core --test rate_adaptation`
- `cargo test -p openpulse-core --test pq_handshake_integration`
- `cargo test -p openpulse-qsy --test qsy_session`

I did not run the compatibility checks that require external peers or services (Pat, APRS client, Winlink CMS).

## Executive summary

- Major: both TCP ingress surfaces still allow unbounded buffering before validation. ARDOP reads arbitrary-length command lines, and KISS accumulates bytes until the next `FEND` before applying its 255-byte payload limit.
- Major: the ARDOP and KISS bridge workers still use `Mutex::lock().unwrap()` and suppress modem transmit failures, so a single panic poisons the worker and a failed TX can disappear without caller feedback.
- Major: `verify_manifest()` authenticates the manifest envelope but does not verify the received payload against `payload_hash`, so callers can treat a tampered transfer as valid if they forget the second check.
- Major: `RelayForwarder` replay suppression is TTL-only with no capacity bound, so unique nonces can grow memory without limit inside the TTL window.
- Major: dependency resolution for the PQ stack is still non-reproducible because `ml-kem` is not exactly pinned and [../../.gitignore](../../.gitignore#L3) excludes `Cargo.lock`.
- Several high-risk items from the brief checked out in current code and tests: PQ ACK session binding, SL2 fallback semantics, signed QSY frames, B2F timeout gating, and FEC layer ordering.

## Findings by topic

### 1. Architecture and design

- Minor: [../../crates/openpulse-modem/src/channel_sim.rs](../../crates/openpulse-modem/src/channel_sim.rs#L28) is a one-way routing harness, not a full-duplex two-station channel model. `route()` drains samples from the TX loopback and fills the RX loopback only once per call, with no concurrent reverse path or shared timebase ([../../crates/openpulse-modem/src/channel_sim.rs](../../crates/openpulse-modem/src/channel_sim.rs#L56), [../../crates/openpulse-modem/src/channel_sim.rs](../../crates/openpulse-modem/src/channel_sim.rs#L57), [../../crates/openpulse-modem/src/channel_sim.rs](../../crates/openpulse-modem/src/channel_sim.rs#L59)). That is acceptable for current one-direction loopback tests, but it does not validate full-duplex timing behavior.
  Proposed mitigation: either rename/document it as a one-way harness, or extend it to schedule both directions against a shared simulated clock.

### 2. Security review

- Major: the ARDOP command port accepts unbounded line input. `handle_client()` passes user data straight into `BufReader::read_line(&mut line)` with no maximum line length or `take()` guard ([../../crates/openpulse-ardop/src/command.rs](../../crates/openpulse-ardop/src/command.rs#L26), [../../crates/openpulse-ardop/src/command.rs](../../crates/openpulse-ardop/src/command.rs#L37), [../../crates/openpulse-ardop/src/command.rs](../../crates/openpulse-ardop/src/command.rs#L38)). A long line from a local process, or from a misbound listener, can force unbounded allocation before dispatching any command.
  Proposed fix: cap line length at the reader boundary and drop the connection once the cap is exceeded.

- Major: the KISS server enforces `MAX_PAYLOAD_BYTES` too late. The 255-byte limit is only checked after `read_kiss_frame()` has already pushed bytes into a growing `Vec` until the next `FEND` ([../../crates/openpulse-kiss/src/server.rs](../../crates/openpulse-kiss/src/server.rs#L15), [../../crates/openpulse-kiss/src/server.rs](../../crates/openpulse-kiss/src/server.rs#L47), [../../crates/openpulse-kiss/src/server.rs](../../crates/openpulse-kiss/src/server.rs#L93), [../../crates/openpulse-kiss/src/server.rs](../../crates/openpulse-kiss/src/server.rs#L110)). A peer that withholds `FEND` can therefore grow memory without bound.
  Proposed fix: enforce a hard frame-body cap inside `read_kiss_frame()` and abort the frame or connection once the cap is crossed.

- Major: manifest verification is not fail-closed. `verify_manifest()` returns success once the signature matches the manifest body, and the doc comment explicitly pushes `payload_hash` verification onto the caller ([../../crates/openpulse-core/src/manifest.rs](../../crates/openpulse-core/src/manifest.rs#L89), [../../crates/openpulse-core/src/manifest.rs](../../crates/openpulse-core/src/manifest.rs#L90), [../../crates/openpulse-core/src/manifest.rs](../../crates/openpulse-core/src/manifest.rs#L92)). That means the API cannot guarantee payload integrity on its own.
  Proposed fix: add a verification path that takes the received payload bytes and checks signature plus hash together, then make callers use that combined API by default.

- Minor: the shipped config surface still exposes `qsy.allow_trustlevels`, but the config crate marks it as “not yet enforced by the QSY session layer” ([../../crates/openpulse-config/src/lib.rs](../../crates/openpulse-config/src/lib.rs#L50), [../../crates/openpulse-config/src/lib.rs](../../crates/openpulse-config/src/lib.rs#L51)). The CLI currently only prints that field in `qsy status` ([../../crates/openpulse-cli/src/commands/qsy.rs](../../crates/openpulse-cli/src/commands/qsy.rs#L79), [../../crates/openpulse-cli/src/commands/qsy.rs](../../crates/openpulse-cli/src/commands/qsy.rs#L80), [../../crates/openpulse-cli/src/commands/qsy.rs](../../crates/openpulse-cli/src/commands/qsy.rs#L83)). The session layer can enforce trust allowlists when it is given a `QsyPolicy`, but that config is not wired into a production responder path in the inspected code.
  Proposed fix: either wire config parsing into the real QSY responder entry point, or clearly mark the field experimental in user-facing docs.

### 3. Correctness and edge cases

- No major defect found in the requested PQ handshake, rate-adaptation fallback, QSY signing, or RS length-prefix paths.
- Confirmed: `verify_pq_conack()` rejects mismatched session IDs ([../../crates/openpulse-core/src/pq_handshake.rs](../../crates/openpulse-core/src/pq_handshake.rs#L424), [../../crates/openpulse-core/src/pq_handshake.rs](../../crates/openpulse-core/src/pq_handshake.rs#L432)), and the integration suite covers that case ([../../crates/openpulse-core/tests/pq_handshake_integration.rs](../../crates/openpulse-core/tests/pq_handshake_integration.rs#L251)).
- Confirmed: `AckDown` floors at SL2 and only the third consecutive NACK at SL2 falls back to SL1 ([../../crates/openpulse-core/src/rate.rs](../../crates/openpulse-core/src/rate.rs#L204), [../../crates/openpulse-core/src/rate.rs](../../crates/openpulse-core/src/rate.rs#L221), [../../crates/openpulse-core/src/rate.rs](../../crates/openpulse-core/src/rate.rs#L237)), with explicit regression tests ([../../crates/openpulse-core/tests/rate_adaptation.rs](../../crates/openpulse-core/tests/rate_adaptation.rs#L60), [../../crates/openpulse-core/tests/rate_adaptation.rs](../../crates/openpulse-core/tests/rate_adaptation.rs#L98)).
- Confirmed: signed QSY frames go through `verify_line()` before decoding ([../../crates/openpulse-qsy/src/frame.rs](../../crates/openpulse-qsy/src/frame.rs#L164), [../../crates/openpulse-qsy/src/frame.rs](../../crates/openpulse-qsy/src/frame.rs#L185), [../../crates/openpulse-qsy/src/frame.rs](../../crates/openpulse-qsy/src/frame.rs#L186)), and the session suite covers signature tampering plus the no-overlap vote path ([../../crates/openpulse-qsy/tests/qsy_session.rs](../../crates/openpulse-qsy/tests/qsy_session.rs#L49), [../../crates/openpulse-qsy/tests/qsy_session.rs](../../crates/openpulse-qsy/tests/qsy_session.rs#L217)).
- Confirmed: the disjoint-candidate QSY path now sends an explicit reject instead of hanging the peer ([../../crates/openpulse-qsy/src/session.rs](../../crates/openpulse-qsy/src/session.rs#L306), [../../crates/openpulse-qsy/src/session.rs](../../crates/openpulse-qsy/src/session.rs#L327), [../../crates/openpulse-qsy/src/session.rs](../../crates/openpulse-qsy/src/session.rs#L329)).

### 4. Performance and resource usage

- Major: `RelayForwarder` uses a plain `HashMap` replay table with TTL eviction but no capacity limit. The table starts at `HashMap::new()`, inserts every unseen `(session_id, nonce)`, and only shrinks via `evict_expired()` ([../../crates/openpulse-core/src/relay.rs](../../crates/openpulse-core/src/relay.rs#L246), [../../crates/openpulse-core/src/relay.rs](../../crates/openpulse-core/src/relay.rs#L255), [../../crates/openpulse-core/src/relay.rs](../../crates/openpulse-core/src/relay.rs#L270), [../../crates/openpulse-core/src/relay.rs](../../crates/openpulse-core/src/relay.rs#L308), [../../crates/openpulse-core/src/relay.rs](../../crates/openpulse-core/src/relay.rs#L325)). A sender that rotates nonces faster than the TTL window expires can therefore drive unbounded memory growth.
  Proposed fix: add a hard capacity cap, reject beyond-capacity inserts, or move to an LRU/age-bucket structure with predictable bounds.

- No evidence found that B2F driver reads are unbounded by timeout. `connect()` applies read timeouts to both sockets, and `run_irs()` wraps the connect wait in an explicit timeout window ([../../crates/openpulse-b2f-driver/src/lib.rs](../../crates/openpulse-b2f-driver/src/lib.rs#L62), [../../crates/openpulse-b2f-driver/src/lib.rs](../../crates/openpulse-b2f-driver/src/lib.rs#L63), [../../crates/openpulse-b2f-driver/src/lib.rs](../../crates/openpulse-b2f-driver/src/lib.rs#L125), [../../crates/openpulse-b2f-driver/src/lib.rs](../../crates/openpulse-b2f-driver/src/lib.rs#L126), [../../crates/openpulse-b2f-driver/src/lib.rs](../../crates/openpulse-b2f-driver/src/lib.rs#L127), [../../crates/openpulse-b2f-driver/src/cmd.rs](../../crates/openpulse-b2f-driver/src/cmd.rs#L61), [../../crates/openpulse-b2f-driver/src/data.rs](../../crates/openpulse-b2f-driver/src/data.rs#L45)).

### 5. Code quality and maintainability

- Major: both bridge workers are brittle to mutex poison and still collapse on `unwrap()`. The ARDOP worker locks the modem with `bridge.engine.lock().unwrap()` and also locks the relay forwarder with `fwd_arc.lock().unwrap()` ([../../crates/openpulse-ardop/src/bridge.rs](../../crates/openpulse-ardop/src/bridge.rs#L102), [../../crates/openpulse-ardop/src/bridge.rs](../../crates/openpulse-ardop/src/bridge.rs#L184)). The KISS worker does the same on both receive and relay-forward paths ([../../crates/openpulse-kiss/src/bridge.rs](../../crates/openpulse-kiss/src/bridge.rs#L102), [../../crates/openpulse-kiss/src/bridge.rs](../../crates/openpulse-kiss/src/bridge.rs#L116), [../../crates/openpulse-kiss/src/bridge.rs](../../crates/openpulse-kiss/src/bridge.rs#L156)). One panic inside engine or plugin code poisons the mutex and turns subsequent worker iterations into panics.
  Proposed fix: recover from poison with `into_inner()` and isolate plugin panics with `catch_unwind` or a process boundary if plugin isolation is a hard requirement.

- Major: modem transmit failures are still silently discarded in the bridge layer. In ARDOP, the worker computes `tx_result` and only branches on `is_ok()` without logging or surfacing the error ([../../crates/openpulse-ardop/src/bridge.rs](../../crates/openpulse-ardop/src/bridge.rs#L104), [../../crates/openpulse-ardop/src/bridge.rs](../../crates/openpulse-ardop/src/bridge.rs#L106), [../../crates/openpulse-ardop/src/bridge.rs](../../crates/openpulse-ardop/src/bridge.rs#L109)). In KISS, transmit is discarded entirely with `let _ = ...transmit(...)` ([../../crates/openpulse-kiss/src/bridge.rs](../../crates/openpulse-kiss/src/bridge.rs#L101)). The RX broadcast sends are intentionally best-effort, but TX errors should not disappear.
  Proposed fix: emit a structured bridge error event and log the modem failure so the caller can fail fast instead of hanging.

- Major: the PQ dependency surface is still not reproducible across environments. `ml-dsa` is pinned exactly, but `ml-kem` is still a floating `0.3` dependency ([../../crates/openpulse-core/Cargo.toml](../../crates/openpulse-core/Cargo.toml#L19), [../../crates/openpulse-core/Cargo.toml](../../crates/openpulse-core/Cargo.toml#L20)), and [../../.gitignore](../../.gitignore#L3) excludes `Cargo.lock` entirely.
  Proposed fix: pin `ml-kem` to an exact version and commit `Cargo.lock`, or otherwise document and enforce a reproducible dependency strategy in CI.

### 6. Compatibility verification

- Unverified in this review: Pat against `openpulse-tnc`, APRS/Dire Wolf against `openpulse-kisstnc`, and a live Winlink CMS delivery run. Those require external peers or credentials that were not available in this environment.

## Requested hotspot verdicts

| Area | Verdict | Evidence |
|---|---|---|
| Plugin isolation / bridge panic containment | Major finding | [../../crates/openpulse-ardop/src/bridge.rs](../../crates/openpulse-ardop/src/bridge.rs#L102), [../../crates/openpulse-kiss/src/bridge.rs](../../crates/openpulse-kiss/src/bridge.rs#L102) |
| SAR and FEC ordering | Confirmed | [../../crates/openpulse-modem/src/engine.rs](../../crates/openpulse-modem/src/engine.rs#L780), [../../crates/openpulse-modem/src/engine.rs](../../crates/openpulse-modem/src/engine.rs#L817), [../../crates/openpulse-modem/src/engine.rs](../../crates/openpulse-modem/src/engine.rs#L959), [../../crates/openpulse-modem/src/engine.rs](../../crates/openpulse-modem/src/engine.rs#L998), [../../crates/openpulse-modem/src/engine.rs](../../crates/openpulse-modem/src/engine.rs#L1050), [../../crates/openpulse-modem/src/engine.rs](../../crates/openpulse-modem/src/engine.rs#L1089), [../../crates/openpulse-modem/src/engine.rs](../../crates/openpulse-modem/src/engine.rs#L1147), [../../crates/openpulse-modem/src/engine.rs](../../crates/openpulse-modem/src/engine.rs#L1178), [../../crates/openpulse-modem/src/engine.rs](../../crates/openpulse-modem/src/engine.rs#L1423), [../../crates/openpulse-modem/src/engine.rs](../../crates/openpulse-modem/src/engine.rs#L1450) |
| Channel simulation fidelity | Minor limitation | [../../crates/openpulse-modem/src/channel_sim.rs](../../crates/openpulse-modem/src/channel_sim.rs#L28), [../../crates/openpulse-modem/src/channel_sim.rs](../../crates/openpulse-modem/src/channel_sim.rs#L56) |
| PQ handshake session binding | Confirmed | [../../crates/openpulse-core/src/pq_handshake.rs](../../crates/openpulse-core/src/pq_handshake.rs#L424), [../../crates/openpulse-core/src/pq_handshake.rs](../../crates/openpulse-core/src/pq_handshake.rs#L432), [../../crates/openpulse-core/tests/pq_handshake_integration.rs](../../crates/openpulse-core/tests/pq_handshake_integration.rs#L251) |
| QSY signed-frame enforcement | Confirmed | [../../crates/openpulse-qsy/src/frame.rs](../../crates/openpulse-qsy/src/frame.rs#L164), [../../crates/openpulse-qsy/src/frame.rs](../../crates/openpulse-qsy/src/frame.rs#L185), [../../crates/openpulse-qsy/tests/qsy_session.rs](../../crates/openpulse-qsy/tests/qsy_session.rs#L49) |
| Manifest verification ordering | Major finding | [../../crates/openpulse-core/src/manifest.rs](../../crates/openpulse-core/src/manifest.rs#L89), [../../crates/openpulse-core/src/manifest.rs](../../crates/openpulse-core/src/manifest.rs#L92) |
| ARDOP / KISS malformed-input robustness | Major finding | [../../crates/openpulse-ardop/src/command.rs](../../crates/openpulse-ardop/src/command.rs#L38), [../../crates/openpulse-kiss/src/server.rs](../../crates/openpulse-kiss/src/server.rs#L93), [../../crates/openpulse-kiss/src/server.rs](../../crates/openpulse-kiss/src/server.rs#L110) |
| RelayForwarder replay-table growth | Major finding | [../../crates/openpulse-core/src/relay.rs](../../crates/openpulse-core/src/relay.rs#L246), [../../crates/openpulse-core/src/relay.rs](../../crates/openpulse-core/src/relay.rs#L308), [../../crates/openpulse-core/src/relay.rs](../../crates/openpulse-core/src/relay.rs#L325) |
| SL2 ChirpFallback semantics | Confirmed | [../../crates/openpulse-core/src/rate.rs](../../crates/openpulse-core/src/rate.rs#L204), [../../crates/openpulse-core/tests/rate_adaptation.rs](../../crates/openpulse-core/tests/rate_adaptation.rs#L60), [../../crates/openpulse-core/tests/rate_adaptation.rs](../../crates/openpulse-core/tests/rate_adaptation.rs#L98) |
| B2F driver blocking I/O | Confirmed in inspected paths | [../../crates/openpulse-b2f-driver/src/lib.rs](../../crates/openpulse-b2f-driver/src/lib.rs#L62), [../../crates/openpulse-b2f-driver/src/lib.rs](../../crates/openpulse-b2f-driver/src/lib.rs#L125), [../../crates/openpulse-b2f-driver/src/cmd.rs](../../crates/openpulse-b2f-driver/src/cmd.rs#L61), [../../crates/openpulse-b2f-driver/src/data.rs](../../crates/openpulse-b2f-driver/src/data.rs#L45) |

## Recommended actions

1. Bound input growth at the reader layer for both TCP front doors: maximum command-line size for ARDOP and maximum in-progress frame size for KISS.
2. Remove `lock().unwrap()` from long-lived bridge workers and stop swallowing transmit failures; make modem failures observable to clients and logs.
3. Make manifest verification fail-closed by verifying signature and payload hash in one API.
4. Add a hard capacity bound to `RelayForwarder::seen` so replay suppression cannot become a memory sink under nonce spray.
5. Pin `ml-kem` exactly and stop excluding `Cargo.lock`, or document an equivalent reproducible dependency strategy that CI enforces.
6. Either wire `qsy.allow_trustlevels` into a real responder path or mark it explicitly nonfunctional so operators do not assume it provides trust gating.

## Validation performed

- `cargo test -p openpulse-core --test rate_adaptation` — passed (12 tests)
- `cargo test -p openpulse-core --test pq_handshake_integration` — passed (16 tests)
- `cargo test -p openpulse-qsy --test qsy_session` — passed (14 tests)