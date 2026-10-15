---
project: openpulsehf
doc: docs/dev/sbom.md
status: living
last_updated: 2026-10-15
---

# SBOM

## Policy

- The repository commits an SPDX 2.3 SBOM at `SBOM.spdx.json` (repository root).
- It is regenerated on every version bump and attached to the GitHub release
  ([release-checklist.md](release-checklist.md)).
- Between releases it may lag the dependency graph; `scripts/generate-sbom.sh --check` says whether
  it does.

## Process

1. Install the tools if not present: `cargo install cargo-sbom`, and `jq`.
2. Regenerate from the repository root with the script, not `cargo sbom` directly.
3. Commit `SBOM.spdx.json` with the release documentation updates.

## Commands

```bash
scripts/generate-sbom.sh          # rewrites SBOM.spdx.json
scripts/generate-sbom.sh --check  # fails if the committed SBOM is stale
```

The script normalises what `cargo sbom` changes on every run (the `created` timestamp, the random
`documentNamespace`, and the hash order of packages and relationships), so a diff in `SBOM.spdx.json` means the dependencies
changed.
