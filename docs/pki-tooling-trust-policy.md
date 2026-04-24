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

---

## Connection trust levels and signing modes

> Status: design discussion — not yet finalised. Further review required before implementation.

### Motivation

HF bandwidth is severely constrained (300–2400 baud typical). Transmitting full asymmetric signatures per packet consumes a disproportionate share of available throughput. At the same time, signature-less sessions must carry a measurable trust penalty so that operators and automation can reason about the security posture of a connection.

### Certificate distribution model

- Certificates are **never transmitted over air by default**.
- Peers resolve certificates asynchronously via an internet-accessible identity database and local cache before or during session establishment.
- A peer **may** request a certificate over air if out-of-band resolution fails; the responding peer should honour the request but this path carries a trust penalty (see levels below).
- This mirrors the user's original idea and is compatible with identity-based key derivation schemes (callsign-rooted keys) as a future option.

### Session key establishment

- One asymmetric handshake at session start derives a shared symmetric session key (comparable to TLS 1.3 key schedule).
- Steady-state packets carry an HMAC tag (~16–32 bytes) over `(packet_content || sequence_number || session_id)`.
- The sequence number in the HMAC input defeats recording-and-replay / content-substitution attacks — a recorded HMAC cannot be reattached to different content or at a different sequence position.
- Full asymmetric signature per packet is **not** used in normal mode.

### Public key trust levels (GPG-style)

These levels apply to a peer's public key / identity record, independent of any active connection:

| Level | Meaning |
|---|---|
| `full` | Key verified through strong out-of-band evidence (e.g. direct exchange, TQSL corroboration, operator-explicit acceptance) |
| `marginal` | Key seen from multiple independent sources but not directly verified |
| `unknown` | Key present but no verification performed |
| `untrusted` | Key actively flagged as suspect or conflicting |
| `revoked` | Key revoked; reject all sessions |

### Connection trust levels

These levels apply to an **active session** and are derived from the public key trust level plus the certificate acquisition path:

| Level | Conditions |
|---|---|
| `verified` | Key trust is `full`; certificate obtained via out-of-band DB |
| `psk-verified` | Certificate delivered over air AND validated against a pre-shared secret (PSK); trust level is elevated above plain over-air delivery but below full out-of-band verification |
| `reduced` | Key trust is `marginal`, OR certificate delivered over air without PSK validation |
| `unverified` | Key trust is `unknown`; certificate obtained via out-of-band DB |
| `low` | Key trust is `unknown` AND certificate delivered over air without PSK validation |
| `rejected` | Key is `untrusted` or `revoked`; session must not proceed |

Rationale for the air-delivery penalty: over-air certificate delivery cannot rule out a man-in-the-middle inserting a fabricated certificate. The receiver has no way to distinguish the legitimate peer's certificate from an injected one at the RF layer.

**PSK exception**: if both peers hold a pre-shared secret, the delivered certificate can be bound to the PSK (e.g. HMAC of the certificate bytes under the PSK is transmitted alongside it). An attacker without the PSK cannot forge a valid binding, so the MitM threat is mitigated. The only remaining out-of-band dependency is the PSK itself — which may be exchanged in person, via a separate secure channel, or pre-provisioned at manufacture/licensing time. This allows air-only operation with elevated trust, independent of internet connectivity.

### Signing modes

#### Normal mode (default)
- Certificate distribution: out-of-band only.
- Session auth: one asymmetric handshake → symmetric HMAC for all subsequent packets.
- Connection trust level: `verified` or `reduced` depending on key trust level.
- Use case: general data, messaging, beacon, status.


#### PSK mode
- Certificate distribution: certificate delivered over air, bound with a PSK-derived HMAC (see connection trust levels above).
- Session auth: same as normal mode (asymmetric handshake → symmetric HMAC).
- Connection trust level: `psk-verified` (independent of internet/OOB, provided PSK is pre-provisioned).
- Use case: field deployments with no internet access; pre-provisioned station pairs; emergency nets where OOB lookup is not possible but a shared group secret is available.
- Security note: the PSK is the single remaining trust anchor; its compromise degrades the session to `low` trust retroactively.

#### Relaxed mode
- Certificate distribution: out-of-band preferred; over-air permitted on request.
- Session auth: same as normal mode.
- Connection trust level: `reduced` or `low`.
- Use case: emergency comms, first contact with unknown station, no internet access.

#### Paranoid mode
- Certificate distribution: out-of-band only; over-air requests rejected.
- Session auth: **full asymmetric signature on every transmitted frame** (e.g. Ed25519, 64 bytes per packet).
- Connection trust level: `verified` only; session is dropped if key trust is not `full`.
- Use case: remote station control, command transmission, high-value data, automated relay instructions.
- Bandwidth cost: ~64 bytes overhead per packet (Ed25519); operator must accept this consciously.

### Interaction with policy profiles

| Policy profile | Permitted signing modes | Minimum connection trust for publication acceptance |
|---|---|---|
| `strict` | normal, paranoid | `verified` |
| `balanced` | normal, psk, relaxed | `psk-verified` or better |
| `permissive` | normal, psk, relaxed | `reduced` or better |

Paranoid mode is always permitted regardless of policy profile when explicitly configured by the operator.

### Open sub-questions

- Whether connection trust level should be recorded in the session audit trail.
- Whether `reduced`-trust sessions should be allowed to submit identity records or only consume them.
- Exact key derivation scheme for the symmetric session key (X25519 ECDH + HKDF is the leading candidate).
- Whether signing mode should be negotiated in-band during handshake or pre-configured per callsign/group.
- PSK lifecycle: rotation frequency, revocation procedure, and storage requirements (secure enclave vs. config file).
- Whether a group PSK (shared across multiple stations) should be permitted or only pairwise PSKs.
- Whether PSK validation failure should fall back to `low` trust or abort the session entirely (fail-closed is safer).
- Whether some federation peers should be allowed as authoritative for specific evidence classes.