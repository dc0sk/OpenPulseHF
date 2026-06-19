#!/usr/bin/env bash
# Run the test matrix and archive the results.
#
# Usage:
#   scripts/run-test-matrix.sh           # quick tier (~30s)
#   scripts/run-test-matrix.sh --full    # full tier (~15min)
#   scripts/run-test-matrix.sh --output /path/to/reports

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

GIT_SHA=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")
TS=$(date -u +%Y-%m-%dT%H%M%S)

echo "==> Running openpulse-testmatrix ${*:-}"
cargo run --no-default-features -p openpulse-testmatrix -- "$@"

ARCHIVE_DIR="docs/dev/test-reports/archive/${TS}-${GIT_SHA}"
mkdir -p "$ARCHIVE_DIR"
cp -r docs/dev/test-reports/latest/. "$ARCHIVE_DIR/"
echo "==> Archived to $ARCHIVE_DIR"
