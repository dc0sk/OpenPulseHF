#!/usr/bin/env bash
# Cross-compile OpenPulseHF for a pair of aarch64 Raspberry Pis and deploy via rsync.
#
# ⚠️  THESE BINARIES CANNOT TRANSMIT OR RECEIVE OVER A RADIO.  ⚠️
#
# The cross build below is `--no-default-features`, so cpal (the real audio
# backend) is compiled OUT. A CLI/TNC built this way SILENTLY falls back to the
# in-process LoopbackBackend regardless of `--backend cpal` or a config file
# (see CLAUDE.md → "Audio backend opt-in"). The `openpulse` you deploy here will
# happily "transmit" into a null loop and key no audio — an on-air matrix run
# over these binaries produces zero RF and records every case as a failure it
# cannot explain.
#
# This is deliberate: cross-compiling cpal to aarch64 needs an arm64 `libasound`
# in the cross sysroot, which this toolchain does not carry. The RF runners
# (run-onair-ic9700-ft991a.sh, run-onair-tx500-kx3.sh) therefore build the cpal
# binaries ON THE PI over SSH — they do NOT use this script. Use this script only
# to stage a non-audio helper (e.g. a daemon control client) on a Pi, or set
# ALLOW_NO_AUDIO_DEPLOY=1 to acknowledge the above and proceed anyway.
#
# To get RF-capable binaries onto a Pi, build there:
#   ssh pi 'cd ~/git/OpenPulseHF && cargo build --release -p openpulse-cli --features cpal-backend'
#   ssh pi 'cargo build --release -p openpulse-kiss --features cpal'   # + ardop likewise
#
# Usage:
#   source config/onair-stations.sh   # optional: set STATION_A, STATION_B, SSH_OPTS
#   ALLOW_NO_AUDIO_DEPLOY=1 ./scripts/deploy-rpi-pair.sh
#
# Or override on the command line:
#   STATION_A=user@hostA STATION_B=user@hostB ALLOW_NO_AUDIO_DEPLOY=1 ./scripts/deploy-rpi-pair.sh
#
# Requires:
#   - Rust target aarch64-unknown-linux-gnu installed (rustup target add ...)
#   - A compatible C cross-linker, e.g. aarch64-linux-gnu-gcc on Debian/Ubuntu
#     or the `cross` wrapper if the linker is unavailable natively.

set -euo pipefail

if [[ "${ALLOW_NO_AUDIO_DEPLOY:-0}" != "1" ]]; then
    echo "REFUSING: this script cross-builds --no-default-features, so the deployed" >&2
    echo "  binaries have NO audio backend and cannot key a radio (they fall back to" >&2
    echo "  the loopback backend silently). An on-air run over them transmits nothing." >&2
    echo "" >&2
    echo "  For RF, build the cpal binaries ON the Pi (see the header of this script)." >&2
    echo "  To stage non-audio helpers anyway, re-run with ALLOW_NO_AUDIO_DEPLOY=1." >&2
    exit 1
fi

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
