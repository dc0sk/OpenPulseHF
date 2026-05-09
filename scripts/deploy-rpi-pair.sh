#!/usr/bin/env bash
# Cross-compile OpenPulseHF for a pair of aarch64 Raspberry Pis and deploy via rsync.
#
# Usage:
#   source config/onair-stations.sh   # optional: set STATION_A, STATION_B, SSH_OPTS
#   ./scripts/deploy-rpi-pair.sh
#
# Or override on the command line:
#   STATION_A=user@hostA STATION_B=user@hostB ./scripts/deploy-rpi-pair.sh
#
# Requires:
#   - Rust target aarch64-unknown-linux-gnu installed (rustup target add ...)
#   - A compatible C cross-linker, e.g. aarch64-linux-gnu-gcc on Debian/Ubuntu
#     or the `cross` wrapper if the linker is unavailable natively.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

TARGET="aarch64-unknown-linux-gnu"
STATION_A="${STATION_A:-dc0sk@192.168.1.10}"
STATION_B="${STATION_B:-dc0sk@192.168.1.11}"
SSH_OPTS="${SSH_OPTS:-}"
REMOTE_DIR="${REMOTE_DIR:-\$HOME/bin}"

echo "==> Cross-compiling for ${TARGET} (no audio backend)..."
cargo build --release --target "${TARGET}" --no-default-features \
    -p openpulse-cli \
    -p openpulse-ardop \
    -p openpulse-kiss

BIN_DIR="target/${TARGET}/release"
BINARIES=("openpulse" "openpulse-tnc" "openpulse-kisstnc")

for b in "${BINARIES[@]}"; do
    if [[ ! -f "${BIN_DIR}/${b}" ]]; then
        echo "ERROR: expected binary not found: ${BIN_DIR}/${b}" >&2
        exit 1
    fi
done

echo "==> Deploying to station A (${STATION_A})..."
# shellcheck disable=SC2086
ssh ${SSH_OPTS} "${STATION_A}" "mkdir -p ${REMOTE_DIR}"
# shellcheck disable=SC2086
rsync -avz -e "ssh ${SSH_OPTS}" \
    "${BIN_DIR}/openpulse" \
    "${BIN_DIR}/openpulse-tnc" \
    "${BIN_DIR}/openpulse-kisstnc" \
    "${STATION_A}:${REMOTE_DIR}/"

echo "==> Deploying to station B (${STATION_B})..."
# shellcheck disable=SC2086
ssh ${SSH_OPTS} "${STATION_B}" "mkdir -p ${REMOTE_DIR}"
# shellcheck disable=SC2086
rsync -avz -e "ssh ${SSH_OPTS}" \
    "${BIN_DIR}/openpulse" \
    "${BIN_DIR}/openpulse-tnc" \
    "${BIN_DIR}/openpulse-kisstnc" \
    "${STATION_B}:${REMOTE_DIR}/"

echo "==> Deploy complete."
echo "    Station A: ssh ${SSH_OPTS} ${STATION_A} '~/bin/openpulse --help'"
echo "    Station B: ssh ${SSH_OPTS} ${STATION_B} '~/bin/openpulse --help'"
