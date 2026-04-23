---
project: openpulse
doc: docs/sbom.md
status: living
last_updated: 2026-04-23
---

# SBOM

## Policy

- SBOM generation is optional for normal development cycles.
- When producing a release artifact set, generate and commit an SBOM file if release policy requires it.

## Suggested process

1. Install cargo-sbom if not present.
2. Generate SPDX JSON from repository root.
3. Commit the output with release documentation updates.

## Suggested command

cargo sbom --output-format spdx-json > SBOM.spdx.json
