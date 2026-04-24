---
project: openpulsehf
doc: docs/pki-tooling-architecture.md
status: living
last_updated: 2026-04-24
---

# PKI Tooling Architecture

## System goals

- Provide a separate deployable trust and identity service for OpenPulseHF ecosystems.
- Keep trust publication, moderation, replication, and export workflows explicit and auditable.
- Preserve offline-capable OpenPulseHF verification by making trust-bundle export a first-class output.
- Support operator-managed trust policy rather than implicit global authority.

## Core architecture

1. An operator or automation client submits identity or trust material through web, CLI, or API entrypoints.
2. The ingestion layer stores the raw submission and verifies attached signatures and schema validity.
3. Moderation and policy evaluation decide whether the submission is accepted, quarantined, or rejected.
4. Accepted records are written to the canonical database with revision history and trust evidence linkage.
5. Lookup APIs and web views expose published identity state, revocation state, and trust metadata.
6. Export jobs build versioned trust bundles for OpenPulseHF clients and optional peer replication.

## Service architecture

| Component | Role |
|-----------|------|
| Web frontend | Human-facing lookup, submission, moderation, and audit views |
| API service | Versioned programmatic interface for lookup, submission, status, and trust-bundle export |
| Ingestion and verification pipeline | Validates schema, signatures, policy, and duplicate state before publication |
| Moderation engine | Applies review workflow, quarantine policy, and operator decisions |
| Canonical database | Stores identity records, trust evidence, revocations, revisions, and audit metadata |
| Export service | Produces OpenPulseHF trust bundles and replication feeds |
| Replication worker | Imports and exports trust material across peer PKI instances under policy |
| Audit log store | Tamper-evident record of administrative, publication, and replication events |

## Data architecture

- Identity records are append-only at the revision layer, with a current published view derived from accepted revisions.
- Trust evidence is normalized from distinct source classes: operator assertion, GPG verification, TQSL evidence, and peer replication.
- Revocations are first-class records linked to both subject identity and issuer identity.
- Submission artifacts are preserved so moderators can inspect original uploads and detached signatures.
- Trust-bundle exports are generated from published views rather than directly from raw submissions.

## Trust and verification architecture

- Signature verification is performed at ingestion time and never deferred to moderation alone.
- GPG-signed submissions are evaluated under explicit local trust policy rather than default web-of-trust assumptions.
- TQSL-derived evidence is ingested as supplemental trust input and must remain policy-weighted, not absolute.
- OpenPulseHF-native identities must support Ed25519, ML-DSA, and hybrid signing metadata.
- Trust decisions are separable from publication state so an identity can be published while still marked advisory or pending local trust.

## Publication workflow architecture

- Web and API entrypoints both feed the same ingestion pipeline.
- Each submission receives a stable submission ID and immutable audit trail.
- Moderation state transitions are explicit: pending, accepted, quarantined, rejected, revoked.
- Publication visibility derives from accepted state only.
- Replacement and key-rollover workflows maintain forward links and historical lineage.

## API architecture

- Public lookup endpoints expose published identity state, revocations, and trust-bundle export metadata.
- Authenticated endpoints handle submission, moderation, and replication administration.
- API versioning is path- or media-type-based and must allow schema evolution without ambiguous downgrade behavior.
- Rate limiting and pagination are applied at the API layer, not left to downstream storage.

## Web frontend architecture

- Public pages support station lookup, fingerprint lookup, revocation visibility, and trust metadata display.
- Authenticated operator views support submission tracking and trust-bundle generation status.
- Moderator views expose validation failures, quarantine queues, and evidence trails.
- Frontend terminology should match OpenPulseHF trust-store and identity language to reduce operator confusion.

## Replication and federation architecture

- Federation is policy-driven and optional.
- Imported records preserve original signer identity, source instance metadata, and validation status.
- Replication transport may be pull, push, or hybrid, but imported material must remain distinguishable from locally accepted publication.
- Export filtering must support partial trust views and allow operators to restrict which records are shared.

## Deployment architecture

- The service must be deployable in self-hosted environments with a database, object storage for submission artifacts, and reverse-proxy-compatible web/API endpoints.
- Stateless API and web components should scale independently from background verification and export workers.
- Backup and restore procedures must cover the canonical database, audit log store, and preserved submission artifacts.
- Operators should be able to disable federation, web submission, or service-side signing features independently.

## Security architecture

- Administrative and moderator roles are distinct and enforced through role-based authorization.
- Service-side signing keys, if used, are isolated behind protected keystore or HSM interfaces.
- Audit logging is append-only or tamper-evident and covers publication, moderation, trust-policy, and replication events.
- The architecture must fail closed on verification errors, schema mismatch, or revocation conflicts.
- Exported trust bundles are signed so OpenPulseHF clients can verify provenance independently of transport.

## Integration architecture for OpenPulseHF

- OpenPulseHF clients consume trust-bundle exports, not live service dependencies, in the default deployment model.
- Export bundles preserve signer algorithm, hybrid-policy requirements, validity windows, and revocation state.
- Relay-policy and signed-transfer verification inputs are derived from the same exported trust model.
- Client tooling may optionally query the PKI service directly for operator diagnostics and trust refresh workflows.

## Observability architecture

- Metrics must distinguish submission volume, acceptance rate, quarantine rate, revocation count, export frequency, and replication lag.
- Audit events must be queryable by submission ID, identity record ID, station identifier, and moderator action.
- Health reporting should separately expose database health, verification worker backlog, export freshness, and replication state.

## Documentation process constraints

- API schema, trust-bundle format, and moderation workflow must be documented alongside implementation.
- Architecture changes that affect trust semantics or export schema require requirements updates and migration notes.