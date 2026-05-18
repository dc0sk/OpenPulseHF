#!/usr/bin/env bash
# Execute the full Phase 5.5-reg on-air validation flow:
# preflight -> matrix run -> evidence bundle -> markdown report scaffold.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

usage() {
    cat <<'EOF'
Usage:
  source config/onair-stations.sh
  ./scripts/run-onair-validation-flow.sh [--quick|--full] [--label NAME] [--notes FILE]
      [--output DIR] [--no-preflight] [--help]

Options:
  --quick         Run quick-tier matrix (default).
  --full          Run full-tier matrix.
  --label NAME    Label used for evidence bundle naming.
  --notes FILE    Operator notes file included in evidence bundle.
  --output DIR    Output root (default: docs/dev/test-reports).
  --no-preflight  Skip preflight in run-onair-tests (advanced use only).
  --help          Show this help text.

Outputs:
  - onair JSON:           <output>/onair-<timestamp>.json
  - evidence bundle:      <output>/on-air/bundle-<utc>-<label>/
  - markdown report:      <output>/on-air/phase-5.5-reg-<timestamp>.md
EOF
}

TIER="--quick"
LABEL=""
NOTES_FILE=""
OUTPUT_DIR="docs/dev/test-reports"
RUN_PREFLIGHT=1

while [[ $# -gt 0 ]]; do
    case "$1" in
        --quick)
            TIER="--quick"
            ;;
        --full)
            TIER="--full"
            ;;
        --label)
            LABEL="$2"
            shift
            ;;
        --notes)
            NOTES_FILE="$2"
            shift
            ;;
        --output)
            OUTPUT_DIR="$2"
            shift
            ;;
        --no-preflight)
            RUN_PREFLIGHT=0
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            usage >&2
            exit 1
            ;;
    esac
    shift
done

mkdir -p "$OUTPUT_DIR" "$OUTPUT_DIR/on-air"

RUN_ARGS=("$TIER" "--output" "$OUTPUT_DIR")
if [[ $RUN_PREFLIGHT -eq 0 ]]; then
    RUN_ARGS+=("--no-preflight")
fi

echo "==> Step 1/3: run on-air matrix (${TIER#--})"
./scripts/run-onair-tests.sh "${RUN_ARGS[@]}"

REPORT_FILE="$(ls -t "$OUTPUT_DIR"/onair-*.json 2>/dev/null | head -1 || true)"
if [[ -z "$REPORT_FILE" || ! -f "$REPORT_FILE" ]]; then
    echo "Unable to locate generated on-air report JSON in $OUTPUT_DIR" >&2
    exit 1
fi

echo "==> Step 2/3: build evidence bundle"
BUNDLE_ARGS=("--report" "$REPORT_FILE" "--output" "$OUTPUT_DIR/on-air" "--require-report" "--require-config" "--require-preflight")
if [[ -n "$LABEL" ]]; then
    BUNDLE_ARGS+=("--label" "$LABEL")
fi
if [[ -n "$NOTES_FILE" ]]; then
    BUNDLE_ARGS+=("--notes" "$NOTES_FILE")
fi
./scripts/onair-bundle-evidence.sh "${BUNDLE_ARGS[@]}"

METADATA_FILE="$(ls -t "$OUTPUT_DIR"/on-air/bundle-*/metadata.json 2>/dev/null | head -1 || true)"
if [[ -z "$METADATA_FILE" || ! -f "$METADATA_FILE" ]]; then
    echo "Unable to locate evidence metadata.json under $OUTPUT_DIR/on-air" >&2
    exit 1
fi

echo "==> Step 3/3: generate Phase 5.5-reg markdown report"
./scripts/onair-generate-report.sh --report "$REPORT_FILE" --metadata "$METADATA_FILE" --output "$OUTPUT_DIR/on-air/phase-5.5-reg-$(date -u +%Y-%m-%dT%H%M%SZ).md"

LATEST_MD="$(ls -t "$OUTPUT_DIR"/on-air/phase-5.5-reg-*.md 2>/dev/null | head -1 || true)"
echo
echo "Flow complete."
echo "- JSON report:      $REPORT_FILE"
echo "- Evidence metadata:$METADATA_FILE"
if [[ -n "$LATEST_MD" ]]; then
    echo "- Markdown report:  $LATEST_MD"
fi
