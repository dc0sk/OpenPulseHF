#!/usr/bin/env bash
set -euo pipefail

# Install OpenPulseHF build dependencies on Raspberry Pi OS / Debian.
# Optional runtime extras (rigctld) can be included with --with-hamlib.

MIN_RUST="1.94.0"
WITH_HAMLIB=0

usage() {
  cat <<'EOF'
Usage: scripts/install-rpi-build-deps.sh [--with-hamlib]

Installs missing system packages required to build OpenPulseHF on Raspberry Pi OS.

Options:
  --with-hamlib   Also install hamlib utilities (rigctld/rigctl) for on-air scripts
  -h, --help      Show this help
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --with-hamlib)
      WITH_HAMLIB=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if ! command -v apt-get >/dev/null 2>&1; then
  echo "error: apt-get not found. This script supports Debian/Raspberry Pi OS." >&2
  exit 1
fi

if [[ -r /etc/os-release ]]; then
  . /etc/os-release
  echo "Detected OS: ${PRETTY_NAME:-unknown}"
fi

if [[ $(id -u) -eq 0 ]]; then
  SUDO=()
else
  if ! command -v sudo >/dev/null 2>&1; then
    echo "error: sudo is required when not running as root." >&2
    exit 1
  fi
  SUDO=(sudo)
fi

have_pkg() {
  dpkg-query -W -f='${Status}\n' "$1" 2>/dev/null | grep -q "install ok installed"
}

collect_missing() {
  local pkg
  local -a missing=()
  for pkg in "$@"; do
    if ! have_pkg "$pkg"; then
      missing+=("$pkg")
    fi
  done
  printf '%s\n' "${missing[@]:-}"
}

version_ge() {
  local a="$1"
  local b="$2"
  [[ "$(printf '%s\n%s\n' "$a" "$b" | sort -V | tail -n1)" == "$a" ]]
}

BASE_PACKAGES=(
  build-essential
  pkg-config
  curl
  ca-certificates
  git
)

OPENPULSE_BUILD_PACKAGES=(
  libasound2-dev
  libudev-dev
  libssl-dev
  clang
  cmake
  protobuf-compiler
)

OPENPULSE_RUNTIME_PACKAGES=(
  alsa-utils
  rsync
)

HAMLIB_PACKAGES=()

resolve_hamlib_package() {
  local candidate
  for candidate in hamlib-utils libhamlib-utils; do
    if apt-cache show "$candidate" >/dev/null 2>&1; then
      echo "$candidate"
      return 0
    fi
  done
  return 1
}

echo "Checking apt packages..."
mapfile -t missing_base < <(collect_missing "${BASE_PACKAGES[@]}")
mapfile -t missing_build < <(collect_missing "${OPENPULSE_BUILD_PACKAGES[@]}")
mapfile -t missing_runtime < <(collect_missing "${OPENPULSE_RUNTIME_PACKAGES[@]}")

missing_all=("${missing_base[@]}" "${missing_build[@]}" "${missing_runtime[@]}")

if [[ $WITH_HAMLIB -eq 1 ]]; then
  if hamlib_pkg="$(resolve_hamlib_package)"; then
    HAMLIB_PACKAGES=("$hamlib_pkg")
  else
    echo "warning: no hamlib utility package found in apt repositories; skipping hamlib install" >&2
    HAMLIB_PACKAGES=()
  fi
  mapfile -t missing_hamlib < <(collect_missing "${HAMLIB_PACKAGES[@]}")
  missing_all+=("${missing_hamlib[@]}")
fi

# Remove empty entries and duplicates.
declare -A seen=()
filtered_missing=()
for pkg in "${missing_all[@]}"; do
  [[ -n "$pkg" ]] || continue
  if [[ -z "${seen[$pkg]:-}" ]]; then
    seen[$pkg]=1
    filtered_missing+=("$pkg")
  fi
done

if [[ ${#filtered_missing[@]} -gt 0 ]]; then
  echo "Installing missing apt packages: ${filtered_missing[*]}"
  "${SUDO[@]}" apt-get update
  "${SUDO[@]}" apt-get install -y "${filtered_missing[@]}"
else
  echo "All required apt packages are already installed."
fi

if ! command -v rustc >/dev/null 2>&1 || ! command -v cargo >/dev/null 2>&1; then
  echo "Rust toolchain not found; installing rustup + stable toolchain..."
  if ! command -v rustup >/dev/null 2>&1; then
    curl https://sh.rustup.rs -sSf | sh -s -- -y
  fi
fi

if [[ -f "$HOME/.cargo/env" ]]; then
  # shellcheck disable=SC1090
  . "$HOME/.cargo/env"
fi

if ! command -v rustup >/dev/null 2>&1; then
  echo "error: rustup is not available after install attempt." >&2
  exit 1
fi

rustup toolchain install stable
rustup default stable
rustup component add rustfmt clippy

if ! command -v rustc >/dev/null 2>&1 || ! command -v cargo >/dev/null 2>&1; then
  echo "error: rustc/cargo are still not in PATH; open a new shell and re-run." >&2
  exit 1
fi

RUST_VER="$(rustc --version | awk '{print $2}')"
if ! version_ge "$RUST_VER" "$MIN_RUST"; then
  echo "error: rustc $RUST_VER is below required minimum $MIN_RUST" >&2
  exit 1
fi

echo "Rust toolchain: rustc $(rustc --version | awk '{print $2}'), cargo $(cargo --version | awk '{print $2}')"
echo "Done. You can now run:"
echo "  ./scripts/check-toolchain.sh"
echo "  cargo build --workspace"
echo "  cargo test --workspace --no-default-features"
