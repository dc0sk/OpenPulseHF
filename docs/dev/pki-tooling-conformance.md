---
project: openpulsehf
doc: docs/pki-tooling-conformance.md
status: living
last_updated: 2026-04-24
---

# PKI Tooling Conformance and Test Plan

## Purpose

This document defines conformance expectations and test strategy for the separate PKI tooling project supporting OpenPulseHF trust workflows.

## Test goals

- Prove correctness of publication, moderation, trust-policy, and export semantics.
- Detect regressions in API compatibility and trust-bundle schema behavior.
- Validate deterministic policy outcomes for fixed inputs.
- Validate migration safety and rollback behavior across schema versions.

## Test scope

The test plan covers:

- API behavior and versioning
- Data model constraints and state transitions
- Signature and trust evidence verification
- Moderation workflow and policy guardrails
- Revocation semantics
- Trust-bundle export correctness and signature verification
- Replication/federation conflict handling
- Backup/restore and disaster-recovery integrity

## Conformance levels

### Level 1: Core correctness

- Required for every change set.
- Includes unit tests and deterministic integration tests.

### Level 2: Workflow conformance

- Required before minor releases.
- Includes full submission-to-publication workflow tests and moderation matrix tests.

### Level 3: Operational conformance

- Required before stable release candidates.
- Includes migration drills, restore drills, and incident simulation tests.

## Test categories

## API conformance tests

- Verify all documented endpoints return required fields.
- Verify unknown fields do not break parsers when additive schema changes occur.
- Verify error responses contain required error model fields.
- Verify pagination behavior and `next_cursor` semantics.
- Verify authorization boundaries between public, moderator, and operator endpoints.

Required checks:

- Contract tests generated from API schema examples.
- Backward-compatibility checks for unchanged major API versions.

## Data model conformance tests

- Verify immutable identifiers and append-only revision semantics.
- Verify uniqueness constraints for station IDs and key fingerprints under policy.
- Verify submission, moderation, and publication state transitions are legal and reject invalid transitions.
- Verify revocation precedence (key > revision > record targeting behavior).

Required checks:

- Constraint tests at persistence layer.
- State-machine transition tests with negative cases.

## Cryptographic and evidence conformance tests

- Verify valid OpenPulseHF-native signatures are accepted.
- Verify invalid signatures are rejected and never published.
- Verify GPG detached-signature validation path and metadata capture.
- Verify TQSL evidence ingestion and source tagging.
- Verify TQSL-only evidence cannot auto-promote trust to trusted under baseline policy.

Required checks:

- Deterministic verification test vectors.
- Negative-path tests for malformed signature artifacts.

## Moderation and trust-policy conformance tests

- Verify strict, balanced, and permissive profile behaviors match documented decision matrix.
- Verify automation acceptance guardrails route ineligible submissions to quarantine.
- Verify moderation actions are audit-linked and reason-coded.
- Verify policy version and effective timestamp are recorded for trust decisions.

Required checks:

- Decision-table conformance tests.
- Replay tests ensuring deterministic outcomes for fixed input plus fixed policy version.

## Revocation conformance tests

- Verify revocation records immediately affect publication and trust recommendation views.
- Verify revoked records propagate correctly to trust-bundle exports.
- Verify conflicting revocations from federation imports are quarantined.

Required checks:

- Revocation lifecycle integration tests.
- Conflict-resolution workflow tests.

## Trust-bundle export conformance tests

- Verify bundle schema fields and record projections match API specification.
- Verify bundle signatures validate with expected keys.
- Verify current-bundle and historical-bundle endpoints are consistent.
- Verify validity windows and revocation state are represented correctly.

Required checks:

- Snapshot tests for stable fixture sets.
- Signature validation tests for every generated bundle fixture.

## Replication conformance tests

- Verify imported records preserve source instance metadata.
- Verify local policy precedence over imported trust recommendations.
- Verify per-peer evidence filters are enforced.
- Verify replication retries/backoff do not duplicate accepted records.

Required checks:

- Multi-instance integration tests.
- Idempotency tests for repeated import batches.

## Migration and rollback conformance tests

- Verify forward migrations from supported previous schema versions.
- Verify rollback where reversible migrations are declared.
- Verify trust-bundle schema compatibility across migration boundaries.
- Verify no silent trust-state drift after migration.

Required checks:

- Migration dry-run tests in CI.
- Post-migration reconciliation tests.

## Backup and restore conformance tests

- Verify restore recovers canonical records, moderation events, revocations, and audit links.
- Verify restored environment can regenerate trust bundles with expected provenance.
- Verify artifact and detached-signature references remain valid after restore.

Required checks:

- Scheduled restore drills in non-production environments.
- Hash comparison checks for sampled restored artifacts.

## Test environments

- Local developer environment with deterministic fixture datasets.
- CI integration environment with ephemeral database and object storage.
- Pre-release staging environment for operational conformance and migration drills.

## Fixture requirements

- Provide signed and unsigned submission fixtures.
- Provide valid and invalid signature fixtures for OpenPulseHF-native and GPG paths.
- Provide TQSL evidence fixtures representing advisory-only scenarios.
- Provide federation conflict fixtures for revocation and lineage collisions.

## Acceptance gates by release type

- Patch release:
  - Level 1 pass required.
- Minor release:
  - Level 1 and Level 2 pass required.
- Release candidate / stable:
  - Level 1, Level 2, and Level 3 pass required.

## Metrics and quality thresholds

- Core trust-policy and moderation code coverage: >= 90% line coverage.
- End-to-end workflow success rate in CI: >= 99% across non-flaky suites.
- Zero tolerated failures in signature-rejection negative tests.
- Zero tolerated failures in trust-bundle signature validation tests.

## Flaky-test policy

- Any flaky conformance test must be quarantined and tracked with owner and due date.
- Quarantined tests cannot remain unresolved beyond one minor release cycle.
- Release gating tests may not be bypassed without explicit operator approval and documented risk acceptance.

## Traceability matrix requirement

- Every requirement in PKI requirements and trust-policy documents must map to at least one test case ID.
- Test reports must publish requirement-to-test and test-to-result mapping artifacts.

## Deliverables

- Automated conformance test suites in CI.
- Versioned fixture packs for signatures, policy scenarios, and federation events.
- Periodic conformance report artifacts attached to release workflows.

## Open questions

- Whether mutation testing should be required for trust-policy decision logic.
- Whether conformance fixtures should include privacy-redacted real-world anonymized data samples.