---
doc: docs/dev/reviews/documentation-review.md
date: 2026-05-22
status: resolved
resolved: 2026-05-23
---

# Documentation Review

## Summary

CLAUDE.md crate map is accurate. Public API doc coverage is good on core types but
several public functions in `relay.rs` and `engine.rs` lack `///` comments. The
CHANGELOG `[Unreleased]` section is stale — PRs #336–#341 are not recorded.
The backlog is now accurate after the cleanup PRs (#340–#341). README does not mention
the turbo codec, calibration wizard, or deny-list enforcement shipped in the most
recent PRs.

---

## Findings

### DOC-01 — CHANGELOG `[Unreleased]` section not updated for PRs #336–#341 · Severity: Medium

**File:** `CHANGELOG.md`

The `[Unreleased]` section ends at PR #325 content (GPU RRC FIR / SC-FDMA GPU soft
demod). It does not include:
- PR #335: SC-FDMA adaptive pilot density
- PR #336: On-device calibration wizard
- PR #337: Turbo codec
- PR #338/#339: Copilot review fixes
- PR #340/#341: Backlog housekeeping

**Recommendation:** Add entries to `[Unreleased]` for PRs #335–#341. When the next
version tag is cut, these move into a `[0.3.0]` block.

---

### DOC-02 — README does not reflect recent shipped features · Severity: Low

**File:** `README.md`

The README feature tables and feature summary do not mention:
- Turbo codec (`FecMode::Turbo`, rate-1/3 PCCC, Max-Log-MAP BCJR)
- On-device calibration wizard (`openpulse calibrate audio|ptt|afc`)
- SC-FDMA adaptive pilot density

These were shipped in PRs #335–#337 and represent significant capability additions to
the FEC and tooling story.

**Recommendation:** Add `Turbo (rate-1/3 PCCC)` to the FEC table and a `Calibration`
entry to the tooling section.

---

### DOC-03 — Several `pub fn` in `relay.rs` lack `///` doc comments · Severity: Low

**File:** `crates/openpulse-core/src/relay.rs`

17 public items; 12 have `///` doc comments. Missing:

- `RelayRouteError` enum variants (inner items: `EmptyRoute`, `LoopDetected`, etc.)
- `RelayForwardError::CapacityExceeded`
- `RelayEvent::CapacityExceeded`
- `RelayForwarder::evict_expired` (public but undocumented)

---

### DOC-04 — CLAUDE.md crate map matches actual crates · Severity: Pass

All crates present in `crates/`, `plugins/`, and `apps/` are listed in CLAUDE.md.
No crate is documented in CLAUDE.md but absent from the filesystem. The `openpulse-gpu`
crate is correctly listed despite having no `crates/openpulse-gpu` entry (it is at
`crates/openpulse-gpu/`).

---

### DOC-05 — Backlog is accurate after PR #340/#341 cleanup · Severity: Pass

All 7 numbered backlog items are now marked as shipped with code references. The only
open item is the deferred on-air regulatory validation (Phase 5.5-reg), which correctly
notes it requires a licensed station.

---

### DOC-06 — Roadmap current-phase statement matches code · Severity: Pass

**File:** `docs/dev/project/roadmap.md`

The roadmap correctly states all scheduled phases (1–9) and FF series are complete.
The "active tracks" section reflects the actual state (no remaining scheduled tracks).

---

### DOC-07 — Key engine functions lack doc comments · Severity: Low

**File:** `crates/openpulse-modem/src/engine.rs`

Several public functions on `ModemEngine` that form the primary external API have
no `///` doc comment:
- `transmit_with_fec_mode`
- `receive_with_fec_mode`
- `transmit_with_turbo`
- `last_rx_snr_db`

These are called directly by CLI commands and integration tests. A one-line summary
of each would significantly reduce the barrier for new contributors.

---

## Action Items

| ID | Severity | Action | Resolution |
|---|---|---|---|
| DOC-01 | Medium | Add PRs #335–#341 entries to `CHANGELOG.md [Unreleased]` | ✅ `CHANGELOG.md [Unreleased]` covers turbo codec (#337), calibration wizard (#336), SC-FDMA adaptive pilot density (#335), QSY responder, GPU kernels, and associated fixes (#338–#341) |
| DOC-02 | Low | Update README feature tables for turbo codec and calibration wizard | ✅ `README.md` FEC table includes `Turbo (rate-1/3 PCCC)`; tooling section includes `calibrate (audio/PTT/AFC)` subcommand |
| DOC-03 | Low | Add `///` doc comments to undocumented public items in `relay.rs` | ✅ All 17 public items in `crates/openpulse-core/src/relay.rs` carry `///` doc comments including enum variants, `evict_expired`, and helper functions |
| DOC-07 | Low | Add one-line `///` doc to key public methods in `engine.rs` | ✅ `transmit_with_fec_mode`, `receive_with_fec_mode`, `transmit_with_turbo`, and `last_rx_snr_db` all carry full doc blocks in `crates/openpulse-modem/src/engine.rs` |
