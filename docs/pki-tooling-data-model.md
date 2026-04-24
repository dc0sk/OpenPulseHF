---
project: openpulsehf
doc: docs/pki-tooling-data-model.md
status: living
last_updated: 2026-04-24
---

# PKI Tooling Data Model

## Purpose

This document defines a concrete data model for the separate PKI tooling project, aligned with the requirements and architecture specifications.

## Design principles

- Keep canonical identity and trust records auditable and revision-preserving.
- Separate publication state from trust evidence state.
- Treat revocation as explicit first-class data, not implicit status mutation.
- Keep export schemas versioned and derivable from canonical data.

## Identifier conventions

- `record_id`: stable identity record identifier (ULID or UUIDv7).
- `revision_id`: immutable identity revision identifier.
- `submission_id`: immutable upload/ingestion identifier.
- `evidence_id`: immutable trust evidence identifier.
- `revocation_id`: immutable revocation identifier.
- `bundle_id`: immutable trust-bundle export identifier.
- `instance_id`: identifier for PKI service instance in federation contexts.

## Core entities

### identity_record

Represents a station identity lineage.

Required fields:

- `record_id`
- `station_id`
- `callsign`
- `current_revision_id`
- `publication_state` (`pending`, `published`, `quarantined`, `rejected`, `revoked`)
- `created_at`
- `updated_at`

Constraints:

- `record_id` is immutable.
- `station_id` must be unique per active published record under local policy.

### identity_revision

Immutable snapshot of identity content for a given record.

Required fields:

- `revision_id`
- `record_id`
- `revision_number`
- `algorithms[]`
- `keys[]`
- `valid_from`
- `valid_until`
- `submitted_via` (`api`, `web`, `replication`)
- `submission_id`
- `created_at`

Constraints:

- `(record_id, revision_number)` must be unique.
- Revisions are append-only; no in-place overwrite.

### key_material

Key rows associated with a revision.

Required fields:

- `revision_id`
- `key_id`
- `algorithm`
- `public_key`
- `fingerprint`
- `key_status` (`active`, `deprecated`, `revoked`, `superseded`)

Constraints:

- `(revision_id, key_id)` unique.
- `fingerprint` unique per active key under local policy.

### submission

Tracks ingestion and moderation lifecycle for uploaded artifacts.

Required fields:

- `submission_id`
- `submitter_identity`
- `submission_state` (`pending`, `accepted`, `quarantined`, `rejected`)
- `received_at`
- `artifact_uri`
- `detached_signature_uri` (nullable)
- `validation_summary`
- `moderation_reason_code` (nullable)

### trust_evidence

Normalized trust evidence linked to records and revisions.

Required fields:

- `evidence_id`
- `record_id`
- `revision_id` (nullable)
- `source_type` (`operator`, `gpg`, `tqsl`, `replication`)
- `source_instance_id` (nullable)
- `verification_state` (`verified`, `unverified`, `failed`, `unknown`)
- `weight_class` (`advisory`, `weighted`, `authoritative`)
- `collected_at`
- `expires_at` (nullable)

Rules:

- TQSL evidence defaults to `advisory` unless local policy escalates it.
- GPG evidence must include signer fingerprint in evidence metadata.

### revocation

Represents explicit revocation events.

Required fields:

- `revocation_id`
- `record_id`
- `revision_id` (nullable)
- `key_id` (nullable)
- `issuer_identity`
- `reason_code`
- `effective_at`
- `created_at`

Rules:

- Revocation applies by most-specific target: key, revision, then record.
- Revocations are immutable; superseding actions require new records.

### moderation_event

Append-only moderation trail.

Required fields:

- `event_id`
- `submission_id`
- `actor_identity`
- `action` (`accept`, `reject`, `quarantine`, `reopen`)
- `reason_code`
- `reason_text`
- `created_at`

### audit_event

Tamper-evident operational event stream.

Required fields:

- `event_id`
- `event_type`
- `entity_type`
- `entity_id`
- `actor_identity`
- `request_id`
- `created_at`
- `event_payload_hash`

## Derived views

### published_identity_view

Materialized or query view joining:

- `identity_record`
- current `identity_revision`
- current `key_material`
- effective `revocation`
- trust aggregate derived from `trust_evidence`

### trust_bundle_view

Deterministic export projection from published identities including:

- signer algorithms
- hybrid policy
- validity windows
- revocation state
- evidence summary

## State transition model

### submission state

- `pending -> accepted`
- `pending -> quarantined`
- `pending -> rejected`
- `quarantined -> accepted`
- `quarantined -> rejected`

### publication state

- `pending -> published`
- `pending -> quarantined`
- `pending -> rejected`
- `published -> revoked`

## Migration and schema versioning policy

- The canonical schema uses semantic schema versions: `major.minor.patch`.
- Major changes may not be auto-applied without explicit operator confirmation.
- Each migration must define:
  - forward steps
  - rollback steps or explicit non-reversible warning
  - data validation checks
  - post-migration reconciliation procedure
- Trust-bundle export schema version must be tracked independently from internal DB schema version.

## Data retention policy baseline

- Keep identity revisions and moderation events indefinitely unless explicit operator retention policy says otherwise.
- Keep audit events for a minimum of 2 years.
- Keep rejected/quarantined submission artifacts for at least 180 days for abuse analysis.

## Example relational mapping

Suggested primary tables:

- `identity_records`
- `identity_revisions`
- `identity_keys`
- `submissions`
- `trust_evidence`
- `revocations`
- `moderation_events`
- `audit_events`
- `trust_bundles`
- `replication_peers`

Suggested key indexes:

- `identity_records(station_id)`
- `identity_keys(fingerprint)`
- `submissions(submission_state, received_at)`
- `revocations(record_id, effective_at)`
- `trust_evidence(record_id, source_type, verification_state)`
- `audit_events(entity_type, entity_id, created_at)`

## Open questions

- Whether evidence aggregation should be precomputed or evaluated at query time under dynamic policy.
- Whether trust-bundle exports should carry full evidence entries or summary-only entries.