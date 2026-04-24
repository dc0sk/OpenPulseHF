---
project: openpulsehf
doc: docs/pki-tooling-spec-map.md
status: living
last_updated: 2026-04-24
---

# PKI Tooling Specification Map

## Purpose

This document defines the PKI tooling specification set, its document roles, and the normative versus informative classification used for implementation and review.

## Classification rules

- Normative documents define required behavior, constraints, or acceptance criteria.
- Informative documents provide guidance, planning, operations, or explanatory context.
- In conflicts, newer normative requirements supersede older informative guidance.

## Canonical document map

| Document | Role | Classification |
|----------|------|----------------|
| docs/pki-tooling-requirements.md | Requirements baseline for PKI project scope and constraints | Normative |
| docs/pki-tooling-architecture.md | System and component architecture | Normative |
| docs/pki-tooling-api.md | API contract and trust-bundle export schema | Normative |
| docs/pki-tooling-data-model.md | Canonical entities, transitions, and migration policy | Normative |
| docs/pki-tooling-trust-policy.md | Moderation and trust-policy decision semantics | Normative |
| docs/pki-tooling-conformance.md | Conformance gates and test obligations | Normative |
| docs/pki-tooling-rollout-plan.md | Phased implementation milestones | Informative |
| docs/pki-tooling-operations-runbook.md | Operational procedures and incident playbooks | Informative |
| docs/pki-tooling-glossary.md | Shared vocabulary and terminology rules | Informative |

## Reading order

Recommended implementation reading order:

1. docs/pki-tooling-requirements.md
2. docs/pki-tooling-architecture.md
3. docs/pki-tooling-api.md
4. docs/pki-tooling-data-model.md
5. docs/pki-tooling-trust-policy.md
6. docs/pki-tooling-conformance.md

Recommended operational reading order:

1. docs/pki-tooling-rollout-plan.md
2. docs/pki-tooling-operations-runbook.md
3. docs/pki-tooling-glossary.md

## Change control guidance

- Changes to normative docs should include corresponding conformance updates in docs/pki-tooling-conformance.md.
- Changes that alter terms should update docs/pki-tooling-glossary.md.
- Rollout and runbook updates should reference affected normative document sections.

## Traceability guidance

- Requirements in docs/pki-tooling-requirements.md should map to API, data model, trust policy, and conformance entries.
- Trust-policy decisions should map to moderation workflow and test cases.
- Export-schema changes should map to API and conformance updates.

## Open questions

- Whether rollout-plan milestones should become normative once implementation begins.
- Whether operations runbook SLO targets should be promoted into normative requirements.