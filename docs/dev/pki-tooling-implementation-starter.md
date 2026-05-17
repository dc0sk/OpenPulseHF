---
project: openpulsehf
doc: docs/pki-tooling-implementation-starter.md
status: living
last_updated: 2026-04-24
---

# PKI Tooling Implementation Starter

## Purpose

This document turns the PKI specification set into an implementation-ready starter blueprint.

## Target baseline

- Language: Rust stable
- Service model: API + workers + web frontend served behind reverse proxy
- Database: PostgreSQL
- Artifact storage: S3-compatible object store (or local filesystem in dev)
- Auth: token-based API auth plus role-based authorization

## Suggested repository layout

```text
pki-tooling/
  Cargo.toml
  rust-toolchain.toml
  .env.example
  migrations/
  crates/
    pki-core/
      src/
        domain/
        policy/
        crypto/
        export/
    pki-api/
      src/
        routes/
        auth/
        handlers/
        dto/
    pki-worker/
      src/
        ingest/
        moderation/
        export/
        replication/
    pki-web/
      src/
        pages/
        components/
  tests/
    fixtures/
    integration/
```

## Crate responsibilities

- `pki-core`: canonical types, state transitions, trust-policy evaluation, signature verification interfaces.
- `pki-api`: HTTP handlers, request validation, authN/authZ checks, API version routing.
- `pki-worker`: background jobs for ingestion, moderation queue actions, trust-bundle export, replication tasks.
- `pki-web`: operator and moderator UI.

## Initial domain types (MVP)

- `IdentityRecord`
- `IdentityRevision`
- `KeyMaterial`
- `Submission`
- `TrustEvidence`
- `Revocation`
- `ModerationEvent`
- `AuditEvent`
- `TrustBundle`

Each type should include explicit ID fields and serialization forms for API DTOs.

## Initial migration set

Migration order:

1. `0001_identity_records.sql`
2. `0002_identity_revisions.sql`
3. `0003_identity_keys.sql`
4. `0004_submissions.sql`
5. `0005_trust_evidence.sql`
6. `0006_revocations.sql`
7. `0007_moderation_events.sql`
8. `0008_audit_events.sql`
9. `0009_trust_bundles.sql`
10. `0010_replication_peers.sql`

Minimum constraints to enforce in migrations:

- immutable primary IDs
- revision append-only uniqueness `(record_id, revision_number)`
- key uniqueness constraints under active policy
- explicit enum/state columns for moderation and publication state

## API starter routes

Public routes:

- `GET /api/v1/identities/{record_id}`
- `GET /api/v1/identities:lookup`
- `GET /api/v1/revocations`
- `GET /api/v1/trust-bundles/current`
- `GET /api/v1/trust-bundles/{bundle_id}`

Authenticated routes:

- `POST /api/v1/submissions`
- `GET /api/v1/submissions/{submission_id}`
- `GET /api/v1/moderation/queue`
- `POST /api/v1/moderation/{submission_id}/decision`

## Worker starter jobs

- `verify_submission_job`
- `derive_trust_evidence_job`
- `apply_policy_decision_job`
- `publish_revision_job`
- `generate_trust_bundle_job`
- `replicate_peer_batch_job`

Each job should emit structured audit events.

## Policy engine starter design

- Represent policy profiles as versioned config objects (`strict`, `balanced`, `permissive`).
- Evaluate decisions using deterministic input structs:
  - signature status
  - revocation conflicts
  - evidence summary
  - lineage continuity
- Persist decision output with policy version and effective timestamp.

## Cryptography integration starter

- Start with trait-based verification interfaces:
  - `OpenPulseSignatureVerifier`
  - `GpgSignatureVerifier`
  - `BundleSigner`
- Keep concrete implementations in adapter modules so policy logic remains testable.

## Configuration starter

Suggested config sections:

- `database`
- `artifact_store`
- `auth`
- `policy`
- `export`
- `replication`
- `observability`

All production-critical settings should be environment-variable overridable.

## Observability starter

Metrics:

- submission ingest rate
- moderation queue depth
- signature verification failures
- trust-bundle generation duration
- replication lag

Logs:

- include `request_id`, `submission_id`, and `record_id` where available
- separate security/audit events from general app logs

Tracing:

- span boundaries for API handler -> worker enqueue -> job execution -> persistence

## MVP acceptance checklist

- Submissions can be ingested and validated.
- Invalid signatures never become published.
- Moderators can accept/reject/quarantine submissions.
- Published identities are queryable through public endpoints.
- Trust bundles can be generated, signed, and downloaded.
- Revocations are visible in API and reflected in trust-bundle output.

## Recommended first milestones (implementation)

1. Milestone A: schema + core domain + basic API skeleton.
2. Milestone B: submission ingest + signature verification + moderation queue.
3. Milestone C: publication pipeline + trust-bundle export.
4. Milestone D: policy engine profiles + conformance fixtures.
5. Milestone E: replication beta and operational hardening.

## Open tasks to create immediately

- Create migration stubs for the first four tables.
- Add API contract tests for public lookup routes.
- Add signature failure negative tests.
- Add policy determinism fixture suite.
- Add trust-bundle snapshot tests.