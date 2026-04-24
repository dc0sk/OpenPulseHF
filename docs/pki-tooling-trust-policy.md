---
project: openpulsehf
doc: docs/pki-tooling-trust-policy.md
status: living
last_updated: 2026-04-24
---

# PKI Moderation and Trust Policy Workflow

## Purpose

This document defines operational trust-policy and moderation workflows for the separate PKI tooling project.

It complements:

- requirements in docs/pki-tooling-requirements.md
- architecture in docs/pki-tooling-architecture.md
- API schema in docs/pki-tooling-api.md
- canonical entities in docs/pki-tooling-data-model.md

## Policy design goals

- Keep trust decisions explicit, auditable, and reversible where safe.
- Separate publication acceptance from trust recommendation.
- Ensure GPG and TQSL evidence are policy inputs, not hard-coded authority.
- Keep fail-closed behavior for signature and revocation conflicts.

## Roles and responsibilities

- Operator: manages local trust policy profile and replication controls.
- Moderator: reviews submissions and applies moderation decisions.
- Auditor: reviews moderation and trust decision history; no publication privileges.
- Automation actor: performs configured verification and routine policy actions under restricted scope.

## Decision domains

### Publication decision

Controls whether an identity revision is visible in published lookup and exports.

States:

- pending
- published
- quarantined
- rejected
- revoked

### Trust recommendation decision

Controls recommendation presented to clients and operators.

States:

- trusted
- untrusted
- unknown
- revoked
- advisory

Rule:

- A record may be published while still carrying `unknown` or `advisory` trust recommendation.

## Baseline policy profiles

### strict

- Requires verified OpenPulseHF-native signatures for publication.
- Requires explicit moderator acceptance for first-time station identity publication.
- Treats GPG and TQSL evidence as advisory only.

### balanced (default)

- Requires valid signature and schema checks for publication.
- Allows automation acceptance for low-risk updates under configured guardrails.
- Treats GPG evidence as weighted and TQSL as advisory by default.

### permissive

- Permits unsigned submissions only into quarantine.
- Allows broader automation-driven acceptance paths.
- Must still block publication on revocation conflicts or failed signature verification.

## Verification and moderation pipeline

1. Submission intake:
   - Assign `submission_id`
   - Persist raw artifact and detached signature (if present)
2. Structural validation:
   - Check schema version
   - Check required fields
   - Check duplicate record/revision constraints
3. Signature validation:
   - Verify OpenPulseHF-native signatures where present
   - Verify GPG detached signature when supplied
4. Evidence extraction:
   - Derive trust evidence rows for operator, gpg, tqsl, replication sources
5. Policy evaluation:
   - Apply profile rules and instance overrides
6. Moderation decision:
   - Accept, reject, or quarantine with reason
7. Publication update:
   - Publish accepted revisions and regenerate trust-bundle projections
8. Audit emission:
   - Record moderation event and linked audit event

## Decision matrix baseline

- Valid signature + no conflicts + known station rollover path:
  - strict: moderator accept required
  - balanced: automation may accept under guardrails
  - permissive: automation accept allowed
- Valid signature + revocation conflict:
  - all profiles: quarantine or reject (never publish)
- Invalid signature:
  - all profiles: reject
- Unsigned submission:
  - strict: reject
  - balanced: quarantine
  - permissive: quarantine
- GPG-valid but OpenPulseHF signature missing:
  - strict: quarantine
  - balanced: quarantine
  - permissive: quarantine
- TQSL evidence present but cryptographic identity mismatch:
  - all profiles: do not raise trust recommendation from unknown

## Guardrails for automation acceptance

Automation may auto-accept only when all are true:

- submission is structurally valid
- required signatures verify
- no active revocation conflicts
- station identity lineage continuity is valid
- policy profile allows automation acceptance for the submission class

Otherwise, automation must route to quarantine.

## Revocation handling policy

- Revocation records take precedence over non-revoked trust recommendations.
- Key-level revocation impacts only targeted keys unless escalated by policy.
- Record-level revocation sets trust recommendation to `revoked`.
- Conflicting revocations from replicated peers require quarantine until moderator resolution.

## GPG and TQSL policy treatment

### GPG

- Use GPG verification as evidence input with explicit policy weighting.
- Do not infer trust solely from local web-of-trust assumptions.
- Record signer fingerprint and signature status in evidence metadata.

### TQSL

- Treat TQSL evidence as supplemental identity corroboration.
- Never use TQSL alone to elevate a record to `trusted` without cryptographic key continuity policy satisfaction.
- Preserve TQSL provenance details for moderator visibility.

## Federation and replication policy

- Imported records are marked with source instance metadata.
- Replicated trust recommendations are advisory until local policy evaluation completes.
- Local revocation decisions may override imported non-revoked states.
- Operators can define per-peer allow/deny filters for imported evidence classes.

## Change control and policy lifecycle

- Policy profiles are versioned artifacts.
- Policy changes require:
  - effective timestamp
  - change reason
  - actor identity
  - migration note for impacted records
- Major policy changes should trigger a recomputation pass for trust recommendations.

## Audit and observability requirements

- Every moderation decision must emit a linked moderation and audit event.
- Every policy evaluation must record profile version and decision outcome class.
- Dashboards should expose:
  - acceptance rate by source type
  - quarantine rate by reason code
  - trust recommendation distribution
  - revocation conflict rate
  - automation versus moderator decision ratio

## Conformance checkpoints

- Signature-failure submissions are never published.
- Revocation conflicts always prevent direct publication.
- TQSL-only evidence never auto-promotes to trusted.
- Policy profile transitions preserve auditability and effective-time semantics.
- Recomputed trust recommendations are deterministic for fixed input data and policy version.

## Open questions

- Whether policy profile inheritance should be supported per station group or tenant.
- Whether some federation peers should be allowed as authoritative for specific evidence classes.