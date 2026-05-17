---
project: openpulsehf
doc: docs/pki-tooling-requirements.md
status: living
last_updated: 2026-04-24
---

# PKI Tooling Requirements

## Purpose

This document defines requirements for a separate PKI-oriented project that supports OpenPulseHF identity publication, lookup, verification, and trust distribution.

The project is intended to provide a database-backed trust and identity service with a web frontend, signed publication workflow, and operator-facing tooling for reviewing and distributing trust material.

## Scope

The PKI tooling project must support:

- publication of station identity records and trust metadata
- lookup of station identity and trust state through web and API interfaces
- signed submission and update workflows for identity material
- operator review, moderation, and audit workflows
- export of trust material suitable for OpenPulseHF trust stores and relay-policy evaluation

The project does not replace on-device signature verification in OpenPulseHF. It provides trust distribution and operator workflow support.

## Functional requirements

- Store station identity records in a durable database with revision history.
- Support publication of identity records, trust anchors, revocation markers, and capability metadata.
- Provide a web frontend for identity lookup, publication status, trust review, and revocation visibility.
- Provide a machine-readable API for lookup, publication, and replication workflows.
- Support authenticated uploads of identity and trust material.
- Support GPG-signed uploads as a first-class publication path for operator-submitted updates.
- Preserve uploaded detached signatures and verify them before accepting publication.
- Record which signing identity submitted each accepted or rejected upload.
- Support TQSL-derived trust evidence as an optional trust input for identity verification and operator reputation workflows.
- Allow trust policy to treat TQSL evidence as supplemental rather than authoritative by default.
- Support explicit trust states including trusted, untrusted, revoked, unknown, and pending-review.
- Support key rollover workflows that preserve historical linkage between old and replacement identities.
- Support revocation publication with effective time, reason code, and issuer identity.
- Support lookup by station identifier, callsign, fingerprint, key ID, and supported algorithm.
- Support publication and lookup of algorithm capabilities including classical, post-quantum, and hybrid signing support.
- Support export of trust bundles for OpenPulseHF clients in a versioned format.
- Support import and synchronization from peer PKI instances under explicit operator policy.
- Support moderator workflows for quarantining suspicious submissions before publication.

## Data model requirements

- Identity records must include stable record IDs, station identifier, public keys, algorithm metadata, publication timestamps, and status.
- The database schema must retain historical versions of identity records and trust decisions.
- Revocation records must link to the affected identity record and the issuer of the revocation.
- Trust evidence records must distinguish source class, including operator assertion, GPG verification, TQSL evidence, and peer replication.
- Submission records must preserve the raw uploaded artifact, detached signature if present, validation result, and moderation decision.
- Exported trust bundles must be versioned and include schema metadata for forward migration.

## Security and trust requirements

- The system must verify all signed uploads before publication.
- Unsigned uploads must be rejected by default unless an explicit local review policy permits pending manual approval.
- GPG trust evaluation must be policy-driven and must not rely solely on the local GPG web-of-trust defaults.
- TQSL-derived trust must be configurable as advisory, weighted, or disabled.
- The system must support OpenPulseHF signing identities based on Ed25519, ML-DSA, and hybrid combinations.
- Private keys for service-side signing operations must be stored in protected keystores or HSM-backed interfaces where available.
- Audit logs must record publication, moderation, revocation, trust-policy, and replication events.
- Audit logs must be append-only or tamper-evident.
- Administrative actions must require strong authentication and role-based authorization.
- Replicated trust material must preserve original signer identity and signature metadata.

## Publication workflow requirements

- Operators must be able to submit new identity material through the web frontend and a non-interactive CLI or API path.
- The publication workflow must support detached-signature uploads and signed manifest bundles.
- Publication validation must include schema validation, signature verification, algorithm-policy checks, and duplicate detection.
- Moderators must be able to approve, reject, or quarantine submissions with an auditable reason.
- Accepted publications must become visible through both the web frontend and the lookup API.
- Rejected and quarantined submissions must remain visible to moderators with full validation history.

## Lookup and API requirements

- The web frontend must provide human-readable lookup pages for station identity and trust state.
- The API must provide versioned endpoints for record lookup, trust-bundle export, revocation queries, and submission status.
- API responses must include enough metadata for OpenPulseHF clients to distinguish authoritative publication, advisory evidence, and revoked state.
- Lookup responses must expose algorithm capability and hybrid-policy metadata.
- API pagination, filtering, and rate limits must be defined and enforced.

## Non-functional requirements

- Correctness and trust integrity take priority over write throughput.
- Read-path latency for common lookup queries should remain bounded under large identity tables.
- The service must support backup, restore, and disaster-recovery procedures without silent trust-state loss.
- Schema migrations must be reversible or have documented rollback procedures.
- The web frontend must remain usable for moderators and operators on desktop and mobile form factors.
- The project must be deployable in self-hosted environments without mandatory dependence on proprietary cloud services.

## Privacy and policy requirements

- The system must allow operators to publish the minimum metadata necessary for trust lookup.
- Personally identifying metadata beyond station and trust requirements must be optional.
- Publication policy must distinguish public data, moderator-only data, and private audit data.
- Replication policies must allow operators to restrict which records are exported to peers.

## Integration requirements for OpenPulseHF

- Exported trust bundles must be consumable by OpenPulseHF trust-store tooling.
- Export format must preserve signer algorithm, hybrid-policy requirements, revocation state, and validity windows.
- The project must support generation of trust views suitable for relay admission policy and signed-transfer verification.
- OpenPulseHF clients must be able to operate without the PKI service once trust bundles are exported.

## Documentation requirements

- The project must document its trust model, moderation model, replication model, and publication workflow.
- The project must document how GPG-signed uploads are created and verified.
- The project must document how TQSL evidence is ingested, interpreted, and limited within trust policy.
- The project must publish its API schema and trust-bundle export schema.

## Open questions

- Whether GPG signatures should remain a transitional publication mechanism or a long-term supported workflow.
- Whether TQSL evidence should influence only discovery ranking or also default trust recommendations.
- Whether federation between PKI instances should be push-based, pull-based, or both.