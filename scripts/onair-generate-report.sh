#!/usr/bin/env bash
# Generate a standardized Phase 5.5-reg on-air validation markdown report from
# run-onair-tests JSON output and optional evidence-bundle metadata.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

usage() {
    cat <<'EOF'
Usage:
  ./scripts/onair-generate-report.sh [--report FILE] [--metadata FILE] [--output FILE]

Options:
  --report FILE    Input on-air JSON report (default: latest docs/dev/test-reports/onair-*.json,
                   fallback: latest docs/test-reports/onair-*.json)
  --metadata FILE  Optional evidence metadata.json from onair-bundle-evidence.sh
  --output FILE    Output markdown path
                   (default: docs/dev/test-reports/on-air/phase-5.5-reg-<timestamp>.md)
  --help           Show this help text.
EOF
}

REPORT_FILE=""
METADATA_FILE=""
OUTPUT_FILE=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --report)
            REPORT_FILE="$2"
            shift
            ;;
        --metadata)
            METADATA_FILE="$2"
            shift
            ;;
        --output)
            OUTPUT_FILE="$2"
            shift
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

if [[ -z "$REPORT_FILE" ]]; then
    REPORT_FILE="$(ls -t docs/dev/test-reports/onair-*.json 2>/dev/null | head -1 || true)"
    if [[ -z "$REPORT_FILE" ]]; then
        REPORT_FILE="$(ls -t docs/test-reports/onair-*.json 2>/dev/null | head -1 || true)"
    fi
fi

if [[ -z "$REPORT_FILE" || ! -f "$REPORT_FILE" ]]; then
    echo "On-air report JSON not found. Pass --report FILE or run scripts/run-onair-tests.sh first." >&2
    exit 1
fi

if [[ -z "$OUTPUT_FILE" ]]; then
    TS="$(date -u +%Y-%m-%dT%H%M%SZ)"
    OUTPUT_FILE="docs/dev/test-reports/on-air/phase-5.5-reg-${TS}.md"
fi

mkdir -p "$(dirname "$OUTPUT_FILE")"

export REPORT_FILE METADATA_FILE OUTPUT_FILE
python3 - <<'PY'
import json
import os
from pathlib import Path

report_path = Path(os.environ["REPORT_FILE"])
metadata_env = os.environ.get("METADATA_FILE", "")
metadata_path = Path(metadata_env) if metadata_env else None
output_path = Path(os.environ["OUTPUT_FILE"])

with report_path.open("r", encoding="utf-8") as f:
    report = json.load(f)

meta = {}
if metadata_path and metadata_path.exists():
    try:
        with metadata_path.open("r", encoding="utf-8") as f:
            meta = json.load(f)
    except Exception:
        meta = {}

ts = report.get("timestamp", "unknown")
tier = report.get("tier", "unknown")
git_sha = report.get("git_sha", "unknown")
station_a = report.get("station_a", "unknown")
station_b = report.get("station_b", "unknown")
callsign_a = report.get("callsign_a", "unknown")
callsign_b = report.get("callsign_b", "unknown")
ptt_backend = report.get("ptt_backend", "unknown")
total = int(report.get("total", 0) or 0)
passed = int(report.get("pass", 0) or 0)
failed = int(report.get("fail", 0) or 0)
pass_rate = (100.0 * passed / total) if total else 0.0

preflight = report.get("preflight") or {}
preflight_ran = preflight.get("ran", False)
preflight_mode = preflight.get("mode", "unknown")

cases = report.get("cases") or []
failed_cases = [c for c in cases if c.get("result") != "pass"]

bundle_captured = meta.get("captured_at_utc", "")
bundle_git_sha = meta.get("git_sha", "")
bundle_git_branch = meta.get("git_branch", "")
bundle_dirty = meta.get("git_dirty", "")

lines = []
lines.append("# Phase 5.5-reg On-Air Validation Report")
lines.append("")
lines.append("## Summary")
lines.append("")
lines.append(f"- Run timestamp (UTC): {ts}")
lines.append(f"- Tier: {tier}")
lines.append(f"- Git SHA (report): {git_sha}")
lines.append(f"- Total cases: {total}")
lines.append(f"- Passed: {passed}")
lines.append(f"- Failed: {failed}")
lines.append(f"- Pass rate: {pass_rate:.1f}%")
lines.append("")
lines.append("## Station Configuration")
lines.append("")
lines.append(f"- Station A: {station_a} ({callsign_a})")
lines.append(f"- Station B: {station_b} ({callsign_b})")
lines.append(f"- PTT backend: {ptt_backend}")
lines.append("")
lines.append("## Preflight")
lines.append("")
lines.append(f"- Ran: {preflight_ran}")
lines.append(f"- Mode: {preflight_mode}")
lines.append("")

if meta:
    lines.append("## Evidence Bundle Metadata")
    lines.append("")
    lines.append(f"- Captured at (UTC): {bundle_captured}")
    lines.append(f"- Git SHA (bundle): {bundle_git_sha}")
    lines.append(f"- Git branch (bundle): {bundle_git_branch}")
    lines.append(f"- Git dirty: {bundle_dirty}")
    lines.append("")

lines.append("## Failed Cases")
lines.append("")
if not failed_cases:
    lines.append("No failed cases were reported.")
else:
    for c in failed_cases:
        mode = c.get("mode", "unknown")
        fec = c.get("fec", "unknown")
        payload = c.get("payload_bytes", "?")
        reason = c.get("fail_reason", "")
        iss_exit = c.get("iss_exit", "")
        lines.append(f"- mode={mode}, fec={fec}, payload={payload}, iss_exit={iss_exit}, reason={reason}")

lines.append("")
lines.append("## Artifacts")
lines.append("")
lines.append(f"- Report JSON: {report_path.as_posix()}")
if metadata_path and metadata_path.exists():
    lines.append(f"- Evidence metadata: {metadata_path.as_posix()}")
lines.append("")
lines.append("## Compliance Notes")
lines.append("")
lines.append("- Confirm station-ID cadence and frequency-plan compliance were reviewed for this run.")
lines.append("- Add operator notes and incident timeline before final Phase 5.5-reg sign-off.")

output_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
print(output_path.as_posix())
PY

echo "On-air validation report written to: $OUTPUT_FILE"
