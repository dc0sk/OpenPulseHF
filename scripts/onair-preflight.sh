#!/usr/bin/env bash
# Local preflight checks before executing on-air tests.
#
# Usage:
#   ./scripts/onair-preflight.sh [--strict]
#
# Strict mode fails if release binaries are missing. Non-strict mode reports
# warnings but still returns success when core tooling/config checks pass.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

usage() {
    cat <<'EOF'
Usage:
  ./scripts/onair-preflight.sh [--strict] [--help]

Options:
  --strict  Fail when expected release binaries are missing.
  --help    Show this help text.
EOF
}

STRICT=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --strict)
            STRICT=1
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

CFG_FILE="${OPENPULSE_CONFIG_FILE:-$HOME/.config/openpulse/config.toml}"

PASS=0
WARN=0
FAIL=0

ok() {
    echo "[PASS] $1"
    PASS=$((PASS + 1))
}

warn() {
    echo "[WARN] $1"
    WARN=$((WARN + 1))
}

fail() {
    echo "[FAIL] $1"
    FAIL=$((FAIL + 1))
}

check_cmd() {
    local cmd="$1"
    if command -v "$cmd" >/dev/null 2>&1; then
        ok "command available: $cmd"
    else
        fail "missing required command: $cmd"
    fi
}

check_binary() {
    local path="$1"
    local label="$2"
    if [[ -x "$path" ]]; then
        ok "$label present: $path"
    else
        if [[ $STRICT -eq 1 ]]; then
            fail "$label missing: $path"
        else
            warn "$label missing: $path (build with release + cpal features before on-air run)"
        fi
    fi
}

echo "== OpenPulseHF on-air preflight =="
echo "repo: $REPO_ROOT"
echo "config: $CFG_FILE"
echo "strict: $STRICT"
echo

check_cmd git
check_cmd cargo
check_cmd python3

if [[ -f "$CFG_FILE" ]]; then
    ok "config exists: $CFG_FILE"
    if grep -Eq '^callsign\s*=\s*"[A-Za-z0-9/\-]+"' "$CFG_FILE"; then
        CALLSIGN=$(grep -E '^callsign\s*=\s*"[A-Za-z0-9/\-]+"' "$CFG_FILE" | head -1 | sed -E 's/.*"(.*)".*/\1/')
        if [[ "$CALLSIGN" == "N0CALL" ]]; then
            fail "callsign is still N0CALL in $CFG_FILE"
        else
            ok "callsign configured: $CALLSIGN"
        fi
    else
        fail "callsign entry not found in $CFG_FILE"
    fi
else
    fail "config missing: $CFG_FILE"
fi

check_binary "$REPO_ROOT/target/release/openpulse" "openpulse CLI"
check_binary "$REPO_ROOT/target/release/openpulse-tnc" "openpulse-tnc"
check_binary "$REPO_ROOT/target/release/openpulse-kisstnc" "openpulse-kisstnc"
check_binary "$REPO_ROOT/target/release/openpulse-gateway" "openpulse-gateway"

if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    SHA=$(git rev-parse --short HEAD)
    ok "git sha: $SHA"
else
    warn "not running inside a git work tree"
fi

echo
echo "Summary: PASS=$PASS WARN=$WARN FAIL=$FAIL"

if [[ $FAIL -ne 0 ]]; then
    exit 1
fi

exit 0