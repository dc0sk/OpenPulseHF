# OpenPulseHF Project Audit (Qwen Primary, Codex Cross-Check)

Date: 2026-05-26
Scope: security, error handling, implementation completeness, plausibility, architecture/design, dead code/code gaps, consistency.
Method: primary pass run with local Ollama Qwen, then line-by-line validation and consolidation with Codex against workspace files.

## Findings

### Critical

1. Unauthenticated moderation queue exposure
- Impact: sensitive moderation workflow data is publicly retrievable without API authentication.
- Evidence: `pki-tooling/src/lib.rs:52`, `pki-tooling/src/lib.rs:55`, `pki-tooling/src/api/handlers.rs:700`, `pki-tooling/src/api/handlers.rs:719`.
- Why it matters: the route is mounted under open endpoints and returns `submission_id`, `submitter_identity`, and moderation reason metadata. This can leak private identifiers and operational moderation state.
- Recommendation: move `/api/v1/moderation/queue` to protected routes behind `require_api_key`; add explicit authorization policy tests.
- Confidence: confirmed.

### High

1. Submission identity is hardcoded to anonymous in persistence and audit trail
- Impact: loss of accountability and weak forensic value for incident response.
- Evidence: `pki-tooling/src/api/handlers.rs:575`, `pki-tooling/src/api/handlers.rs:609`.
- Why it matters: signed submissions are verified cryptographically, but persisted actor identity is always `api:anonymous`, breaking actor traceability and enabling abuse without attributable identity at the API layer.
- Recommendation: derive actor identity from verified payload (`pubkey`/peer identity) and persist both authenticated principal and asserted identity.
- Confidence: confirmed.

2. Internal database/runtime error details are returned to clients
- Impact: information disclosure of internal schema/runtime behavior that can aid reconnaissance.
- Evidence: `pki-tooling/src/api/handlers.rs:255`, `pki-tooling/src/api/handlers.rs:739`, `pki-tooling/src/api/handlers.rs:1459` (representative; many similar callsites).
- Why it matters: exposing raw DB error strings (`{err}`) increases attack surface by leaking table/constraint/query details.
- Recommendation: return stable public error codes/messages; log full internal errors server-side only.
- Confidence: confirmed.

3. Project health gates are currently blocked by toolchain/dependency mismatch
- Impact: CI-equivalent local validation (`clippy`/`test`) cannot run in the current toolchain, reducing confidence in implementation completeness and regressions.
- Evidence: `pki-tooling/Cargo.toml:12` (`sqlx = "0.9"`), absence of repository toolchain pin (`rust-toolchain` file not present).
- Why it matters: reproducible validation is part of architecture governance. Without pinned toolchain/version contract, contributors can silently diverge.
- Recommendation: add `rust-toolchain.toml` and/or explicit `rust-version` constraints; align dependency MSRV policy and CI image/toolchain.
- Confidence: confirmed.

### Medium

1. Ephemeral signing key fallback can break trust continuity
- Impact: generated trust bundles can become unverifiable across restarts when `PKI_SIGNING_KEY` is unset.
- Evidence: `pki-tooling/src/main.rs:33`, `pki-tooling/src/main.rs:38`.
- Why it matters: this is operationally dangerous outside isolated dev mode because verifier trust anchors can drift unexpectedly.
- Recommendation: require persistent signing key by default; gate ephemeral mode behind explicit `--dev` or `PKI_ALLOW_EPHEMERAL_KEY=true`.
- Confidence: confirmed for behavior, needs validation for intended deployment policy.

2. Onboarding/agent documentation contract is inconsistent
- Impact: operational ambiguity for contributors and automation agents.
- Evidence: `AGENTS.md:1` references mandatory rules; `CLAUDE.md:10` states mandatory docs include `docs/AGENTS.md`, but file is missing.
- Why it matters: process drift causes inconsistent review/build behavior and weakens governance controls.
- Recommendation: either restore `docs/AGENTS.md` or remove references and consolidate into one authoritative location.
- Confidence: confirmed.

