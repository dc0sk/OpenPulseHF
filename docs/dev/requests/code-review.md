---
project: openpulsehf
doc: docs/requests/code-review.md
status: open
created: 2026-05-09
last_updated: 2026-05-11
---

# Full Code Review Request

## Scope

Full project review covering design, architecture, security, correctness, and code quality.
This is a pre-feature-freeze review, targeting `main` at the point all BL-FEC-* and FF-* items
are merged.

---

## Review areas

### 1. Architecture and design

- **Plugin isolation**: Is the `ModulationPlugin` trait boundary clean? Can a third-party plugin
  break the engine if it returns malformed data? Are panics possible through plugin callbacks?
- **Engine state machine**: Does `ModemEngine` correctly serialise access to the `HpxReactor`?
  Are there any races between the event broadcast channel and the main receive loop?
- **Session layer coupling**: `HpxSession`/`HpxReactor` live in `openpulse-core`; the engine
  wires them in `openpulse-modem`. Is there accidental coupling (e.g. engine calling into core
  internals not exposed through the trait)? Could the session layer be cleanly swapped for a
  different ARQ protocol?
- **SAR and FEC ordering**: The pipeline is `payload → compress → frame_encode → FEC → modulate`.
  Review that the layer ordering is consistent across all FEC modes
  (`transmit_with_fec`, `transmit_with_concatenated_fec`, `transmit_with_soft_viterbi_fec`,
  `transmit_with_strong_fec`). Verify the inverse ordering is maintained on the receive side.
- **Channel simulation**: Does `ChannelSimHarness` faithfully represent a two-station full-duplex
  channel, or does it introduce timing artefacts (e.g. sample count mismatch between TX and RX)?

### 2. Security review

- **Post-quantum handshake** (`pq_handshake.rs`): Verify that the canonical JSON body is
  deterministic (no key-ordering ambiguity across platforms). Confirm that `PqConReq` and
  `PqConAck` are rejected when the session ID echoed in the ACK does not match the REQ.
- **Trust store**: Confirm that revoked keys (returned as `TrustError::RejectedTrustLevel`)
  are fail-closed — i.e., no path produces a connection with a revoked peer.
- **QSY protocol** (`openpulse-qsy`): Every QSY frame is Ed25519-signed. Verify that the
  `QsyScanner` rejects unsigned or forged frames before applying frequency changes.
- **Manifest verification** (`manifest.rs`): Confirm that SHA-256 of the payload is verified
  before the data is passed to the caller — not after.
- **ARDOP TCP interface**: The command port accepts ASCII over TCP with no authentication.
  Is this safe given that the default bind address is `127.0.0.1`? Does the interface reject
  malformed input that could cause a panic?
- **KISS TCP interface**: Same question as ARDOP — no authentication on the KISS port. Is the
  AX.25 decoder robust against malformed or truncated frames from a remote host?
- **Input size limits on TCP control ports**: ARDOP command port reads ASCII lines with no stated
  maximum length. An adversarial local process (or misrouted traffic to `0.0.0.0`) could send a
  very long line and cause unbounded allocation. Confirm `BufReader::lines()` is bounded or that
  the server drops connections with lines exceeding a sane limit (e.g. 4 KiB).
- **RelayForwarder dedup table growth**: `RelayForwarder` suppresses duplicates keyed on
  `(session_id, nonce)`. The table is evicted by TTL, but a remote sender can generate unique
  nonces faster than the TTL window expires, growing the table without bound. Verify that the
  dedup map has a capacity cap or that the TTL window is short enough to bound memory use.
- **Secret material lifetime**: Are ML-DSA-44 and ML-KEM-768 secret keys zeroed on drop?
  (`ed25519_dalek` uses `Zeroize`; confirm `ml_kem` and `ml_dsa` do the same.)

### 3. Correctness and edge cases

- **K=7 Viterbi** (`soft_viterbi.rs`): Verify encoder/decoder round-trips for empty payloads,
  single-byte payloads, and payloads whose bit count is not a multiple of 8 after the flush bits.
  Confirm the flush-to-state-0 invariant holds even when the input produces G0/G1 transitions
  that coincidentally leave the shift register at a non-zero state before the flush.
