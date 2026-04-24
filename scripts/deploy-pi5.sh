#!/usr/bin/env bash
# Cross-compile OpenPulseHF for Raspberry Pi 5 (aarch64) and deploy via rsync.
#
# Usage:
#   ./scripts/deploy-pi5.sh [user@host]
#
# Defaults to dc0sk@192.168.121.49 if no argument is given.
# Requires: aarch64-linux-gnu-gcc, rustup target aarch64-unknown-linux-gnu

set -euo pipefail

TARGET="aarch64-unknown-linux-gnu"
REMOTE="${1:-dc0sk@192.168.121.49}"
REMOTE_DIR="~/bin"

echo "==> Cross-compiling for ${TARGET} (no audio backend)..."
cargo build --release --target "${TARGET}" --no-default-features -p openpulse-cli

BINARY="target/${TARGET}/release/openpulse"

if [[ ! -f "${BINARY}" ]]; then
  echo "ERROR: expected binary not found at ${BINARY}" >&2
  exit 1
fi

echo "==> Deploying to ${REMOTE}:${REMOTE_DIR}..."
ssh "${REMOTE}" "mkdir -p ${REMOTE_DIR}"
rsync -avz "${BINARY}" "${REMOTE}:${REMOTE_DIR}/openpulse"

echo "==> Done. Run on Pi 5 with: ssh ${REMOTE} '~/bin/openpulse --help'"
