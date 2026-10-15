---
project: openpulsehf
doc: docs/dev/project.md
status: living
last_updated: 2026-10-15
---

# Project management

The roof over the project-tracking documents in [`docs/dev/project/`](project/): start here for how
the project is steered and where each tracking artifact lives.

**The plan of record is [workplan.md](project/workplan.md)**: Release 1 (`v0.17.0`) scope, milestones
M0–M4, triage rules and the decision log. Where any other tracking document disagrees with it, the
work plan wins.

## Tracking documents (`docs/dev/project/`)

| Document | Purpose |
|---|---|
| [workplan.md](project/workplan.md) | **The plan:** Release 1 scope, milestones and status, triage rules, decision log, weekly log |
| [maintainer-todo.md](project/maintainer-todo.md) | What waits on the maintainer: hardware runs, credentials, decisions |
| [release-1.0-criteria.md](project/release-1.0-criteria.md) | What **1.0** requires (Release 1 = `v0.17.0` is a narrower first release) |
| [acceptance-criteria.md](project/acceptance-criteria.md) | Requirement → acceptance test command, one row each |
| [roadmap.md](project/roadmap.md) | Phase-by-phase history of what shipped, with PR numbers (no longer schedules work) |
| [backlog.md](project/backlog.md) | Parked / deferred work items, all `post-release` unless the work plan says otherwise |
| [changelog.md](project/changelog.md) | Development-facing change log (companion to the root [CHANGELOG.md](../../CHANGELOG.md)) |
| [traceability.md](project/traceability.md) | Requirement → design decision → implementation → tests → results ledger |
| [traceability-matrix.md](project/traceability-matrix.md) | Numbered REQ-ID ↔ CAP-ID coverage matrix (forward + backward) |

Related, one level up: [requirements.md](requirements.md) (numbered REQ-IDs) and
[implementation-matrix.md](implementation-matrix.md) (feature → implementation → test summary).

## Decision ownership

- Release scope, milestones and triage are decided in the work plan's decision log ([workplan.md](project/workplan.md)).
- Architecture decisions are made at workspace level and documented in [design/architecture.md](design/architecture.md).
- CLI behavior is source-of-truth for user-visible operation semantics.
- Plugin API changes require explicit compatibility notes in [docs/releasenotes.md](../releasenotes.md).

## Working agreements

- Changes flow through feature branches and pull requests.
- The required CI checks must pass before merge; `scripts/gate.sh` is run daily and before every tag (work plan decision 9).
- Documentation updates are part of feature completion, not follow-up work.
- Substantive changes carry the requirement → design → implementation → tests → results chain into the
  commit/PR and the [traceability ledger](project/traceability.md); the acceptance table in
  [acceptance-criteria.md](project/acceptance-criteria.md) stays current (requirement ↔ acceptance test).
- Every PR names the work-plan milestone it serves and updates that milestone's status line, or says
  `post-release` with a reason.

## Release governance

- Public behavior changes must include changelog and release note entries.
- Version bumps must include companion documentation updates.
