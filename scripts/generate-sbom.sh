#!/usr/bin/env bash
# Regenerate SBOM.spdx.json (SPDX 2.3) for the workspace.
#
# `cargo sbom` stamps a fresh `created` timestamp and a random `documentNamespace` UUID on every run,
# so its raw output differs even when nothing about the dependency graph changed. Both are normalised
# here so a diff in the committed SBOM means the DEPENDENCIES actually changed — which is the only
# thing the artifact is for, and what makes a staleness check possible at all.
#
#   created           → SOURCE_DATE_EPOCH (or today) at 00:00:00Z
#   documentNamespace → derived from the workspace version, so one version == one document
#
# Usage: scripts/generate-sbom.sh [--check]
#   --check  regenerate into a temp file and fail if it differs from the committed SBOM
set -euo pipefail

cd "$(dirname "$0")/.."

for tool in cargo jq; do
  command -v "$tool" >/dev/null || { echo "error: $tool not found" >&2; exit 1; }
done
cargo sbom --version >/dev/null 2>&1 || {
  echo "error: cargo-sbom not installed — cargo install cargo-sbom" >&2
  exit 1
}

version="$(awk '
  /^\[workspace\.package\]/ { in_section=1; next }
  /^\[/ { in_section=0 }
  in_section && $1 == "version" && $2 == "=" { gsub(/"/, "", $3); print $3; exit }
' Cargo.toml)"
[[ -n "$version" ]] || { echo "error: could not read workspace version" >&2; exit 1; }

created="$(date -u -d "@${SOURCE_DATE_EPOCH:-$(date +%s)}" +%Y-%m-%dT00:00:00Z)"
namespace="https://spdx.org/spdxdocs/OpenPulseHF-${version}"

out="$(mktemp)"
trap 'rm -f "$out"' EXIT
cargo sbom --output-format spdx_json_2_3 \
  | jq --arg created "$created" --arg ns "$namespace" '
      .creationInfo.created = $created
      | .documentNamespace = $ns
      # cargo-sbom emits packages/relationships in hash order, which varies run to run.
      | .packages |= sort_by(.SPDXID)
      | .relationships |= sort_by(.spdxElementId, .relationshipType, .relatedSpdxElement)
    ' > "$out"

if [[ "${1:-}" == "--check" ]]; then
  if diff -q "$out" SBOM.spdx.json >/dev/null 2>&1; then
    echo "SBOM.spdx.json is up to date (workspace v${version})."
  else
    echo "error: SBOM.spdx.json is stale — run scripts/generate-sbom.sh" >&2
    diff <(jq -S '.packages' SBOM.spdx.json 2>/dev/null || echo '[]') \
         <(jq -S '.packages' "$out") | head -40 >&2 || true
    exit 1
  fi
else
  mv "$out" SBOM.spdx.json
  trap - EXIT
  echo "Wrote SBOM.spdx.json — workspace v${version}, $(jq '.packages | length' SBOM.spdx.json) packages."
fi
