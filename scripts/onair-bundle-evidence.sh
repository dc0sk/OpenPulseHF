#!/usr/bin/env bash
# Build a structured evidence bundle for Phase 5.5-reg on-air validation.
#
# Usage:
#   ./scripts/onair-bundle-evidence.sh [--report FILE] [--notes FILE] [--output DIR] [--label NAME]
#
# Defaults:
#   report: latest docs/test-reports/onair-*.json (if present)
#   notes:  none
#   output: docs/test-reports/on-air

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

REPORT_FILE=""
NOTES_FILE=""
OUTPUT_DIR="docs/test-reports/on-air"
LABEL=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --report)
            REPORT_FILE="$2"
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
        --label)
            LABEL="$2"
            shift
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 1
            ;;
    esac
    shift
done

if [[ -z "$REPORT_FILE" ]]; then
    REPORT_FILE="$(ls -t docs/test-reports/onair-*.json 2>/dev/null | head -1 || true)"
fi

TS="$(date -u +%Y-%m-%dT%H%M%SZ)"
SAFE_LABEL=""
if [[ -n "$LABEL" ]]; then
    SAFE_LABEL="-$(printf '%s' "$LABEL" | tr -c 'A-Za-z0-9._-' '_')"
fi

BUNDLE_DIR="${OUTPUT_DIR}/bundle-${TS}${SAFE_LABEL}"
mkdir -p "$BUNDLE_DIR"

CONFIG_FILE="${OPENPULSE_CONFIG_FILE:-$HOME/.config/openpulse/config.toml}"
GIT_SHA="$(git rev-parse HEAD 2>/dev/null || echo unknown)"
GIT_BRANCH="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo unknown)"

REPORT_BASENAME=""
if [[ -n "$REPORT_FILE" ]]; then
    if [[ ! -f "$REPORT_FILE" ]]; then
        echo "Report file not found: $REPORT_FILE" >&2
        exit 1
    fi
    REPORT_BASENAME="$(basename "$REPORT_FILE")"
    cp "$REPORT_FILE" "$BUNDLE_DIR/$REPORT_BASENAME"
fi

NOTES_BASENAME=""
if [[ -n "$NOTES_FILE" ]]; then
    if [[ ! -f "$NOTES_FILE" ]]; then
        echo "Notes file not found: $NOTES_FILE" >&2
        exit 1
    fi
    NOTES_BASENAME="$(basename "$NOTES_FILE")"
    cp "$NOTES_FILE" "$BUNDLE_DIR/$NOTES_BASENAME"
fi

CONFIG_BASENAME=""
if [[ -f "$CONFIG_FILE" ]]; then
    CONFIG_BASENAME="config.toml.snapshot"
    cp "$CONFIG_FILE" "$BUNDLE_DIR/$CONFIG_BASENAME"
fi

HASH_CMD=""
if command -v sha256sum >/dev/null 2>&1; then
    HASH_CMD="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
    HASH_CMD="shasum -a 256"
fi

REPORT_SHA256=""
CONFIG_SHA256=""
NOTES_SHA256=""

if [[ -n "$HASH_CMD" ]]; then
    if [[ -n "$REPORT_BASENAME" ]]; then
        REPORT_SHA256="$($HASH_CMD "$BUNDLE_DIR/$REPORT_BASENAME" | awk '{print $1}')"
    fi
    if [[ -n "$CONFIG_BASENAME" ]]; then
        CONFIG_SHA256="$($HASH_CMD "$BUNDLE_DIR/$CONFIG_BASENAME" | awk '{print $1}')"
    fi
    if [[ -n "$NOTES_BASENAME" ]]; then
        NOTES_SHA256="$($HASH_CMD "$BUNDLE_DIR/$NOTES_BASENAME" | awk '{print $1}')"
    fi
fi

export TS GIT_SHA GIT_BRANCH CONFIG_FILE BUNDLE_DIR REPORT_FILE REPORT_BASENAME REPORT_SHA256
export CONFIG_BASENAME CONFIG_SHA256 NOTES_BASENAME NOTES_SHA256 LABEL

python3 - <<'PY'
import json
import os
import socket

bundle_dir = os.environ["BUNDLE_DIR"]
report_path = os.path.join(bundle_dir, os.environ["REPORT_BASENAME"]) if os.environ["REPORT_BASENAME"] else None
preflight = None

if report_path and os.path.exists(report_path):
    try:
        with open(report_path, "r", encoding="utf-8") as f:
            report = json.load(f)
        preflight = report.get("preflight")
    except Exception:
        preflight = None

meta = {
    "captured_at_utc": os.environ["TS"],
    "git_sha": os.environ["GIT_SHA"],
    "git_branch": os.environ["GIT_BRANCH"],
    "host": socket.gethostname(),
    "operator_user": os.environ.get("USER", "unknown"),
    "label": os.environ.get("LABEL", ""),
    "files": {
        "report": {
            "source": os.environ.get("REPORT_FILE", ""),
            "bundle_name": os.environ.get("REPORT_BASENAME", ""),
            "sha256": os.environ.get("REPORT_SHA256", ""),
        },
        "config_snapshot": {
            "source": os.environ.get("CONFIG_FILE", ""),
            "bundle_name": os.environ.get("CONFIG_BASENAME", ""),
            "sha256": os.environ.get("CONFIG_SHA256", ""),
        },
        "notes": {
            "bundle_name": os.environ.get("NOTES_BASENAME", ""),
            "sha256": os.environ.get("NOTES_SHA256", ""),
        },
    },
    "preflight": preflight,
}

with open(os.path.join(bundle_dir, "metadata.json"), "w", encoding="utf-8") as f:
    json.dump(meta, f, indent=2)
    f.write("\n")
PY

echo "Evidence bundle created: $BUNDLE_DIR"
echo "- metadata: $BUNDLE_DIR/metadata.json"
if [[ -n "$REPORT_BASENAME" ]]; then
    echo "- report: $BUNDLE_DIR/$REPORT_BASENAME"
fi
if [[ -n "$CONFIG_BASENAME" ]]; then
    echo "- config snapshot: $BUNDLE_DIR/$CONFIG_BASENAME"
fi
if [[ -n "$NOTES_BASENAME" ]]; then
    echo "- notes: $BUNDLE_DIR/$NOTES_BASENAME"
fi
