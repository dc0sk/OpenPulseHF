---
project: openpulsehf
doc: docs/dev/project.md
status: living
last_updated: 2026-07-04
---

# Project management

The roof over the project-tracking documents in [`docs/dev/project/`](project/): start here for how
the project is steered and where each tracking artifact lives.

## Tracking documents (`docs/dev/project/`)

| Document | Purpose |
|---|---|
| [roadmap.md](project/roadmap.md) | Phase plan, execution order, and phase-gate history |
| [backlog.md](project/backlog.md) | Parked / deferred work items not yet scheduled |
| [changelog.md](project/changelog.md) | Development-facing change log (companion to the root [CHANGELOG.md](../../CHANGELOG.md)) |
| [traceability.md](project/traceability.md) | Requirement → design decision → implementation → tests → results ledger |
| [traceability-matrix.md](project/traceability-matrix.md) | Numbered REQ-ID ↔ CAP-ID coverage matrix (forward + backward) |

Related, one level up: [requirements.md](requirements.md) (numbered REQ-IDs) and
[implementation-matrix.md](implementation-matrix.md) (feature → implementation → test summary).

## Decision ownership

- Architecture decisions are made at workspace level and documented in docs/architecture.md.
- CLI behavior is source-of-truth for user-visible operation semantics.
- Plugin API changes require explicit compatibility notes in docs/releasenotes.md.

## Working agreements

- Changes flow through feature branches and pull requests.
- CI must pass before merge.
- Documentation updates are part of feature completion, not follow-up work.
- Substantive changes carry the requirement → design → implementation → tests → results chain into the
  commit/PR and the [traceability ledger](project/traceability.md); the acceptance table in
  [CLAUDE.md](../../CLAUDE.md) stays current (requirement ↔ acceptance test).

## Release governance

- Public behavior changes must include changelog and release note entries.
- Version bumps must include companion documentation updates.
