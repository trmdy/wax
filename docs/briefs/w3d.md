# Shard W3D — harness protocol client + scoreboard upgrades

You are shard **W3D** of the wax v1 mission, wave 3. Coordinator: bee
**CL.d73**.

**Required reading before any code:** `MISSION.md`, `docs/w1-contracts.md`
§5–6, `docs/w2-contracts.md` §3, `docs/w3-contracts.md` §4 (your spec) and
§1 (the wire protocol you drive — it is frozen; build against the spec,
not against W3A's branch). You work in your git worktree only
(`.worktrees/w3d`), on branch `agent/wax-w3d-harness`. Never touch
`main`. No remote; commit locally, the coordinator merges.

## The job

You carry the W3 gate measurement: a harness client that drives the real
`wax serve` end-to-end, plus the scoreboard rows the operator ordered.

1. **Protocol client** (Rust, inside `harness/wax-harness`): per corpus
   file, spawn a fresh `wax serve` (fresh process — RSS discipline, same
   reason the oracle is per-file), then `version` → `open` → `meta` →
   window `(0,0,64,24)` → 4 more 64×24 windows at deterministic in-extent
   offsets (corners + center; fewer when the sheet is small) → `stats` →
   `close` → EOF. Record per-request wall time, open success, and
   `stats.peakRssBytes`. Timeouts/kills recorded as failures, never
   hidden. Export smoke: deterministic ~50-file xlsx subset, `export` CSV
   to a temp dir, assert `ok:true` + non-empty output.
2. **Scoreboard rows** (additive JSON in `harness/scoreboard.json`, table
   rows in `SCOREBOARD.md`; never fabricate — `n/a (serve unavailable)`
   when the binary lacks `serve`, and the run must still complete):
   - `window latency` p50/p95 over all window requests corpus-wide (this
     replaces the standing `n/a`).
   - `serve peak RSS (p50 / max)`.
   - `open-via-serve %` (should track `files opened %`; print both, a gap
     is a finding).
3. **Per-extension table**: add `formula-text fidelity %` and
   `cached-result fidelity %` columns (same definitions as the corpus-wide
   rows, split by manifest `ext`). Operator's explicit ask: xlsx formula
   fidelity is ~99.96% and must be visible instead of the misleading
   18.80% corpus-wide figure. Keep the corpus-wide rows too — honest both
   ways.
4. `harness/run.sh` grows the serve pass (flag-gated if useful, on by
   default when the binary supports `serve`); existing dump-differential
   behavior and JSON fields stay backward-compatible.

## Working without W3A's branch

W3A implements `serve` concurrently. Until it merges: code the client
against `docs/w3-contracts.md` §1 exactly, and test it against a mock —
a small fixture server (a node or shell script speaking canned v0
NDJSON, including out-of-order responses, an error case, and a stats
payload) checked in under `harness/wax-harness/tests/`. The first
real-binary corpus run happens at integration; the coordinator drives it
and you fix fallout. If the spec is ambiguous anywhere, buz CL.d73 —
the contract gets amended, not guessed at.

## Tests

Client unit tests (request building, id correlation incl. out-of-order,
window-offset selection incl. small sheets, latency aggregation math);
mock-server integration tests (happy path, error mid-session, process
death mid-session, hang → client-side timeout); scoreboard generation
tests updated for the new rows/columns (existing tests keep passing;
fixtures extended).

## Boundaries (do not touch)

`crates/**` (you spawn the binary, you don't edit it), `harness/oracle/**`,
`harness/formats/**`, `corpus/**`, `scripts/**`, `.github/**`, `docs/**`,
`ASSIGNMENTS.json`. You own `harness/wax-harness/**` and `harness/run.sh`.

## Definition of done

- `scripts/check.sh` fully green in your worktree.
- Client + rows implemented; full mock-backed test suite; a
  `harness/run.sh --limit 20` sanity run works in your worktree (serve
  rows showing `n/a (serve unavailable)` until integration is expected
  and correct).
- Commit messages end with your bee name.

## Sealing

`hive seal <your-bee-name> --from seal.json` with status, summary,
deliverables, exact test counts, and deviations. Then
`hive buz send CL.d73 --sender <your-bee-name> --tier queue -p "sealed"`.
Blockers → buz immediately; never a silent stall.