- **FEC short payloads**: RS(255,223) always produces a 255-byte block. Confirm that the 4-byte
  length prefix is correctly stripped on receive for payloads shorter than one block.
- **OFDM channel estimation**: The OFDM plugin uses LS+ZF equalization with pilot subcarriers.
  Review the pilot pattern — are there enough pilots per OFDM symbol to track the Watterson
  Moderate F1 Doppler spread (0.5 Hz)? What happens when a pilot subcarrier lands on a deep fade?
- **SC-FDMA DFT spread**: Confirm that the pre-coding DFT and the post-decoding IDFT are
  inverse operations to numerical precision (no phase accumulation across symbol blocks).
- **RRC matched filtering**: The `-RRC` modes apply a 3-tap Gardner timing recovery loop.
  Does the loop converge within the preamble length at the lowest SNR covered by the test matrix?
  Confirm the Costas PLL phase error does not wrap at high baud rates (QPSK9600).
- **SAR reassembly**: Confirm that `SarReassembler::expire()` correctly evicts timed-out
  sessions without dropping a concurrent `ingest()` call. Check for off-by-one in
  `fragment_total` — is a 0-fragment total rejected?
- **Memory-ARQ** (`SoftCombiner`): If `combine()` is called before any accumulation, does it
  return an empty vec or panic? Verify `reset()` actually clears the accumulator.
