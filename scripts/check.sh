#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly REPO_ROOT
readonly HARNESS_MANIFEST="harness/wax-harness/Cargo.toml"
readonly ORACLE_DIR="harness/oracle"

fast=false

usage() {
  printf 'Usage: %s [--fast]\n' "${0##*/}" >&2
}

while (($# > 0)); do
  case "$1" in
    --fast)
      fast=true
      ;;
    *)
      printf 'ERROR: unknown argument: %s\n' "$1" >&2
      usage
      exit 2
      ;;
  esac
  shift
done

run_step() {
  local name="$1"
  local status
  shift

  printf '\n==> %s\n' "$name"
  if "$@"; then
    printf '<== PASS: %s\n' "$name"
  else
    status=$?
    printf '<== FAIL: %s (exit %d)\n' "$name" "$status" >&2
    exit "$status"
  fi
}

require_file() {
  local path="$1"
  local description="$2"

  if [[ ! -f "$path" ]]; then
    printf 'ERROR: missing %s: %s\n' "$description" "$path" >&2
    return 1
  fi
}

require_command() {
  local command_name="$1"

  if ! command -v "$command_name" >/dev/null 2>&1; then
    printf 'ERROR: required command is not available: %s\n' "$command_name" >&2
    return 1
  fi
}

check_layout() {
  require_file "Cargo.toml" "root Cargo workspace manifest" || return 1
  require_file "$HARNESS_MANIFEST" "standalone wax-harness manifest" || return 1
  require_file "${ORACLE_DIR}/package.json" "oracle package manifest" || return 1
  if [[ "$fast" == false ]]; then
    require_file "${ORACLE_DIR}/package-lock.json" "oracle npm lockfile" || return 1
  fi
  require_command cargo || return 1

  if command -v node >/dev/null 2>&1; then
    require_command npm || return 1
  fi
}

cd "$REPO_ROOT"

run_step "preflight: repository layout and tools" check_layout

run_step "workspace: cargo fmt --check" cargo fmt --check
run_step "workspace: cargo clippy --all-targets" \
  cargo clippy --all-targets -- -D warnings
run_step "workspace: cargo test" cargo test

run_step "wax-harness: cargo fmt --check" \
  cargo fmt --manifest-path "$HARNESS_MANIFEST" -- --check
run_step "wax-harness: cargo clippy --all-targets" \
  cargo clippy --manifest-path "$HARNESS_MANIFEST" --all-targets -- -D warnings
run_step "wax-harness: cargo test" \
  cargo test --manifest-path "$HARNESS_MANIFEST"

if command -v node >/dev/null 2>&1; then
  if [[ "$fast" == false ]]; then
    run_step "oracle: npm ci" npm ci --prefix "$ORACLE_DIR"
  else
    printf '\n==> SKIP: oracle npm ci (--fast)\n'
  fi
  run_step "oracle: npm test" npm test --prefix "$ORACLE_DIR"
else
  printf '\n==> SKIP: oracle checks (node is not available)\n'
fi

printf '\nAll CI checks passed.\n'
