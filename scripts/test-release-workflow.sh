#!/bin/sh
# Static assertions for the release workflow's exact-tag and source-commit
# guarantees. This does not execute GitHub Actions.
# shellcheck disable=SC2016
set -eu

ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
WORKFLOW="${OMP_BRIDGE_RELEASE_WORKFLOW:-$ROOT/.github/workflows/bridge-release.yml}"

fail() {
  printf 'release workflow test failed: %s\n' "$1" >&2
  exit 1
}

line_of() {
  grep -n -- "$1" "$WORKFLOW" | head -n 1 | cut -d: -f1
}

for required in \
  'name: Resolve requested release tag' \
  'ref: ${{ steps.tag.outputs.tag }}' \
  'name: Verify exact tag, version, and source commit' \
  'git describe --tags --exact-match HEAD' \
  'git rev-parse "$TAG^{commit}"' \
  'test "$TAG" = "v$VERSION"' \
  'echo "commit=$COMMIT" >> "$GITHUB_OUTPUT"' \
  'BUILD_INFO.txt' \
  'Source commit: $COMMIT'; do
  grep -Fq -- "$required" "$WORKFLOW" || fail "missing: $required"
done

RESOLVE_LINE="$(line_of 'name: Resolve requested release tag')"
CHECKOUT_LINE="$(line_of 'name: Check out the exact release tag')"
VERIFY_LINE="$(line_of 'name: Verify exact tag, version, and source commit')"
BUILD_LINE="$(line_of 'name: Build release binaries')"
[ -n "$RESOLVE_LINE" ] && [ -n "$CHECKOUT_LINE" ] && [ -n "$VERIFY_LINE" ] && [ -n "$BUILD_LINE" ] ||
  fail "missing workflow step"

[ "$RESOLVE_LINE" -lt "$CHECKOUT_LINE" ] || fail "tag must resolve before checkout"
[ "$CHECKOUT_LINE" -lt "$VERIFY_LINE" ] || fail "tag must be checked out before verification"
[ "$VERIFY_LINE" -lt "$BUILD_LINE" ] || fail "tag and version must be verified before build"

printf 'release workflow assertions passed\n'
