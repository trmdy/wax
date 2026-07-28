# Review — W3D: harness protocol client + scoreboard rows (`agent/wax-w3d-harness`, 12f6c0e)

Reviewer: CL.d73 (coordinator). Verdict: **merge, with two coordinator
integration patches** (below).

## Scope check

Diff stays in `harness/wax-harness/**` (+ its README/fixtures). Notably it
did **not** need to touch `harness/run.sh` — the serve pass lives inside
the harness binary, which is cleaner than the brief's assumption. 19 files,
+1,827/−36. `scripts/check.sh` re-verified green by the coordinator in the
shard worktree (39 harness tests).

## What's there

- **Serve client** (`src/serve.rs`): fresh `wax serve` per file;
  version-handshake validation → open → meta → deterministic 64×24 windows
  (corners + center via `window_offsets`, dedup'd, small-sheet aware) →
  stats → close → EOF with `wait_timeout` + kill fallback. Out-of-order
  responses handled with a completed-response cache keyed by id. Every
  request records wall-time + ok/error; aborts record all in-flight ids;
  stderr captured (capped 4 KiB) for diagnostics; `Drop` kills the child.
  Timeouts/kills recorded as failures, never hidden — matches the spec.
- **Availability probe** (`detect_serve`): `wax serve --help` heuristic;
  hang-or-clean-exit ⇒ available, recognizable "unknown subcommand" ⇒
  `n/a (serve unavailable)`, anything else (a supported-but-broken serve)
  deliberately counts as available so breakage lands in per-file failure
  data instead of hiding behind n/a. Right call.
- **Scoreboard**: additive camelCase fields under `metrics`:
  `openViaServe`, `windowLatencyPercentilesMs {p50,p95}`,
  `servePeakRssBytes {p50,max}`, `serveStatus {status,reason}`; per-ext
  table gains formula-text + cached-result fidelity columns. Historical
  values honestly `n/a` in the committed snapshot (no corpus payloads in
  the worktree) — correct per contract, refreshed at integration.
- Export smoke on a deterministic xlsx subset, temp-dir CSV, failure
  chained (open-fail ⇒ smoke marked skipped, not silently absent).
- Tests: mock server (`mock-serve.js`) covering happy path, out-of-order
  ids, mid-session errors, process death, hangs (client-side timeout);
  aggregation math unit-tested; golden SCOREBOARD.md render.

## Deviations, accepted and recorded

1. **JSON shape**: the contract amendment specified a `serveMetrics`
   object; W3D shipped the same data as flat additive `metrics.*` fields
   and flagged it in the seal. The substance (additive, backward
   compatible, p50/p95 + RSS + open%) is intact and tested; the shard
   exited after sealing. Accepted — contract re-amended to match the
   shipped shape rather than reworking tested code for a cosmetic
   difference.
2. **Legacy `metrics.windowLatencyMs`** stays `{wax:null,sheetjs:null}`
   permanently instead of gaining a p50 scalar fill. Accepted for the same
   reason; the SCOREBOARD.md row renders real p50/p95 from the percentile
   field, which is what the operator asked to see.

## Coordinator integration patches (applied on main at merge)

1. `detect_serve` unavailable-patterns miss wax's own historical error
   text ``wax: unknown command `serve` `` (patterns only cover clap-style
   spellings). Harmless post-merge (serve always exists) but wrong if
   `WAX_BIN` points at a pre-W3A binary: misdetects as available and
   scores 0% instead of n/a. One-line pattern addition.

## Verdict

Solid client with honest failure accounting and the right degrade
behavior. The real-binary corpus numbers land in the post-merge harness
run.
