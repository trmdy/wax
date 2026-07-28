#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
readonly PACKAGE_SCRIPT="${SCRIPT_DIR}/package.sh"

TEST_DIR="$(mktemp -d "${TMPDIR:-/tmp}/wax-package-test.XXXXXX")"
readonly TEST_DIR

cleanup() {
  rm -rf -- "$TEST_DIR"
}
trap cleanup EXIT

mkdir -p "${TEST_DIR}/target" "${TEST_DIR}/out" "${TEST_DIR}/unpacked"
printf '#!/usr/bin/env sh\nprintf "wax fixture\\n"\n' > "${TEST_DIR}/target/wax"
chmod 0755 "${TEST_DIR}/target/wax"

(
  cd "${TEST_DIR}/out"
  "$PACKAGE_SCRIPT" "${TEST_DIR}/target" "1.2.3-rc.1" "macos-arm64"
)

readonly ARCHIVE="${TEST_DIR}/out/wax-v1.2.3-rc.1-macos-arm64.tar.gz"
if [[ ! -f "$ARCHIVE" ]]; then
  printf 'ERROR: package test did not produce %s\n' "$ARCHIVE" >&2
  exit 1
fi

archive_entries="$(tar -tzf "$ARCHIVE")"
readonly archive_entries
if [[ "$archive_entries" != $'wax\nREADME.md' ]]; then
  printf 'ERROR: unexpected archive entries:\n%s\n' "$archive_entries" >&2
  exit 1
fi

tar -xzf "$ARCHIVE" -C "${TEST_DIR}/unpacked"
cmp "${TEST_DIR}/target/wax" "${TEST_DIR}/unpacked/wax"
cmp "${SCRIPT_DIR}/../README.md" "${TEST_DIR}/unpacked/README.md"

if [[ "$(uname -s)" == "Darwin" ]]; then
  wax_mode="$(stat -f '%Lp' "${TEST_DIR}/unpacked/wax")"
  readme_mode="$(stat -f '%Lp' "${TEST_DIR}/unpacked/README.md")"
else
  wax_mode="$(stat -c '%a' "${TEST_DIR}/unpacked/wax")"
  readme_mode="$(stat -c '%a' "${TEST_DIR}/unpacked/README.md")"
fi
readonly wax_mode readme_mode
if [[ "$wax_mode" != "755" || "$readme_mode" != "644" ]]; then
  printf 'ERROR: unexpected packaged modes: wax=%s README.md=%s\n' \
    "$wax_mode" "$readme_mode" >&2
  exit 1
fi

if (
  cd "${TEST_DIR}/out"
  "$PACKAGE_SCRIPT" "${TEST_DIR}/target" "1.2.3-rc.1" "macos-arm64"
) >/dev/null 2>&1; then
  printf 'ERROR: package script overwrote an existing archive\n' >&2
  exit 1
fi

if (
  cd "${TEST_DIR}/out"
  "$PACKAGE_SCRIPT" "${TEST_DIR}/target" "1.2.3" "windows-x64"
) >/dev/null 2>&1; then
  printf 'ERROR: package script accepted an unsupported platform\n' >&2
  exit 1
fi

printf 'Package script tests passed.\n'
