#!/usr/bin/env bash
# Run the full test matrix (all channel models and payload sizes).
#
# Equivalent to: scripts/run-test-matrix.sh --full

set -euo pipefail
exec "$(dirname "${BASH_SOURCE[0]}")/run-test-matrix.sh" --full "$@"
