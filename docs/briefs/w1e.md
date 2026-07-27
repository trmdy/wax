# Shard W1E — CI: check script + workflows

You are shard **W1E** of the wax v1 mission. Coordinator: bee **CL.661**.

**Required reading:** `MISSION.md`, `docs/w1-contracts.md` §0 (final
layout — write CI against that layout, not the current empty tree) and
§7. Branch `agent/wax-w1e-ci`, this worktree only. No remote yet; the
workflows must be correct for the day the repo is pushed to GitHub.

**Concurrency note:** the workspace (W1A), oracle (W1C) and harness
(W1D) are being built in parallel on other branches. You cannot run the
full pipeline locally yet — make `scripts/check.sh` correct against the
contracts §0 layout and *degrade loudly* (clear error, non-zero exit) if
a component is missing, plus verify what you can (shellcheck, actionlint
or yaml validation).

## Deliverables

1. **`scripts/check.sh`** — the single CI body, also the local
   pre-merge gate: `cargo fmt --check`, `cargo clippy --all-targets
   -- -D warnings`, `cargo test` (workspace root), then the same three
   for the standalone `harness/wax-harness` crate (it has its own
   `[workspace]`), then oracle checks (`npm ci --prefix harness/oracle`
   + its tests) when node is present. Bash strict mode, per-step
   headers, first failure fails the script with a clear name of what
   failed. `--fast` flag skipping the oracle npm install.
2. **`.github/workflows/ci.yml`** — push + PR on all branches: checkout,
   rust toolchain from `rust-toolchain.toml` (fall back to stable),
   cargo + npm caching, node 22, run `scripts/check.sh`. Linux runner;
   add a macOS job for `cargo build --release` only (cheap smoke).
   No corpus fetching in CI (network-heavy; harness E2E stays local
   in W1 — leave a comment saying exactly that).
3. **`.github/workflows/release.yml`** — skeleton only, `workflow_
   dispatch`-triggered, with TODO markers for W5 (per-platform tarballs
   macOS arm64/x64 + Linux x64, checksums). Must be valid yaml that
   does a release build on the three targets; artifact upload can be
   plain `actions/upload-artifact`.
4. **Validation**: shellcheck `scripts/check.sh` clean; workflows pass
   `actionlint` if available (install via brew if missing) else a yaml
   parse check. Include a tiny `scripts/check-ci-defs.sh` that runs
   those validators so it's repeatable.

## Boundaries (do not touch)

Everything outside `scripts/` and `.github/`. Coordinator contact:
`hive buz send CL.661 --sender <your-bee-name> --tier queue -p "<msg>"`.

## Definition of done

- shellcheck + actionlint (or yaml-parse) clean; scripts executable
  (`chmod +x`).
- `scripts/check.sh` run in this worktree fails *loudly and clearly*
  (missing workspace) rather than passing vacuously or crashing
  cryptically.
- Commits end with your bee name.

## Sealing

Seal (`hive seal <your-bee-name> --from seal.json`) with deliverables,
validation evidence, and deviations. Then buz CL.661.
