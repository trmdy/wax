# Shard W1D — differential harness + scoreboard

You are shard **W1D** of the wax v1 mission. Coordinator: bee **CL.661**.

**Required reading:** `MISSION.md` (§Scoreboard), `docs/w1-contracts.md`
§1 (dump schema), §2 (CLI + `$WAX_BIN`), §4 (manifest), §5 (oracle
invocation), §6 (scoreboard — binding). Branch `agent/wax-w1d-harness`,
this worktree only. No remote; commit locally.

**Concurrency note:** the wax binary (W1A), oracle (W1C) and corpus (W1B)
are being built in parallel — they are NOT on your branch. Develop
against the frozen contracts using mocks/fixtures; integration against
the real pieces happens on `main` after merge (coordinator runs it, and
may buz you to fix fallout).

## Deliverables

1. **`harness/wax-harness/`** — a Rust crate that is **standalone
   buildable**: give its `Cargo.toml` an empty `[workspace]` table so it
   does not require the root workspace (the coordinator wires it into
   the root workspace at integration). It contains:
   - dump-document model (deserialize contracts §1 JSON; tolerate and
     surface schema violations as per-file errors, not panics),
   - the comparator: given a wax dump and a sheetjs dump for the same
     file, compute per-file metrics — opened, cell-value matches over
     the union of non-empty cells (`(t,v)` equal; numbers within 1e-9
     relative), display coverage, formula fidelity
     (whitespace-normalized `f`), cached-result fidelity, plus wall
     times and RSS,
   - the aggregator + renderers: `SCOREBOARD.md` (repo root) and
     `harness/scoreboard.json`, columns exactly per contracts §6;
     metrics W1 cannot honestly produce print `n/a` (window latency).
     **Never fabricate a metric.**
2. **Runner binary** (in the same crate): reads `corpus/manifest.jsonl`
   (`--manifest`, `--limit`, `--jobs`), for each file spawns wax
   (`$WAX_BIN`, fallback `target/release/wax`) and the oracle
   (`node harness/oracle/run.js`), same `--max-cells` both sides,
   per-file timeout, records crashes/timeouts/OOM as `ok:false` rows
   instead of dying, skips `private:true` entries whose absolute path
   doesn't exist, streams per-file results to `harness/results.jsonl`
   (gitignored), then aggregates.
3. **`harness/run.sh`** — thin entry point:
   `harness/run.sh [--manifest <path>] [--limit N] [--jobs N]`; builds
   the harness crate (and wax via `cargo build --release` if `$WAX_BIN`
   unset), runs the runner, regenerates `SCOREBOARD.md` +
   `harness/scoreboard.json`. Fails loudly if oracle deps are missing
   (tell the user to `npm ci` in `harness/oracle/`).
4. **`harness/adjudications.md`** — seeded with the format + rules from
   contracts §6 (empty verdict table; disagreements are never
   auto-resolved in SheetJS's favour).
5. **Tests** (the bulk of your value): comparator unit tests over
   hand-written dump-pairs (exact match, numeric tolerance edges, type
   mismatch, formula whitespace, truncated pairs, one-side-failed),
   aggregator tests (percentiles, division-by-zero guards, empty
   corpus), scoreboard rendering golden test, runner integration test
   using **fake `$WAX_BIN` and oracle scripts** (tiny shell/node stubs
   emitting canned dumps) against a 3-line fixture manifest.

## Boundaries (do not touch)

`crates/`, `corpus/` (fixtures live inside `harness/wax-harness/tests/`),
`harness/oracle/`, `scripts/`, `.github/`, `docs/`, root files except
`SCOREBOARD.md` generation logic (don't commit a fake SCOREBOARD.md —
it's generated at integration). Contract problems → buz coordinator:
`hive buz send CL.661 --sender <your-bee-name> --tier queue -p "<msg>"`.

## Definition of done

- `cargo test` green in `harness/wax-harness/`; fmt + clippy
  (`-D warnings`) clean.
- End-to-end `harness/run.sh --manifest tests/fixtures/manifest.jsonl`
  works with the fake binaries (document how you invoke it in tests).
- Commits end with your bee name.

## Sealing

Seal (`hive seal <your-bee-name> --from seal.json`) with deliverables,
test counts, and deviations. Then buz CL.661.
