#!/bin/sh
# Install the OMP bridge from GitHub Releases.
#
# This script never uses sudo and never writes outside the chosen user-writable
# install directory. It verifies the published SHA-256 before replacing an
# existing binary.
set -eu

REPOSITORY="xeriomy/omp-mobile"
DEFAULT_BASE_URL="https://github.com/${REPOSITORY}/releases/download"
BASE_URL="${OMP_BRIDGE_RELEASE_BASE_URL:-$DEFAULT_BASE_URL}"
VERSION="${OMP_BRIDGE_VERSION:-}"
INSTALL_DIR="${OMP_BRIDGE_INSTALL_DIR:-$HOME/.local/bin}"
PRINT_TARGET=0

usage() {
  cat <<'EOF'
Usage: install-bridge.sh [--version <tag>] [--dir <path>] [--print-target]

Environment:
  OMP_BRIDGE_VERSION       Release tag, for example v0.1.0 (default: latest)
  OMP_BRIDGE_INSTALL_DIR   Install directory (default: ~/.local/bin)
  OMP_BRIDGE_ALLOW_ROOT=1  Allow running as root (containers and tests only)
EOF
}

die() {
  printf 'install-bridge: %s\n' "$1" >&2
  exit 1
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version)
      [ "$#" -ge 2 ] || die "--version needs a value"
      VERSION="$2"
      shift 2
      ;;
    --dir)
      [ "$#" -ge 2 ] || die "--dir needs a value"
      INSTALL_DIR="$2"
      shift 2
      ;;
    --print-target)
      PRINT_TARGET=1
      shift
      ;;
    --help | -h)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

detect_target() {
  [ "$(uname -s)" = "Linux" ] || die "only Linux is supported"
  if [ -n "${PREFIX:-}" ] && printf '%s' "$PREFIX" | grep -q 'com.termux'; then
    die "native Termux is not a supported release target yet; use Ubuntu Proot"
  fi
  if ! (getconf GNU_LIBC_VERSION >/dev/null 2>&1 || ls /lib/*/libc.so.6 >/dev/null 2>&1); then
    die "a glibc-based Linux is required (musl and bionic are not supported)"
  fi
  case "$(uname -m)" in
    aarch64 | arm64) printf '%s\n' "aarch64-unknown-linux-gnu" ;;
    x86_64 | amd64) printf '%s\n' "x86_64-unknown-linux-gnu" ;;
    *) die "unsupported architecture: $(uname -m)" ;;
  esac
}

TARGET="$(detect_target)"
if [ "$PRINT_TARGET" = "1" ]; then
  printf '%s\n' "$TARGET"
  exit 0
fi

if [ "$(id -u)" = "0" ] && [ "${OMP_BRIDGE_ALLOW_ROOT:-0}" != "1" ]; then
  die "refusing to install as root; run as the user who will run omp-bridge"
fi

for command in curl sha256sum install mktemp; do
  command -v "$command" >/dev/null 2>&1 || die "missing required command: $command"
done

if [ -z "$VERSION" ]; then
  command -v sed >/dev/null 2>&1 || die "missing required command: sed"
  VERSION="$(curl -fsSL "https://api.github.com/repos/${REPOSITORY}/releases/latest" |
    sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  [ -n "$VERSION" ] || die "cannot determine the latest release; pass --version"
fi
case "$VERSION" in
  v*) ;;
  *) VERSION="v$VERSION" ;;
esac

ASSET="omp-bridge-${TARGET}"
CHECKSUM="${ASSET}.sha256"
DOWNLOAD_URL="${BASE_URL}/${VERSION}"
TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/omp-bridge-install.XXXXXX")"
NEW_BINARY=""
cleanup() {
  rm -rf "$TMP_DIR"
  [ -z "$NEW_BINARY" ] || rm -f "$NEW_BINARY"
}
trap cleanup EXIT HUP INT TERM

printf 'Installing OMP Bridge %s for %s\n' "$VERSION" "$TARGET"
printf '  From: %s\n' "$DOWNLOAD_URL/$ASSET"
printf '  To:   %s\n' "$INSTALL_DIR/omp-bridge"

curl -fsSL "$DOWNLOAD_URL/$ASSET" -o "$TMP_DIR/$ASSET" ||
  die "cannot download $ASSET"
curl -fsSL "$DOWNLOAD_URL/$CHECKSUM" -o "$TMP_DIR/$CHECKSUM" ||
  die "cannot download $CHECKSUM"

(
  cd "$TMP_DIR"
  sha256sum -c "$CHECKSUM"
) || die "SHA-256 verification failed; refusing to install"

DESTINATION="$INSTALL_DIR/omp-bridge"
if [ -x "$DESTINATION" ]; then
  STATUS_OUTPUT="$("$DESTINATION" status 2>/dev/null || true)"
  if printf '%s\n' "$STATUS_OUTPUT" |
    grep -Eq 'status: (starting|running|engine ready|bridge unhealthy)'; then
    die "the bridge is running; stop it with 'omp-bridge stop' before upgrading"
  fi
fi

mkdir -p "$INSTALL_DIR"
[ -w "$INSTALL_DIR" ] || die "install directory is not writable: $INSTALL_DIR"
NEW_BINARY="$INSTALL_DIR/.omp-bridge.new.$$"
install -m 0755 "$TMP_DIR/$ASSET" "$NEW_BINARY"
mv -f "$NEW_BINARY" "$DESTINATION"
NEW_BINARY=""

printf '\nInstalled: %s\n' "$DESTINATION"
"$DESTINATION" --version
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    printf '%s\n' "" "Add this directory to PATH:" "  export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac
printf '\nNext:\n  omp-bridge setup\n  omp-bridge start\n'
