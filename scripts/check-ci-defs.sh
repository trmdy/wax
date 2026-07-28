#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly SCRIPT_DIR
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
readonly REPO_ROOT
readonly CI_WORKFLOW="${REPO_ROOT}/.github/workflows/ci.yml"
readonly RELEASE_WORKFLOW="${REPO_ROOT}/.github/workflows/release.yml"
readonly CHECK_SCRIPT="${SCRIPT_DIR}/check.sh"

for required_file in \
  "$CHECK_SCRIPT" \
  "$CI_WORKFLOW" \
  "$RELEASE_WORKFLOW"; do
  if [[ ! -f "$required_file" ]]; then
    printf 'ERROR: missing CI definition: %s\n' "$required_file" >&2
    exit 1
  fi
done

if ! command -v shellcheck >/dev/null 2>&1; then
  printf 'ERROR: shellcheck is required to validate CI scripts\n' >&2
  exit 1
fi

printf '==> shellcheck\n'
shellcheck "$CHECK_SCRIPT" "${SCRIPT_DIR}/check-ci-defs.sh"

if command -v actionlint >/dev/null 2>&1; then
  printf '==> actionlint\n'
  (
    cd "$REPO_ROOT"
    actionlint .github/workflows/ci.yml .github/workflows/release.yml
  )
elif command -v ruby >/dev/null 2>&1; then
  printf '==> YAML parse (actionlint unavailable)\n'
  ruby -ryaml -e 'ARGV.each { |path| YAML.parse_file(path) or abort("empty YAML: #{path}") }' \
    "$CI_WORKFLOW" "$RELEASE_WORKFLOW"
else
  printf 'ERROR: actionlint is unavailable and no YAML parser was found\n' >&2
  exit 1
fi

printf 'CI definitions are valid.\n'
