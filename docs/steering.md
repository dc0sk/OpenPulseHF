---
project: openpulsehf
doc: docs/steering.md
status: living
last_updated: 2026-04-23
---

# Steering

## Decision ownership

- Architecture decisions are made at workspace level and documented in docs/architecture.md.
- CLI behavior is source-of-truth for user-visible operation semantics.
- Plugin API changes require explicit compatibility notes in docs/releasenotes.md.

## Working agreements

- Changes flow through feature branches and pull requests.
- CI must pass before merge.
- Documentation updates are part of feature completion, not follow-up work.

## Release governance

- Public behavior changes must include changelog and release note entries.
- Version bumps must include companion documentation updates.
