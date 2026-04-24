---
project: openpulsehf
doc: docs/trust-store-file-format.md
status: living
last_updated: 2026-04-24
---

# Trust-Store File Format and Migration Policy

## Purpose

This document defines:

- the on-disk trust-store file format consumed by OpenPulseHF components
- version-compatibility rules for trust-store readers/writers
- required migration procedures between trust-store format versions

It complements:

- docs/pki-tooling-api.md (trust-bundle export schema)
- docs/pki-tooling-data-model.md (canonical entities and DB migration policy)
- docs/pki-tooling-trust-policy.md (trust decision semantics)

## Scope

In scope:

- trust-store serialization format (JSON)
- required top-level metadata
- record and key entry constraints
- signature and integrity metadata
- migration and rollback policy

Out of scope:

- transport protocol for distribution
- PKI tooling database schema migration details
- key generation and cryptographic algorithm selection policy

## File naming and location

Recommended file names:

- `trust-store.current.json` (active)
- `trust-store.backup.<timestamp>.json` (automatic rollback point)
- `trust-store.staged.json` (temporary write target)

Default storage root is implementation-defined, but writes must be atomic by rename from staged to current.

## Canonical format

The trust-store is a UTF-8 JSON object.

Required top-level fields:

- `format_version`: trust-store format version string, `major.minor.patch`
- `generated_at`: RFC 3339 timestamp
- `source`: object describing issuing instance and bundle provenance
- `policy_profile`: active trust policy profile used by importer (`strict`, `balanced`, `permissive`)
- `records`: array of trust entries
- `signature`: detached-signature metadata for the store payload

### Source object

Required fields:

- `issuer_instance_id`
- `bundle_id`
- `bundle_schema_version`

Optional fields:

- `fetched_at`
- `origin_url`

### Record object

Required fields for each `records[]` item:

- `record_id`
- `station_id`
- `callsign`
- `trust_state` (`trusted`, `untrusted`, `unknown`, `revoked`, `advisory`)
- `revocation_state` (`none`, `key_revoked`, `revision_revoked`, `record_revoked`)
- `algorithms` (non-empty string array)
- `keys` (non-empty key array)
- `hybrid_policy`
- `valid_from`
- `valid_until`
- `evidence_summary` (array; may be empty)

### Key object

Required fields for each `keys[]` item:

- `key_id`
- `algorithm`
- `public_key`
- `fingerprint`
- `status` (`active`, `deprecated`, `revoked`, `superseded`)

### Signature object

Required fields:

- `algorithm`
- `key_id`
- `signature_b64`
- `signed_payload_sha256`

Rules:

- `signed_payload_sha256` must be computed over canonical JSON for all top-level fields except `signature`.
- Signature verification failure must fail closed: importer must reject update and keep prior active store.

## Minimal example (v1)

```json
{
  "format_version": "1.0.0",
  "generated_at": "2026-04-24T10:00:00Z",
  "source": {
    "issuer_instance_id": "instance-eu-west-1",
    "bundle_id": "bundle_01HXYZ",
    "bundle_schema_version": "1.0"
  },
  "policy_profile": "balanced",
  "records": [
    {
      "record_id": "rec_01",
      "station_id": "station-42",
      "callsign": "W1ABC",
      "trust_state": "trusted",
      "revocation_state": "none",
      "algorithms": ["ed25519"],
      "keys": [
        {
          "key_id": "k1",
          "algorithm": "ed25519",
          "public_key": "BASE64...",
          "fingerprint": "SHA256:...",
          "status": "active"
        }
      ],
      "hybrid_policy": "allow_hmac_after_handshake",
      "valid_from": "2026-01-01T00:00:00Z",
      "valid_until": "2027-01-01T00:00:00Z",
      "evidence_summary": []
    }
  ],
  "signature": {
    "algorithm": "ed25519",
    "key_id": "issuer-key-1",
    "signature_b64": "BASE64...",
    "signed_payload_sha256": "hex..."
  }
}
```

## Compatibility policy

Trust-store format version follows semantic versioning:

- major: breaking structural change
- minor: backward-compatible additive change
- patch: non-structural clarification or bug fix

Reader compatibility rules:

- Reader `X.Y` must accept store `X.y` where `y <= Y`.
- Reader must reject store with different major version.
- Reader may ignore unknown additive fields when major matches.

Writer policy:

- Writers must emit the lowest version that can represent data losslessly.
- Writers must not silently downgrade when required fields would be lost.

## Migration policy

Each format migration must provide:

- migration identifier and source/target versions
- deterministic forward transformation
- rollback strategy or explicit non-reversible declaration
- validation checklist
- operator communication note

### Required migration phases

1. Preflight

- verify current store signature
- verify store parses and required fields exist
- create timestamped backup copy

2. Transform

- apply deterministic transformation from source schema to target schema
- preserve original `source.bundle_id` and provenance

3. Validate

- validate target against target schema
- verify semantic invariants:
  - no duplicate `record_id`
  - no duplicate key fingerprint among active keys
  - all `valid_until >= valid_from`

4. Activate

- write target to staged file
- fsync staged content
- atomically rename staged to current

5. Post-activate

- reload trust store in reader process
- emit audit event with migration id and versions

### Rollback policy

Rollback is required for all major migrations unless explicitly marked non-reversible.

Rollback trigger conditions:

- signature verification failure after migration
- schema validation failure
- runtime importer load failure

Rollback procedure:

- restore latest backup as current atomically
- reload importer
- emit rollback audit event with failure reason

## Required conformance checks

Implementations are conformant only if they:

- reject signature-invalid stores
- reject unsupported major versions
- perform atomic activation writes
- preserve backup before activation
- emit audit records for migration and rollback events

## Migration manifest template

Each migration release should include a manifest:

```yaml
migration_id: trust-store-1.0.0-to-2.0.0
source_version: 1.0.0
target_version: 2.0.0
reversible: true
forward_steps:
  - add field records[].key_origin with default "declared"
  - split trust_state "advisory" into "advisory_gpg" or "advisory_tqsl"
rollback_steps:
  - merge advisory subtypes back into "advisory"
validation:
  - schema validation passes for 2.0.0
  - no duplicate record_id
  - no duplicate active key fingerprints
operator_notice: "Major upgrade; restart importer after activation."
```

## Open questions

- Whether to define a compact binary trust-store encoding alongside JSON.
- Whether `policy_profile` should be stored as import metadata only or enforced during downstream session establishment.