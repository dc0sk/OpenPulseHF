---
project: openpulsehf
doc: docs/pki-tooling-rollout-plan.md
status: living
last_updated: 2026-04-24
---

# PKI Tooling Rollout Plan

## Purpose

This document defines an implementation rollout plan for the separate PKI tooling project described by the PKI requirements, architecture, API, data model, and trust-policy specifications.

## Rollout principles

- Ship incrementally with verifiable trust-safety gates.
- Prefer fail-closed behavior over partial trust ambiguity.
- Keep each milestone deployable in self-hosted environments.
- Require reproducible acceptance checks before phase promotion.

## Phase 0: Foundations

### Objectives

- Establish repository structure, CI, and baseline runtime environment.
- Define migration scaffolding and schema versioning policy implementation.
- Stand up minimal API skeleton and role-based auth scaffolding.

### Deliverables

- Initial database schema migration set.
- API server with health, version, and auth bootstrap endpoints.
- Base audit event pipeline with append-only semantics.
- Development deployment profile with database and object storage.

### Exit criteria

- Migrations apply and rollback cleanly in CI.
- Audit events are emitted for all authenticated admin actions.
- API version endpoint returns schema metadata and build metadata.

## Phase 1: Core identity publication and lookup

### Objectives

- Deliver end-to-end identity publication and lookup without federation.
- Support signed submissions and moderator review workflow.
- Expose stable public lookup endpoints for published identities.

### Deliverables

- Submission ingest pipeline with schema and signature checks.
- Moderation queue and decision endpoints.
- Public lookup pages and API for published identities.
- Revocation record ingestion and visibility in lookup responses.

### Exit criteria

- Invalid signatures never reach published state.
- Moderator actions are auditable and queryable by submission ID.
- Revoked records are consistently surfaced as revoked in web and API.

## Phase 2: Trust evidence and policy enforcement

### Objectives

- Implement trust evidence ingestion and trust recommendation computation.
- Support policy profiles with deterministic decisions.
- Integrate GPG evidence and optional TQSL evidence ingestion.

### Deliverables

- Trust evidence pipelines for operator, GPG, TQSL, and replication sources.
- Policy profile engine for strict, balanced, and permissive profiles.
- Moderation and trust-policy dashboards for reason-code analysis.

### Exit criteria

- Re-running policy evaluation on identical inputs yields identical recommendations.
- TQSL-only evidence never auto-promotes records to trusted.
- Policy profile version and effective time are recorded with each decision.

## Phase 3: Trust-bundle export for OpenPulseHF

### Objectives

- Deliver production-grade trust-bundle exports consumable by OpenPulseHF.
- Ensure export provenance and signature verification are deterministic.

### Deliverables

- Current and historical trust-bundle export endpoints.
- Signed trust-bundle artifacts with schema version metadata.
- Export freshness and provenance observability metrics.

### Exit criteria

- OpenPulseHF trust import tooling can consume exported bundles without schema patches.
- Bundle signatures verify successfully in integration tests.
- Revocation and validity windows are represented correctly in bundle records.

## Phase 4: Federation and replication

### Objectives

- Add optional federation with controlled trust import/export behavior.
- Preserve source provenance and local policy precedence.

### Deliverables

- Replication peer management and policy filters.
- Pull and/or push replication workers with retry and backoff controls.
- Conflict handling workflows for revocation and identity lineage collisions.

### Exit criteria

- Imported records remain distinguishable from locally published records.
- Revocation conflicts are quarantined and require explicit resolution.
- Per-peer evidence-class filters are enforced and auditable.

## Phase 5: Hardening and release readiness

### Objectives

- Validate reliability, backup/restore, and operational ergonomics.
- Finalize release process, upgrade guidance, and runbooks.

### Deliverables

- Disaster recovery tests for database, artifacts, and audit trail.
- Security review findings and remediation closure.
- Operational runbooks for moderation, policy migration, and incident response.

### Exit criteria

- Backup/restore drill completes with no silent trust-state loss.
- Upgrade from previous schema versions passes migration conformance tests.
- Operational SLOs are met for lookup latency and moderation backlog.

## Cross-phase quality gates

- Frontmatter and doc validation pass in CI for all PKI docs.
- Migration tests cover forward and rollback behavior where applicable.
- Audit integrity checks validate append-only event semantics.
- API compatibility checks enforce schema-version expectations.
- Security checks verify fail-closed behavior for signature and revocation faults.

## Suggested milestone sequence

1. M0: Foundation baseline ready.
2. M1: Submission and moderation MVP.
3. M2: Public lookup and revocation visibility.
4. M3: Trust evidence and policy engine.
5. M4: Signed trust-bundle exports for OpenPulseHF.
6. M5: Federation beta with per-peer policy controls.
7. M6: Release candidate with operational runbooks and recovery drills.

## Risk register (initial)

- Policy complexity may outpace moderator usability.
- Federation could introduce conflicting trust assertions if source precedence is unclear.
- Migration errors could cause trust-state drift if rollback procedures are incomplete.
- Over-reliance on optional evidence sources (for example TQSL) could create false trust elevation if policy guardrails regress.

## Dependency map

- Phase 1 depends on Phase 0 completion.
- Phase 2 depends on Phase 1 moderation and identity pipelines.
- Phase 3 depends on Phase 2 trust recommendation stability.
- Phase 4 depends on Phase 3 export correctness.
- Phase 5 depends on completion of Phases 1 through 4.

## Open questions

- Whether federation should ship in the first stable release or remain an opt-in beta.
- Whether trust-bundle signatures should use dedicated service keys or operator-managed keys by default.
- Whether policy profile customization should be global-only or support multi-tenant overrides.