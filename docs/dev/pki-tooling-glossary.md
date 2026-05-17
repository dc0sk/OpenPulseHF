---
project: openpulsehf
doc: docs/pki-tooling-glossary.md
status: living
last_updated: 2026-04-24
---

# PKI Tooling Glossary

## Purpose

This glossary defines canonical terminology used across the PKI tooling documentation set.

## Terms

### identity record

Stable logical identity entry for a station, referenced by `record_id` and containing publication and trust state pointers.

### identity revision

Immutable snapshot of identity metadata and key material for an identity record.

### key rollover

Controlled replacement of key material while preserving lineage continuity between old and new revisions.

### publication state

Visibility state governing whether identity data is eligible for lookup and export (`pending`, `published`, `quarantined`, `rejected`, `revoked`).

### trust recommendation

Policy-derived recommendation value intended for consumers (`trusted`, `untrusted`, `unknown`, `advisory`, `revoked`).

### trust evidence

Recorded supporting signal for trust decisions, tagged by source class (`operator`, `gpg`, `tqsl`, `replication`) and verification status.

### moderation decision

Explicit operator or moderator action on a submission (`accept`, `reject`, `quarantine`, `reopen`).

### submission

Ingested identity or trust artifact with validation and moderation lifecycle tracking.

### detached signature

Signature artifact provided separately from payload content, commonly used with GPG-signed uploads.

### revocation

Explicit declaration that a key, revision, or identity record is no longer trusted for publication or verification.

### revocation conflict

Condition where imported or local revocation assertions disagree and require policy or moderator resolution.

### trust bundle

Versioned export artifact consumed by OpenPulseHF clients, containing identity, key, trust, and revocation metadata plus provenance signature.

### bundle provenance

Metadata indicating issuer instance, generation time, schema version, and signing details for a trust bundle.

### policy profile

Named trust-policy mode (`strict`, `balanced`, `permissive`) controlling acceptance and recommendation behavior.

### policy version

Versioned identifier for a trust-policy configuration used to make a decision.

### policy effective time

Timestamp from which a specific policy version is considered active for decision logic.

### automation guardrail

Hard safety condition that must hold before automation can accept or promote submissions.

### fail-closed

Security posture where verification ambiguity or failure blocks publication or promotion rather than allowing partial acceptance.

### federation

Optional exchange of trust and identity data between PKI service instances under explicit policy controls.

### replication peer

Configured external PKI instance participating in import and/or export flows.

### local policy precedence

Rule that local trust-policy evaluation overrides imported recommendations when they conflict.

### audit event

Tamper-evident event record capturing action metadata, actor identity, entity references, and request context.

### moderation queue

Operational worklist of pending and quarantined submissions awaiting review.

### quarantine

Intermediate non-published state used to hold suspicious or unresolved submissions for manual triage.

### schema version

Version identifier for API, data model, or bundle formats used for compatibility and migration control.

### migration

Controlled schema or policy evolution operation with forward steps and rollback or reconciliation guidance.

### reconciliation

Post-migration or post-incident process for ensuring derived views and trust outputs match canonical data.

### conformance test

Test that verifies implementation behavior against documented requirements and policy semantics.

### SLO

Service-level objective for operational behavior such as availability, latency, and export freshness.

## Canonical abbreviations

- PKI: Public Key Infrastructure
- API: Application Programming Interface
- SLO: Service Level Objective
- TQSL: Trusted QSL
- ML-DSA: Module-Lattice-based Digital Signature Algorithm

## Terminology rules

- Use "trust recommendation" for policy output and "publication state" for visibility output; do not use them interchangeably.
- Use "evidence source" to describe provenance class, and "verification state" to describe validation result.
- Use "revoked" only when an explicit revocation record exists.