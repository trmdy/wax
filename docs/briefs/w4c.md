# Shard W4C — protocol/CLI export wiring + CI-green duty

You are shard **W4C** of the wax v1 mission, wave 4. Coordinator: bee
**CL.6cbf**.

**Required reading before any code:** `MISSION.md`, `docs/w4-contracts.md`
(§3 is your spec; §2 is the writer API you consume),
`docs/w3-contracts.md` §1 (proto v0), `crates/wax-cli/src/serve.rs`. You
work in your git worktree only (`.worktrees/w4c`), on branch
`agent/wax-w4c-export`. Never touch `main`; never `git push`. Commit
locally, the coordinator merges.

## The job

1. **serve `export` xlsx**: wire `wax_write::write_xlsx` into the existing
   export worker path (op shape unchanged; `sheet` ignored for xlsx but
   still range-validated; handle open-warnings appended to `dropped`;
   `WriteError` → wire error). CSV delegates to `wax_write::write_csv`
   once W4A's implementation merges — until then keep serve's local CSV
   path working; structure your change so the swap is one commit.
2. **`wax export` subcommand** per contract §3: reader options + deadline
   like `dump`, store build, writer call, one JSON result line, exit 0,
   help text updated. This is the harness's bulk entry point — W4B builds
   against its exact output shape, so match the contract to the letter.
3. **CI-green duty**: the repo is public and GitHub Actions is live.
   After every W4 merge the coordinator pushes `main`; you own diagnosing
   and fixing CI breakage fast (`gh run list` / `gh run view`), including
   pre-existing workflow rot the first live runs uncover. You own
   `scripts/**` and `.github/**` this wave. Do not weaken checks to get
   green — fix causes.

Sequencing: the wax-write stub on `main` returns structured `internal`
errors — your wiring, tests included, must already behave sanely against
the stub (export xlsx over serve returns that structured error). Write
the end-to-end success-path tests now but gate/ignore them until W4A
merges; the coordinator will tell you when to flip them on and do the CSV
delegation swap.

## Tests

Serve integration tests: xlsx export end-to-end (open fixture → export →
re-dump → assert values, merges), error paths (`bad_handle`, out-of-range
`sheet` for both formats, unwritable `out` ⇒ `internal` with io context,
unknown format ⇒ `unsupported`), warnings-appended-to-dropped, cancel
during export (no partial file). CLI tests: `wax export` JSON shapes for
success/reader-failure/writer-failure, `--sheet`/`--format` validation,
help text.

## Boundaries (do not touch)

`crates/wax-write/**` (W4A), `crates/wax-read/**`, `crates/wax-core/**`,
`crates/wax-store/**` (W4D/coordinator), `harness/**` (W4B), `corpus/**`,
`docs/**`, `ASSIGNMENTS.json`. You own `crates/wax-cli/**`,
`crates/wax-proto/**` (no wire changes without coordinator sign-off),
`scripts/**`, `.github/**`.

## Definition of done

- `scripts/check.sh` fully green in your worktree.
- `wax export` + serve xlsx wiring complete and tested against the stub;
  success-path tests ready to flip on at integration.
- Any CI breakage you were asked to handle is fixed with causes named.
- Commit messages end with your bee name.

## Sealing

`hive seal <your-bee-name>` with status, summary, deliverables, exact test
counts, and deviations. Then
`hive buz send CL.6cbf --sender <your-bee-name> --tier queue -p "sealed"`.
Blockers → buz immediately; never a silent stall.
