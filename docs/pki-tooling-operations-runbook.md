---
project: openpulsehf
doc: docs/pki-tooling-operations-runbook.md
status: living
last_updated: 2026-04-24
---

# PKI Tooling Operations Runbook

## Purpose

This runbook defines day-to-day operational procedures for the separate PKI tooling service used with OpenPulseHF trust publication and lookup workflows.

## Audience

- Service operators
- Moderators
- Security responders
- On-call engineers

## Service boundaries

- Web frontend
- API service
- Ingestion and verification workers
- Moderation queue services
- Trust-bundle export jobs
- Replication workers (if enabled)
- Canonical database
- Submission artifact storage
- Audit event store

## Operational SLO targets

- Lookup API availability: 99.9% monthly
- Moderation queue API availability: 99.5% monthly
- Trust-bundle export freshness: new export available within 10 minutes of accepted publication
- P95 public lookup latency: <= 300 ms under normal load
- P95 moderation queue query latency: <= 800 ms under normal load

## Daily checks

1. Verify API and web health endpoints report healthy state.
2. Verify worker backlog stays within expected queue depth.
3. Verify trust-bundle export timestamp is current.
4. Verify no unresolved revocation conflicts are older than policy threshold.
5. Verify audit event ingestion is continuous and monotonic.

## Weekly checks

1. Run backup integrity verification for database and artifact store.
2. Run restore simulation in non-production environment.
3. Review moderation decision distribution for unusual spikes.
4. Review replication lag and peer import error rates.

## Monthly checks

1. Rotate operational credentials and review key access policies.
2. Review policy profile changes and effective-time audit records.
3. Review security findings and unresolved high-risk items.
4. Rehearse incident response tabletop for signature failure or trust-bundle corruption scenarios.

## Backup and restore procedure

### Backup scope

- Canonical database snapshots
- Incremental database logs
- Submission artifacts and detached signatures
- Audit event store
- Service configuration and policy profile versions

### Backup cadence

- Full database backup: daily
- Incremental log backup: every 15 minutes
- Artifact store backup: every 6 hours
- Audit store backup: every 6 hours

### Restore validation steps

1. Restore database to isolated environment.
2. Restore artifacts and audit data for matching time window.
3. Verify schema version alignment.
4. Recompute a test trust-bundle and compare against expected snapshot hash.
5. Verify revocation and moderation histories are intact for sampled records.

## Incident response playbooks

### Incident A: signature verification failures spike

Symptoms:

- Sudden increase in `signature_verification_failed` errors
- Large moderation quarantine growth

Actions:

1. Confirm whether issue is algorithm-specific or source-specific.
2. Validate cryptographic dependency and key material health.
3. Pause automation acceptance pathways if false negatives are suspected.
4. Route affected submissions to quarantine until root cause is confirmed.
5. Publish operator advisory with expected mitigation timeline.

Exit criteria:

- Signature verification pass rate returns to baseline.
- Quarantined backlog triage complete for affected window.

### Incident B: trust-bundle export corruption or stale exports

Symptoms:

- Bundle signature verification failures
- Export freshness SLO breached

Actions:

1. Disable publication of new bundle as current if signature check fails.
2. Roll back to most recent verified bundle as current pointer.
3. Restart export job workers and verify deterministic regeneration.
4. Verify signing key access path and timestamp consistency.
5. Reissue signed bundle and notify downstream consumers.

Exit criteria:

- Current bundle passes signature verification and schema checks.
- Export freshness SLO restored.

### Incident C: revocation conflict storm from replication peers

Symptoms:

- Rapid increase in revocation conflicts from imported records
- Moderator queue overload

Actions:

1. Throttle or pause imports from high-conflict peers.
2. Apply peer-specific filters to reduce non-actionable evidence imports.
3. Prioritize moderator workflow on high-impact stations first.
4. Escalate peer trust-policy review.

Exit criteria:

- Conflict intake reduced to manageable levels.
- High-impact conflicts resolved or quarantined with explicit ownership.

## Moderation escalation path

1. Moderator marks submission as escalated with reason code.
2. Security responder reviews evidence and revocation context.
3. Operator decides final policy override if needed.
4. Decision and rationale are recorded in moderation and audit logs.

Severity levels:

- Sev-3: routine moderation ambiguity
- Sev-2: potential trust-state inconsistency affecting active users
- Sev-1: confirmed publication integrity risk or compromised signing path

## Change management

- All production policy changes require change record, approver identity, and effective timestamp.
- Schema migrations require tested rollback or documented non-reversible warning.
- New algorithm support must pass compatibility and trust-bundle validation tests before enablement.

## Rollback guidance

- Application rollback must not revert canonical audit events.
- Policy rollback must create a new policy version (no in-place mutation of historical versions).
- Data rollback must preserve evidence provenance and moderation history.

## Access control operations

- Enforce least privilege for operator, moderator, auditor, and automation roles.
- Use short-lived credentials for automation where possible.
- Review and revoke dormant privileged accounts monthly.

## Observability and alerting baselines

Critical alerts:

- trust-bundle signature failure
- export freshness SLO breach
- audit ingestion interruption
- database replication lag above threshold
- moderation queue growth beyond threshold

Dashboard minimums:

- ingestion throughput and error rate by source type
- moderation queue depth and aging
- trust recommendation distribution trends
- revocation conflict rates
- export generation duration and success rate

## Communication templates

### Internal incident update

- incident ID
- start time
- impacted components
- current mitigation
- next update time

### External operator advisory

- issue summary
- impact to lookup/publication/trust export
- recommended temporary action
- expected recovery window

## Post-incident review requirements

- Complete review within 5 business days for Sev-1 and Sev-2 incidents.
- Capture root cause, detection gaps, and preventive actions.
- Track preventive actions to completion with owner and due date.

## Open questions

- Whether to enforce active-active regional deployments before federation leaves beta.
- Whether moderators should have emergency temporary peer-block controls without operator approval.