- **LMS/DFE equalizer** (`crates/openpulse-dsp/src/equalizer.rs`, PR #194): Does the supervised
  preamble training converge within the preamble length at minimum SNR? Is the DFE feedback path
  stable when decision-directed mode starts with high residual ISI? What happens when the payload
  is shorter than `fwd_len + dfe_len`?
- **64QAM soft demodulator** (`plugins/64qam`, PR #195): Verify the max-log-MAP branch metric
  computation — confirm that log-domain LLR accumulation does not overflow `f32` for the extreme
  constellation points (±7 PAM-8 levels). Confirm the Gray-coded bit labeling is consistent
  between modulator and demodulator for all 64 symbols. Verify the `64QAM2000-RRC` RRC-filtered
  path produces correct output at the matched filter — Hann windowing was deliberately avoided
  (it causes amplitude-dependent ISI in multi-level constellations), so confirm the rectangular
  window does not create spectral leakage that degrades adjacent subcarrier rejection.

### 4. Performance and resource usage

- **Heap allocation in the modulation hot path**: Profile `bpsk_modulate` and `qpsk_modulate`
  with `cargo flamegraph` or `perf`. Are there avoidable allocations inside the per-symbol loop?
- **GPU path fallback latency**: If `GpuContext::init()` returns `None` (headless CI), does the
  CPU fallback add measurable latency on the first call, or is the path already warm?
- **ARDOP TCP bridge threading**: The `ModemBridge` spawns an OS thread per connection. Under
  high-frequency reconnects (e.g. Pat session retries), is there a thread leak?
- **Bridge lock contention**: `Arc<Mutex<ModemEngine>>` is locked on every TX frame and every
  RX poll in the 5 ms worker loop. At high traffic (rapid Pat retries, mesh relay bursts) this
  serialises all modem access behind a single lock. Profile whether the lock hold time during a
  full FEC-encoded TX frame causes measurable latency on subsequent RX polls.
- **Broadcast channel backpressure**: `broadcast::Sender<EngineEvent>` drops events for
  slow subscribers. Is there any subscriber that must not miss events (e.g. the TUI scroll buffer)?
  If so, use `subscribe()` with a capacity guarantee.
- **B2F driver blocking IO**: `run_iss` and `run_irs` block on `TcpStream` reads. On a slow
  or unreliable CMS connection this blocks the caller thread indefinitely unless the
  `arq_timeout` is set. Confirm every read is gated by a `set_read_timeout()` call.

### 5. Code quality and maintainability

- **Error handling consistency**: Library crates must not use `unwrap()` or `expect()` in
  production paths (CLAUDE.md). Audit `openpulse-core`, `openpulse-modem`, `openpulse-channel`,
  and `openpulse-radio` for any `unwrap()` outside `#[cfg(test)]` blocks.
- **Silent error discard in bridge workers**: The ARDOP and KISS bridge worker loops use
  `let _ = engine.transmit(...)` and `let _ = bridge.rx_data_tx.send(...)`. The RX-channel
  discard is intentional (no subscribers is not an error), but TX errors from the modem engine
  are also swallowed — the TX-pending counter still decrements and the caller receives no
  feedback. Audit every `let _ = ...` in `openpulse-ardop::bridge` and `openpulse-kiss::bridge`
  and classify each as intentional vs. a silent failure that should propagate or be logged.
- **Mutex poison propagation**: Worker threads in both bridges call `.lock().unwrap()` on
  `Arc<Mutex<ModemEngine>>`. If a plugin panics mid-frame (malformed packet, integer overflow in
  DSP), the mutex becomes poisoned and every subsequent `.lock().unwrap()` in the worker thread
  panics, bringing down the process. Replace with `.lock().unwrap_or_else(|e| e.into_inner())`
  or structured panic recovery so a single bad frame does not terminate the bridge.
- **Integer arithmetic in DSP paths**: Confirm that sample accumulation and bit-packing in
  `bpsk_modulate`, `qpsk_modulate`, `psk8_modulate`, `qam64_modulate`, OFDM and SC-FDMA do not
  overflow `i32` or saturate `f32` for maximum-length payloads. Pay particular attention to
  intermediate sums in the IQ accumulator and the LDPC min-sum belief values.
- **Dead code**: Run `cargo check --workspace --no-default-features 2>&1 | grep "unused"` after
  enabling `#![warn(dead_code)]` in each crate root. Remove dead items or gate them with
  a feature flag and document why they exist.
- **Clippy compliance**: All crates must pass `cargo clippy --workspace --no-default-features -- -D warnings`.
  Review any `#[allow(clippy::...)]` attributes — each must carry a justification comment.
- **Doc coverage**: Run `cargo doc --workspace --no-default-features 2>&1 | grep "warning"` and
  address all missing documentation warnings on public items.
- **Test coverage**: Identify any public function in `openpulse-core` or `openpulse-modem` that
  has no corresponding test (unit or integration). Priority: `soft_viterbi.rs`, `ldpc.rs`,
  `compression.rs` edge cases.
- **Module boundary discipline**: Confirm that no `pub(crate)` symbol is accessed by another
  crate through a re-export that bypasses visibility intent.
- **Pre-release dependency pinning**: `ml-dsa` and `ml-kem` are declared with pre-release version
  strings (`=0.1.0-rc.11`, `0.3`). `Cargo.lock` is gitignored, so CI resolves fresh on every run.
  A new RC with a breaking API change will silently break CI (this already happened with rc.9→rc.11,
  breaking `KeyGen` and `MlDsa44::from_seed`). Audit all pre-release and rapidly-moving
  dependencies; pin each with `=x.y.z-rcN` and document the minimum stable target. Consider
  committing `Cargo.lock` (removing it from `.gitignore`) to make dependency resolution
  reproducible across all environments.

### 6. Compatibility verification

- **ARDOP Pat compatibility**: Run Pat against `openpulse-tnc` in loopback mode and verify
  a full Winlink message round-trip (FC→FS→data blobs→DISC).
- **KISS APRS compatibility**: Verify that a standard APRS client (e.g. APRS.fi web feed via
  Dire Wolf) can receive a beacon frame sent through `openpulse-kisstnc`.
- **B2F Winlink CMS**: If a test account is available, run `openpulse-gateway send` against
  `cms.winlink.org` on the primary port and verify delivery.

---

## Deliverables expected

1. Annotated issue list with severity (Critical / Major / Minor / Suggestion)
2. For each Critical and Major finding: proposed fix or mitigation
3. For architecture findings: redline diagram or description of recommended restructuring
4. Updated test cases for any correctness findings that are not already covered

---

## Out of scope

- Performance benchmarking against VARA or ARDOP hardware implementations
- On-air RF validation (deferred to Phase 3.5-reg)
- LDPC/Turbo decoder (BL-FEC-6 is explicitly deferred pending GPU)
- FreeDV authenticated voice shim (FF-11 is a prototype; review deferred)
