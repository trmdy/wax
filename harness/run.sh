#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
default_repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
repo_root=${WAX_REPO_ROOT:-$default_repo_root}
oracle_script=${WAX_ORACLE_SCRIPT:-$repo_root/harness/oracle/run.js}

if ! command -v node >/dev/null 2>&1; then
  echo "wax harness: node is required to run the SheetJS oracle" >&2
  exit 1
fi

if [ ! -f "$oracle_script" ]; then
  echo "wax harness: oracle runner not found at $oracle_script" >&2
  echo "wax harness: complete W1C setup and run npm ci in harness/oracle/" >&2
  exit 1
fi

if [ -z "${WAX_ORACLE_SCRIPT:-}" ] && [ ! -d "$repo_root/harness/oracle/node_modules/xlsx" ]; then
  echo "wax harness: oracle dependencies are missing" >&2
  echo "wax harness: run npm ci in $repo_root/harness/oracle/" >&2
  exit 1
fi

if [ -z "${WAX_BIN:-}" ]; then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "wax harness: cargo is required to build wax" >&2
    exit 1
  fi
  (
    cd "$repo_root"
    cargo build --release
  )
  WAX_BIN="$repo_root/target/release/wax"
  export WAX_BIN
fi

if [ -n "${WAX_HARNESS_BIN:-}" ]; then
  harness_bin=$WAX_HARNESS_BIN
else
  if ! command -v cargo >/dev/null 2>&1; then
    echo "wax harness: cargo is required to build the harness" >&2
    exit 1
  fi
  harness_target_dir=${WAX_HARNESS_TARGET_DIR:-$script_dir/wax-harness/target}
  cargo build \
    --release \
    --manifest-path "$script_dir/wax-harness/Cargo.toml" \
    --target-dir "$harness_target_dir"
  harness_bin="$harness_target_dir/release/wax-harness"
fi

exec "$harness_bin" --repo-root "$repo_root" "$@"
