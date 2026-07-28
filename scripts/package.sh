#!/usr/bin/env bash

set -euo pipefail

usage() {
  printf 'Usage: %s <target-dir> <version> <platform>\n' "${0##*/}" >&2
  printf 'Platforms: macos-arm64, macos-x64, linux-x64\n' >&2
}

if (($# != 3)); then
  usage
  exit 2
fi

readonly TARGET_DIR="$1"
readonly VERSION="$2"
readonly PLATFORM="$3"

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly REPO_ROOT
readonly BINARY="${TARGET_DIR}/wax"
readonly README="${REPO_ROOT}/README.md"
readonly ARCHIVE_NAME="wax-v${VERSION}-${PLATFORM}.tar.gz"
readonly ARCHIVE_PATH="${PWD}/${ARCHIVE_NAME}"

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
  printf 'ERROR: version must be an unprefixed semantic version: %s\n' \
    "$VERSION" >&2
  exit 2
fi

case "$PLATFORM" in
  macos-arm64 | macos-x64 | linux-x64) ;;
  *)
    printf 'ERROR: unsupported platform slug: %s\n' "$PLATFORM" >&2
    usage
    exit 2
    ;;
esac

if [[ ! -f "$BINARY" || ! -x "$BINARY" ]]; then
  printf 'ERROR: target binary is missing or not executable: %s\n' \
    "$BINARY" >&2
  exit 1
fi
if [[ ! -f "$README" ]]; then
  printf 'ERROR: repository README is missing: %s\n' "$README" >&2
  exit 1
fi
if [[ -e "$ARCHIVE_PATH" ]]; then
  printf 'ERROR: refusing to overwrite existing archive: %s\n' \
    "$ARCHIVE_PATH" >&2
  exit 1
fi

STAGING_DIR="$(mktemp -d "${TMPDIR:-/tmp}/wax-package.XXXXXX")"
readonly STAGING_DIR
ARCHIVE_TMP="$(mktemp "${PWD}/.${ARCHIVE_NAME}.XXXXXX")"
readonly ARCHIVE_TMP

cleanup() {
  rm -rf -- "$STAGING_DIR"
  rm -f -- "$ARCHIVE_TMP"
}
trap cleanup EXIT

cp "$BINARY" "${STAGING_DIR}/wax"
cp "$README" "${STAGING_DIR}/README.md"
chmod 0755 "${STAGING_DIR}/wax"
chmod 0644 "${STAGING_DIR}/README.md"

(
  cd "$STAGING_DIR"
  tar -czf "$ARCHIVE_TMP" wax README.md
)
chmod 0644 "$ARCHIVE_TMP"
mv "$ARCHIVE_TMP" "$ARCHIVE_PATH"

printf 'Created %s\n' "$ARCHIVE_PATH"
