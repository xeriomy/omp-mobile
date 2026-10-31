#!/bin/sh
# Executable checks for the release installer. Uses a local file:// release
# directory so no network access is required.
set -eu

ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/install-bridge.sh"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/omp-bridge-installer-test.XXXXXX")"
cleanup() {
  rm -rf "$TMP"
}
trap cleanup EXIT HUP INT TERM

fail() {
  printf 'installer test failed: %s\n' "$1" >&2
  exit 1
}

bash -n "$SCRIPT"

TARGET="$("$SCRIPT" --print-target)"
case "$TARGET" in
  aarch64-unknown-linux-gnu | x86_64-unknown-linux-gnu) ;;
  *) fail "unexpected target: $TARGET" ;;
esac

RELEASE="$TMP/release/v0.1.0"
mkdir -p "$RELEASE" "$TMP/bin"
ASSET="omp-bridge-$TARGET"
cat >"$RELEASE/$ASSET" <<'EOF'
#!/bin/sh
if [ "${1:-}" = "--version" ]; then
  printf 'omp-bridge 0.1.0\n'
  exit 0
fi
printf 'fake bridge\n'
EOF
chmod +x "$RELEASE/$ASSET"
(
  cd "$RELEASE"
  sha256sum "$ASSET" >"$ASSET.sha256"
)

OMP_BRIDGE_ALLOW_ROOT=1 \
  OMP_BRIDGE_RELEASE_BASE_URL="file://$TMP/release" \
  "$SCRIPT" --version v0.1.0 --dir "$TMP/bin" >/dev/null
[ -x "$TMP/bin/omp-bridge" ] || fail "binary was not installed"
"$TMP/bin/omp-bridge" --version | grep -q '0.1.0' || fail "installed binary is wrong"

if [ "$(id -u)" = "0" ]; then
  if OMP_BRIDGE_RELEASE_BASE_URL="file://$TMP/release" \
    "$SCRIPT" --version v0.1.0 --dir "$TMP/root-bin" >/dev/null 2>&1; then
    fail "installer allowed root without OMP_BRIDGE_ALLOW_ROOT"
  fi
fi

printf 'corrupted\n' >>"$RELEASE/$ASSET"
if OMP_BRIDGE_ALLOW_ROOT=1 \
  OMP_BRIDGE_RELEASE_BASE_URL="file://$TMP/release" \
  "$SCRIPT" --version v0.1.0 --dir "$TMP/bin" >/dev/null 2>&1; then
  fail "installer accepted a bad checksum"
fi
"$TMP/bin/omp-bridge" --version | grep -q '0.1.0' || fail "bad checksum replaced the binary"
(
  cd "$RELEASE"
  sha256sum "$ASSET" >"$ASSET.sha256"
)

cat >"$TMP/bin/omp-bridge" <<'EOF'
#!/bin/sh
if [ "${1:-}" = "status" ]; then
  printf 'OMP Bridge status: engine ready\n'
  exit 0
fi
printf 'omp-bridge 0.1.0\n'
EOF
chmod +x "$TMP/bin/omp-bridge"
if OMP_BRIDGE_ALLOW_ROOT=1 \
  OMP_BRIDGE_RELEASE_BASE_URL="file://$TMP/release" \
  "$SCRIPT" --version v0.1.0 --dir "$TMP/bin" >/dev/null 2>&1; then
  fail "installer replaced a running bridge"
fi

printf 'installer tests passed (%s)\n' "$TARGET"
