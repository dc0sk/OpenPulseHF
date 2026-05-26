#!/usr/bin/env bash
set -euo pipefail

REQUIRED_MIN="${1:-1.94.0}"

if ! command -v rustc >/dev/null 2>&1; then
  echo "error: rustc is not installed or not in PATH" >&2
  exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo is not installed or not in PATH" >&2
  exit 1
fi

RUSTC_VER="$(rustc --version | awk '{print $2}')"
CARGO_VER="$(cargo --version | awk '{print $2}')"

printf 'Detected toolchain: rustc=%s cargo=%s\n' "$RUSTC_VER" "$CARGO_VER"

if [[ "$(printf '%s\n%s\n' "$REQUIRED_MIN" "$RUSTC_VER" | sort -V | head -n1)" != "$REQUIRED_MIN" ]]; then
  cat >&2 <<EOF
error: rustc $RUSTC_VER is below required minimum $REQUIRED_MIN.
This workspace currently depends on crates that require newer rustc (for example sqlx 0.9 in pki-tooling).

Resolution options:
  1) Install/use Rust $REQUIRED_MIN or newer (recommended)
  2) If temporarily constrained, run core fallback gates excluding pki-tooling:
     cargo clippy --workspace --exclude pki-tooling --no-default-features -- -D warnings
     cargo test --workspace --exclude pki-tooling --no-default-features
EOF
  exit 1
fi

echo "Toolchain check passed (>= $REQUIRED_MIN)."