3. Production startup path uses panic-style termination on recoverable startup failures
- Impact: abrupt process exit without graceful shutdown path, reducing resilience.
- Evidence: `pki-tooling/src/main.rs:13`, `pki-tooling/src/main.rs:18`, `pki-tooling/src/main.rs:64`, `pki-tooling/src/main.rs:69`.
- Why it matters: `expect` is acceptable for some binaries, but here it converts environmental faults (DB unavailable, port busy) into panic exits rather than controlled failure reporting/retry policy.
- Recommendation: replace `expect` with structured error propagation and explicit fatal logging/exit codes.
- Confidence: confirmed.

### Low

1. Runtime binding address is hardcoded
- Impact: deployment rigidity and environment-specific workarounds.
- Evidence: `pki-tooling/src/main.rs:62`.
- Why it matters: static bind address/port complicates containerized and multi-instance deployment.
- Recommendation: make host/port configurable via env/CLI.
- Confidence: confirmed.

2. Dead-code suppressions exist in source tree, mostly justified but should be tracked
- Impact: low direct risk; can hide stale code paths over time.
- Evidence: `apps/openpulse-panel/src/transport.rs:34`, `crates/openpulse-qsy/src/session.rs:113`.
- Why it matters: suppressions should be intentional and periodically reviewed.
- Recommendation: keep a short rationale comment where suppression is non-test and audit quarterly.
- Confidence: confirmed.

## Action Items (Prioritized)

1. Protect moderation queue endpoint with API auth and add regression tests.
- Suggested owner: PKI/API maintainer.
- Effort: S.

2. Replace anonymous submission actor persistence with verified identity attribution model.
- Suggested owner: PKI/API maintainer + security reviewer.
- Effort: M.

3. Introduce public-safe error responses and centralized internal error logging.
- Suggested owner: PKI/API maintainer.
- Effort: M.

4. Pin toolchain (`rust-toolchain.toml`) and codify MSRV policy for workspace/dependencies.
- Suggested owner: build/release maintainer.
- Effort: S.

5. Require persistent PKI signing key by default; make ephemeral mode explicit and non-default.
- Suggested owner: PKI/API maintainer.
- Effort: S.

6. Resolve onboarding docs inconsistency (`docs/AGENTS.md` reference vs actual files).
- Suggested owner: docs/process maintainer.
- Effort: S.

7. Refactor startup `expect` calls in `pki-tooling` into explicit error handling with consistent exit behavior.
- Suggested owner: PKI/API maintainer.
- Effort: S.

8. Externalize bind host/port configuration.
- Suggested owner: PKI/API maintainer.
- Effort: S.

9. Create an unwrap/expect reduction plan for non-test code paths (first pass on API and daemon entrypoints).
- Suggested owner: crate owners.
- Effort: M.

10. Implement a two-tier validation gate model: core workspace gate (excluding `pki-tooling`) plus full workspace gate (including `pki-tooling`) on the pinned toolchain.
- Suggested owner: build/release maintainer.
- Effort: M.

11. Add a constrained-environment fallback gate command set focused on DSP/protocol crates when full workspace gates are unavailable.
- Suggested owner: build/release maintainer + modem/channel crate owners.
- Effort: S.

12. Document the toolchain contract and fallback validation flow in `CLAUDE.md` and `README.md`.
- Suggested owner: docs/process maintainer.
- Effort: S.

13. Add a preflight toolchain check script that fails fast with a clear Rust-version/dependency-compatibility message.
- Suggested owner: build/release maintainer.
- Effort: S.

14. Add scheduled full-confidence CI runs (nightly or merge-gated) using a pinned container/toolchain image.
- Suggested owner: build/release maintainer.
- Effort: M.

15. If Rust 1.94 adoption is delayed, temporarily pin `sqlx` to a Rust-1.91-compatible version in `pki-tooling` and track reversal as explicit technical debt.
- Suggested owner: PKI/API maintainer + build/release maintainer.
- Effort: M.

## Residual Risks

- This review found concrete issues in PKI/service surfaces and process governance; wider DSP/protocol correctness risks remain due to inability to run full clippy/test gates in this environment.
- High aggregate `unwrap/expect` counts indicate ongoing reliability debt, though many occurrences are test-only and not directly exploitable.

## What Could Not Be Verified From Evidence

- Full workspace runtime behavior under CI-equivalent lint/test due to toolchain mismatch.
- Whether open moderation queue access is intentionally public per product policy (no explicit policy artifact found in reviewed files).
- End-to-end threat model and deployment assumptions for `pki-tooling` (dev-only vs production service role).
