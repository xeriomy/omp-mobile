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

clean_root_env() {
  env -u container -u PROOT_L2S_DIR -u PROOT_TMP_DIR -u PROOT_LOADER "$@"
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

clean_root_env \
  OMP_BRIDGE_ALLOW_ROOT=1 \
  OMP_BRIDGE_RELEASE_BASE_URL="file://$TMP/release" \
  "$SCRIPT" --version v0.1.0 --dir "$TMP/bin" >/dev/null
[ -x "$TMP/bin/omp-bridge" ] || fail "binary was not installed"
"$TMP/bin/omp-bridge" --version | grep -q '0.1.0' || fail "installed binary is wrong"

if [ "$(id -u)" = "0" ]; then
  if env -u container -u PROOT_L2S_DIR -u PROOT_TMP_DIR -u PROOT_LOADER \
    OMP_BRIDGE_RELEASE_BASE_URL="file://$TMP/release" \
    "$SCRIPT" --version v0.1.0 --dir "$TMP/root-bin" >/dev/null 2>&1; then
    fail "installer allowed ordinary root without OMP_BRIDGE_ALLOW_ROOT"
  fi

  PROOT_HOME="$TMP/proot-home"
  mkdir -p "$PROOT_HOME"
  if ! env container=proot-distro HOME="$PROOT_HOME" \
    OMP_BRIDGE_RELEASE_BASE_URL="file://$TMP/release" \
    "$SCRIPT" --version v0.1.0 --dir "$PROOT_HOME/.local/bin" >"$TMP/proot.out" 2>&1; then
    fail "installer refused root inside PRoot"
  fi
  grep -q 'PRoot' "$TMP/proot.out" || fail "PRoot install did not print a warning"
  [ -x "$PROOT_HOME/.local/bin/omp-bridge" ] || fail "PRoot install did not install the binary"

  if env container=proot-distro HOME="$PROOT_HOME" \
    OMP_BRIDGE_RELEASE_BASE_URL="file://$TMP/release" \
    "$SCRIPT" --version v0.1.0 --dir "$TMP/outside-home" >/dev/null 2>&1; then
    fail "PRoot root install escaped HOME"
  fi

  if ! env -u container -u PROOT_TMP_DIR -u PROOT_LOADER \
    PROOT_L2S_DIR="$TMP/proot-l2s" HOME="$PROOT_HOME" \
    OMP_BRIDGE_RELEASE_BASE_URL="file://$TMP/release" \
    "$SCRIPT" --version v0.1.0 --dir "$PROOT_HOME/bin-two" >/dev/null 2>&1; then
    fail "installer did not detect PROOT_L2S_DIR"
  fi

  if env -u container -u PROOT_L2S_DIR -u PROOT_TMP_DIR -u PROOT_LOADER \
    OMP_BRIDGE_ALLOW_ROOT=1 OMP_BRIDGE_RELEASE_BASE_URL="file://$TMP/release" \
    "$SCRIPT" --version v0.1.0 --dir "$TMP/container-bin" >"$TMP/container.out" 2>&1; then
    grep -q 'OMP_BRIDGE_ALLOW_ROOT' "$TMP/container.out" ||
      fail "container override did not print a warning"
  else
    fail "installer refused explicit container root override"
  fi
fi

printf 'corrupted\n' >>"$RELEASE/$ASSET"
if clean_root_env \
  OMP_BRIDGE_ALLOW_ROOT=1 \
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
if clean_root_env \
  OMP_BRIDGE_ALLOW_ROOT=1 \
  OMP_BRIDGE_RELEASE_BASE_URL="file://$TMP/release" \
  "$SCRIPT" --version v0.1.0 --dir "$TMP/bin" >/dev/null 2>&1; then
  fail "installer replaced a running bridge"
fi

printf 'installer tests passed (%s)\n' "$TARGET"
