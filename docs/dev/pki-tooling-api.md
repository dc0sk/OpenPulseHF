---
project: openpulsehf
doc: docs/pki-tooling-api.md
status: living
last_updated: 2026-04-24
---

# PKI Tooling API and Export Schema

## Purpose

This document defines the initial API surface and export schema expectations for the separate PKI tooling project used with OpenPulseHF.

## Design goals

- Keep public lookup endpoints simple, explicit, and versioned.
- Preserve enough metadata for OpenPulseHF trust-store import and relay-policy evaluation.
- Make moderation and publication workflows auditable through stable identifiers.
- Avoid ambiguous trust semantics by separating publication state, trust evidence, and revocation state.

## API versioning

- All endpoints are versioned under `/api/v1/`.
- Breaking schema changes require a new major API version.
- Response payloads must include `schema_version` and `generated_at` metadata.

## Authentication model

- Public lookup and trust-bundle export endpoints may be readable without authentication.
- Submission, moderation, replication administration, and audit access endpoints require authentication.
- Authenticated responses must include caller role context where relevant to moderation workflows.

## Resource model

Primary resource types:

- identity record
- identity revision
- revocation record
- trust evidence record
- submission record
- trust bundle
- replication peer

## Public lookup endpoints

### `GET /api/v1/identities/{record_id}`

Returns the current published identity view for a stable record ID.

Required fields:

- `schema_version`
- `record_id`
- `station_id`
- `callsign`
- `publication_state`
- `trust_state`
- `algorithms[]`
- `keys[]`
- `valid_from`
- `valid_until`
- `revocation_state`
- `current_revision_id`

### `GET /api/v1/identities:lookup`

Lookup endpoint supporting query parameters:

- `station_id`
- `callsign`
- `fingerprint`
- `key_id`
- `algorithm`
- `trust_state`

Rules:

- Returns paginated results.
- Returns only published records for unauthenticated callers.
- Includes explicit empty-result response instead of 404 for filtered searches.

### `GET /api/v1/revocations`

Query revocation records by:

- `record_id`
- `fingerprint`
- `issuer_id`
- `effective_before`
- `effective_after`

## Submission endpoints

### `POST /api/v1/submissions`

Creates a new submission.

Accepted content forms:

- JSON identity publication bundle
- multipart upload with artifact and detached GPG signature

Required response fields:

- `submission_id`
- `received_at`
- `submission_state`
- `validation_summary`

### `GET /api/v1/submissions/{submission_id}`

Returns validation status, moderation status, and linked publication outcome.

## Moderation endpoints

### `GET /api/v1/moderation/queue`

Returns pending and quarantined submissions visible to moderators.

### `POST /api/v1/moderation/{submission_id}/decision`

Records an explicit moderation decision.

Required request fields:

- `decision` (`accept`, `reject`, `quarantine`)
- `reason_code`
- `reason_text`

## Trust-bundle export endpoints

### `GET /api/v1/trust-bundles/current`

Returns the current trust bundle for OpenPulseHF clients.

Required top-level fields:

- `schema_version`
- `bundle_id`
- `generated_at`
- `issuer_instance_id`
- `signing_algorithms[]`
- `records[]`
- `bundle_signature`

### `GET /api/v1/trust-bundles/{bundle_id}`

Returns a specific historical trust bundle.

## Trust bundle record schema

Each `records[]` entry must include:

- `record_id`
- `station_id`
- `callsign`
- `trust_state`
- `revocation_state`
- `algorithms[]`
- `keys[]`
- `hybrid_policy`
- `valid_from`
- `valid_until`
- `evidence_summary[]`

Each `keys[]` entry must include:

- `key_id`
- `algorithm`
- `public_key`
- `fingerprint`
- `status`

## Trust evidence schema

Each evidence item must include:

- `evidence_id`
- `source_type` (`operator`, `gpg`, `tqsl`, `replication`)
- `source_instance_id`
- `verification_state`
- `collected_at`
- `weight_class`

Rules:

- TQSL-derived evidence must remain distinguishable from OpenPulseHF-native signature evidence.
- GPG evidence must identify the signing key fingerprint used for verification.

## Error model

Error responses must include:

- `schema_version`
- `error_code`
- `message`
- `request_id`
- `details[]`

Initial error codes:

- `unsupported_schema_version`
- `malformed_submission`
- `signature_verification_failed`
- `policy_rejected`
- `duplicate_submission`
- `not_found`
- `rate_limited`
- `unauthorized`
- `forbidden`

## Pagination and filtering

- List endpoints use cursor pagination.
- Responses must include `next_cursor` when more results exist.
- Servers must define and enforce maximum page sizes.

## Audit linkage requirements

- Submission, moderation, and publication responses must include stable IDs that can be joined against audit records.
- Trust-bundle exports must include provenance metadata linking the bundle to the publication state used to generate it.

## Open questions

- Whether trust-bundle exports should also be offered in a compact binary format in addition to JSON.
- Whether unauthenticated lookup should expose evidence summaries or only aggregate trust state.