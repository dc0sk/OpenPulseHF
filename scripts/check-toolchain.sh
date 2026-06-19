#!/usr/bin/env bash
set -euo pipefail

REQUIRED_MIN="${1:-1.94.0}"

version_ge() {
  local lhs="$1"
  local rhs="$2"
  local lhs_parts rhs_parts i

  IFS='.' read -r -a lhs_parts <<<"$lhs"
  IFS='.' read -r -a rhs_parts <<<"$rhs"

  # Compare dotted numeric versions component-wise, padding with zeros.
  for ((i = 0; i < 3; i++)); do
    local l="${lhs_parts[i]:-0}"
    local r="${rhs_parts[i]:-0}"
    if ((l > r)); then
      return 0
    fi
    if ((l < r)); then
      return 1
    fi
  done

  return 0
}

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

if ! version_ge "$RUSTC_VER" "$REQUIRED_MIN"; then
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
