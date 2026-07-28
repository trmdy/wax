#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly REPO_ROOT
readonly HARNESS_MANIFEST="harness/wax-harness/Cargo.toml"
readonly ORACLE_DIR="harness/oracle"

fast=false
fuzz_only=false
fuzz_burn=false

usage() {
  printf 'Usage: %s [--fast] [--fuzz-only] [--fuzz-burn]\n' "${0##*/}" >&2
}

while (($# > 0)); do
  case "$1" in
    --fast)
      fast=true
      ;;
    --fuzz-burn)
      fuzz_burn=true
      fuzz_only=true
      ;;
    --fuzz-only)
      fuzz_only=true
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

# Deterministic regression replay: every seed and every artifact ever
# recovered from a burn must still be handled cleanly. This is a *gate*, so
# it must not depend on what a random mutation happens to discover — timed
# discovery burns live in `--fuzz-burn` (and the nightly CI job), where a new
# finding is a task, not a broken build.
smoke_fuzz_target() {
  local nightly_toolchain="$1"
  local target="$2"
  local corpus_dir
  local status=0

  corpus_dir="$(mktemp -d "${TMPDIR:-/tmp}/wax-fuzz-${target}.XXXXXX")"
  cp -R "fuzz/corpus/${target}/." "$corpus_dir/"
  if [[ -d "fuzz/artifacts/${target}" ]]; then
    find "fuzz/artifacts/${target}" -type f -exec cp {} "$corpus_dir/" \;
  fi
  rustup run "$nightly_toolchain" cargo fuzz run "$target" "$corpus_dir" -- \
    -runs=0 -print_final_stats=1 -verbosity=0 || status=$?
  rm -rf -- "$corpus_dir"
  return "$status"
}

burn_fuzz_target() {
  local nightly_toolchain="$1"
  local target="$2"
  local corpus_dir
  local status=0

  corpus_dir="$(mktemp -d "${TMPDIR:-/tmp}/wax-fuzz-${target}.XXXXXX")"
  cp -R "fuzz/corpus/${target}/." "$corpus_dir/"
  rustup run "$nightly_toolchain" cargo fuzz run "$target" "$corpus_dir" -- \
    -max_total_time="${WAX_FUZZ_BURN_SECONDS:-300}" -print_final_stats=1 -verbosity=0 || status=$?
  rm -rf -- "$corpus_dir"
  return "$status"
}

run_fuzz_checks() {
  local nightly_toolchain
  local target

  if ! command -v rustup >/dev/null 2>&1; then
    printf '\n==> SKIP: fuzz checks (rustup/nightly is not available)\n'
    return 0
  fi
  nightly_toolchain="$(
    rustup toolchain list |
      awk '$1 ~ /^nightly(-|$)/ { print $1; exit }'
  )"
  if [[ -z "$nightly_toolchain" ]]; then
    printf '\n==> SKIP: fuzz checks (nightly Rust is not installed)\n'
    return 0
  fi
  if ! rustup run "$nightly_toolchain" cargo fuzz --version >/dev/null 2>&1; then
    printf '\n==> SKIP: fuzz checks (cargo-fuzz is not installed)\n'
    return 0
  fi

  run_step "fuzz: cargo fuzz build" \
    rustup run "$nightly_toolchain" cargo fuzz build
  for target in container_preflight xlsx_reader legacy_xls_reader; do
    if [[ "$fuzz_burn" == true ]]; then
      run_step "fuzz: ${target} ${WAX_FUZZ_BURN_SECONDS:-300} second burn" \
        burn_fuzz_target "$nightly_toolchain" "$target"
    else
      run_step "fuzz: ${target} seed + artifact replay" \
        smoke_fuzz_target "$nightly_toolchain" "$target"
    fi
  done
}

cd "$REPO_ROOT"

if [[ "$fuzz_only" == true ]]; then
  run_fuzz_checks
  exit 0
fi

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

run_fuzz_checks

printf '\nAll CI checks passed.\n'
