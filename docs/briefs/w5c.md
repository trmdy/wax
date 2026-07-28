# Shard W5C — release CI + README + vendoring doc

You are shard **W5C** of the wax v1 mission, wave 5 (hardening + release).
Coordinator: bee **CL.7c63**.

**Required reading before any code:** `MISSION.md` (the repo header: how
apiary consumes wax; §Protocol v0), `.github/workflows/release.yml` (the
W4-era skeleton you are finishing), `.github/workflows/ci.yml`,
`scripts/check.sh`, `crates/wax-cli` (`--version` already prints
`wax 0.1.0 (proto 0)` — that pairing is the contract). You work in your
git worktree only (`.worktrees/w5c`), on branch `agent/wax-w5c-release`.
Never touch `main`; never `git push`. Commit locally, the coordinator
merges and runs the real GitHub testing.

## The job

### 1. Release workflow

Finish `release.yml` so that pushing a tag `v*` produces a GitHub release
with per-platform artifacts:

- Targets stay as in the skeleton: macOS arm64 (`aarch64-apple-darwin`),
  macOS x64 (`x86_64-apple-darwin`), Linux x64
  (`x86_64-unknown-linux-gnu`).
- Package each binary as `wax-v<version>-<platform>.tar.gz` with platform
  slugs `macos-arm64`, `macos-x64`, `linux-x64`; tarball contains the
  `wax` binary (mode 0755) + `README.md`. Put the packaging logic in
  `scripts/package.sh` (takes target dir + version + platform slug) so it
  is locally testable and CI just calls it.
- One `SHA256SUMS.txt` covering all three tarballs (standard
  `shasum -a 256` format: `<hex>  <filename>`), generated in a final job
  that downloads all build artifacts.
- Create the GitHub release (use `gh release create` or
  `softprops/action-gh-release`) attaching the three tarballs +
  `SHA256SUMS.txt`. Keep `workflow_dispatch` as a dry-run path that builds
  + packages + uploads workflow artifacts but does **not** create a
  release.
- Guard: the job must fail if the tag version ≠ the workspace
  `Cargo.toml` version ≠ what `wax --version` prints. Check binary sanity
  before packaging (`./wax --version` on the runner; Linux runner can run
  its own binary, macOS runners theirs).
- `permissions: contents: write` only where the release is created;
  builds stay read-only.
- Note: MISSION.md says "signed" tarballs; the operator-ratified W5 scope
  is sha256 checksums only — no code signing/notarization. Record that as
  a deviation in your seal, do not attempt signing.

### 2. README

Rewrite `README.md` as the real front page:

- What wax is (one paragraph, out-of-process sheet engine).
- **Install**: download from GitHub releases (per-platform tarball
  names), verify against `SHA256SUMS.txt`, extract, `wax --version`.
  Build-from-source as the alternative (`cargo build --release`, pinned
  toolchain note).
- **Serve protocol**: NDJSON-over-stdio overview — `open / meta / window /
  export / close`, cancellation, caps/timeouts, error codes, the
  `proto` number in the `open` response and in `wax --version`; a short
  real example session (crib from MISSION.md §Protocol v0 but reflect the
  *implemented* protocol — read `crates/wax-proto`/serve tests, do not
  invent fields).
- **Harness**: corpus + oracle + scoreboard in a few paragraphs; how to
  run `harness/run.sh`; pointer to SCOREBOARD.md and adjudications.
- Keep it honest and tight; link MISSION.md for the deep story.

### 3. `docs/vendoring.md` — what apiary needs

Written for the apiary sheet-pane integrator (APIA-162): exact artifact
names per platform, the platform→slug mapping, checksum file name +
format + verify command, version/proto pairing rules (`wax --version`
format, when proto bumps, what a consumer should pin: version **and**
sha256), where binaries live in apiary conventions (PATH in dev, vendored
`extraResources` in packaged builds — cite MISSION.md), and the
subprocess contract in two sentences (stdio NDJSON, SIGTERM clean).

## Validation

You cannot run GitHub Actions from the worktree. Instead: `actionlint` on
both workflows (install if missing), `shellcheck` + `bash -n` on
`scripts/package.sh`, and a full local dry-run of the packaging path:
`cargo build --release`, then `scripts/package.sh` for the host platform,
untar the result, run `./wax --version`, verify the checksum line. The
coordinator will push your branch and `workflow_dispatch`-test the real
matrix before merge — design for that (dispatch dry-run path must work
from a branch ref).

## Boundaries (do not touch)

Your lane: `.github/**`, `scripts/**` (additive — do not break
`check.sh`), `README.md`, `docs/vendoring.md`. Not any crate code
(`--version` is already correct), not `harness/**`, not `fuzz/**`. CI
(`ci.yml`) only if a fix is needed for release plumbing — flag it in the
seal.

## Definition of done

- Tag path produces release + 3 tarballs + SHA256SUMS.txt (proven by
  actionlint + local packaging dry-run; real-matrix proof happens at
  coordinator dispatch-test).
- README + vendoring doc complete and accurate against the implemented
  protocol.
- `scripts/check.sh` fully green in your worktree.
- Commit messages end with your bee name.

## Sealing

`hive seal <your-bee-name>` (implementation seal) with status, summary,
deliverables, validation evidence (actionlint/shellcheck/dry-run output
summary), and deviations (incl. the no-signing note). Then
`hive buz send CL.7c63 --sender <your-bee-name> --tier queue -p "sealed"`.
Blockers → buz immediately; never a silent stall.
