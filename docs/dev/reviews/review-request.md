# OpenPulseHF — Copilot Review Request

**Requested**: 2026-05-08  
**Output target**: `docs/reviews/review-260508.md`  
**Reviewer**: GitHub Copilot (local session)  
**Context**: After completion of FF-1 (QSY frequency-agility, PR #140).  
`docs/roadmap.md` claims 54 ✅ completed items across Phases 1–9 and selected FF items.

---

## Review objectives

This review has one primary goal: **challenge every completeness claim in this codebase**.

The review should read like a trusted senior engineer who just joined the team and is auditing
whether the project is as mature as its documentation claims. Be adversarial. Look for gaps,
mismatches, dead code, broken contracts, and unsubstantiated claims. Do not rubber-stamp.

---

## Topic areas

### 1. Roadmap accuracy (user request)

`docs/roadmap.md` marks 54 items ✅.  For each ✅ claim, verify:

- The relevant crate, module, or file actually exists and is non-trivial
- The implementation matches the spec in `CLAUDE.md` (acceptance criteria table)
- If the item has acceptance tests listed, confirm the tests exist and cover the claim
- Flag any ✅ item that appears to be incomplete, stub-only, or missing from the workspace

Key phases to scrutinise:
- Phase 9 (9.1–9.5): `docs/roadmap.md` does not show ✅ markers in the Phase 9 section;
  CLAUDE.md claims all five items done. Verify the implementations exist.
- FF-1 (QSY): merged in PR #140 on 2026-05-08. Confirm `crates/openpulse-qsy` is present,
  the session state machine covers all five frame types, the scanner test passes.
- Phase 3.3 (GPU compute): `crates/openpulse-gpu` exists. Confirm the crate builds,
  the feature flag works, and the CPU fallback path is exercised by tests.
- Phase 4.5 (testbench GUI): `apps/openpulse-testbench` exists. Confirm it compiles and
  the claimed 4-column layout / 7 channel models are present in code.

### 2. Statement correctness (user request)

Review key technical documents for factual accuracy against the code:

- `docs/architecture.md` — does the described crate graph match the actual `Cargo.toml` dependency tree?
- `docs/peer-query-relay-wire.md` — does the wire format spec match `crates/openpulse-core/src/wire_query.rs`? Check field sizes, byte offsets, and enum codes.
- `docs/hpx-waveform-design.md` and `docs/hpx-session-state-machine.md` — do HPX waveform parameters and state labels match the implementation in `openpulse-core/src/hpx.rs`?
- `CLAUDE.md` crate map — are the listed crates and their stated roles still current? (Several crates were added after the crate map was last updated.)

### 3. Implementation quality (user request)

Audit the following for correctness, not just existence:

- `crates/openpulse-core/src/relay.rs` — `RelayForwarder` duplicate suppression: does the
  `(session_id, nonce)` TTL eviction actually free memory, or does the suppression map grow unbounded?
- `crates/openpulse-core/src/rate.rs` — `RateAdapter::apply_ack()`: verify the SL1 ChirpFallback
  path requires exactly 3 consecutive NACKs at SL2 (not just any 3 NACKs).
- `crates/openpulse-b2f/src/compress.rs` — LZHUF codec: the `decompress_lzhuf` 16 MiB cap is
  documented as an OOM guard. Confirm the cap is applied _before_ allocation, not after.
- `crates/openpulse-qsy/src/session.rs` — `pick_best_freq`: what happens when the partner's vote
  list doesn't contain a frequency that the initiator listed? Verify the fallback is safe.
- `plugins/bpsk/src/demodulate.rs` — `estimate_frequency_offset()`: confirm the IQ-squaring
  estimator's ±baud/4 tracking range claim matches the actual FFT bin calculation.

### 4. Architecture and design (user request)

- Is the `openpulse-daemon` crate (`crates/openpulse-daemon/`) functional or a stub? It is not
  listed in `CLAUDE.md`'s crate map. Determine its intended role and whether it's safe to keep
  as-is, document, or remove.
- Same question for `crates/openpulse-dsp/`, `crates/openpulse-mesh/`,
  `crates/openpulse-repeater/`, `apps/openpulse-testmatrix/`, and `apps/openpulse-panel/`.
  These crates are in the workspace but absent from the CLAUDE.md crate map.
- Does the `openpulse-gpu` crate's wgpu dependency introduce a hard dependency on GPU drivers
  at build time even when the `gpu` feature is disabled? Check the workspace `Cargo.toml` for
  conditional compilation hygiene.
- Is there a circular dependency risk between `openpulse-core` and any other crate? Map the
  core ← modem ← cli dependency chain and confirm no back-edges.

### 5. Test coverage per claimed feature (assistant addition)

For each of these subsystems, verify that the claimed tests exist, are not trivially passing,
and cover the failure path as well as the success path:

| Subsystem | Expected test file |
|---|---|
| ACK codec + rate adaptation | `crates/openpulse-core/tests/rate_adaptation.rs` |
| Signed handshake | `crates/openpulse-core/tests/handshake_integration.rs` |
| PQ handshake | `crates/openpulse-core/tests/pq_handshake_integration.rs` |
| Relay forwarding | `crates/openpulse-core/tests/relay_integration.rs` |
| Query propagation | `crates/openpulse-core/tests/query_propagation_integration.rs` |
| QSY session + scanner | `crates/openpulse-qsy/tests/qsy_session.rs` |
| ARDOP TCP bridge | `crates/openpulse-ardop/tests/ardop_integration.rs` |
| B2F driver e2e | `crates/openpulse-b2f-driver/tests/e2e_loopback.rs` |
| CSMA/DCD | `crates/openpulse-modem/tests/csma_loopback.rs` |
| Engine events | `crates/openpulse-modem/tests/engine_events.rs` |

Flag any file that is missing, has fewer tests than claimed, or only tests the happy path.

### 6. Wire format spec compliance (assistant addition)

`docs/peer-query-relay-wire.md` is the authoritative spec for the binary OPHF envelope and
all query/relay payload types. Cross-check:

- `WireEnvelope` header: 104 B claimed → count actual struct field sizes in `wire_query.rs`
- `PeerQueryRequest`: 17-byte fixed payload → verify
- `RelayDataChunk`: 82-byte header claimed → verify
- `RelayHopAck`: 49-byte fixed → verify
- `RouteDiscoveryRequest`: 47-byte fixed → verify
- All `WireMsgType` enum code values (0x01–0x08) match the spec table

Any mismatch between spec and code is a protocol compatibility bug.

### 7. Ed25519 signing surface audit (assistant addition)

Several subsystems independently sign data with Ed25519. Audit each for consistent security posture:

- `crates/openpulse-core/src/handshake.rs` — CONREQ/CONACK signing
- `crates/openpulse-core/src/manifest.rs` — transfer manifest signing
- `crates/openpulse-core/src/pq_handshake.rs` — hybrid and PQ-only dual-signing
- `crates/openpulse-core/src/peer_descriptor.rs` — self-authenticating identity
- `crates/openpulse-qsy/src/frame.rs` — per-frame signing

Check: (a) is the signed byte range explicit and stable (no serialiser ambiguity)? (b) is a
tamper/truncation test present for each signer? (c) are any signers using deprecated or
pre-release crate versions that should be upgraded?

### 8. CLAUDE.md freshness (assistant addition)

`CLAUDE.md` is the agent contract and is checked-in. Audit it for staleness:

- The crate map lists 8 crates. The workspace actually has significantly more. List all missing crates
  and recommend whether to add them or mark them as intentionally undocumented.
- The "sharp edges" section mentions two known issues. Are these still present in the code, or
  have they been silently fixed? Are there new sharp edges that should be documented?
- Phase 2 is listed as "Active phase" — given that Phases 3–9 and FF-1 are now also claimed done,
  what is the correct active phase? Flag this as a stale annotation.
- Are there any acceptance criteria in the table that reference tests that no longer exist or have
  been renamed?

### 9. Config–feature parity (assistant addition)

`crates/openpulse-config/src/lib.rs` defines the TOML schema. For each config field, verify
there is at least one code path that actually reads and uses it:

- `[qsy]` — `QsyConfig` was just added. Confirm `commands/qsy.rs` reads all fields (enabled,
  candidate_freqs_hz, scan_dwell_ms, switchover_offset_s, allow_trustlevels).
- `[relay]` — confirm `relay.rs` or equivalent reads relay config, not just defaults.
- `[audio]` — confirm `backend` field is consumed in both `openpulse-ardop` and `openpulse-kiss` startup.
- Flag any config field with no reader (dead config is as misleading as dead code).

### 10. Phase 7 completeness (assistant addition)

Phase 7 (Operator Panel and Dual-Rig Control) has **zero ✅ items** in `docs/roadmap.md`, yet:

- `apps/openpulse-panel/` exists in the workspace
- `crates/openpulse-daemon/` exists with `protocol.rs`

Determine: are these stubs placed speculatively, or is Phase 7 partially implemented without
being marked done? If stub-only, confirm no Phase 7 code is accidentally reachable from
production paths. If partially done, identify which items meet the acceptance criteria.

---

## Output format

Write findings to `docs/reviews/review-260508.md` using this structure:

```
# OpenPulseHF Review — 2026-05-08

## Executive summary
<3–5 bullet points: most critical findings>

## Findings by topic

### 1. Roadmap accuracy
...

### 2. Statement correctness
...

[etc.]

## Verdict per ✅ claim
<table: | Phase/Item | Claim | Verdict (Confirmed / Partial / Unverified / Missing) | Evidence |>

## Recommended actions
<numbered list, highest priority first>
```

Be specific: cite file paths and line numbers for every finding. Do not write vague statements
like "the code looks good" — either confirm with evidence or flag as unverified.
