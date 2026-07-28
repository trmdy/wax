# Review — W4B: writer round-trip validation + scoreboard (`agent/wax-w4b-roundtrip`, aa2f94e)

Reviewer: CL.6cbf (coordinator, agent-assisted deep review). Verdict:
**merge after one fast-follow fix** (sent to the shard 2026-07-28; see
the fix log at the bottom).

## Scope check

Diff stays in `harness/wax-harness/**` (+ README/fixtures); `run.sh`
needed no change (it already forwards `$@`, so `--soffice` flows
through). 17 files, +1,736/−24. Verified in a detached worktree at
aa2f94e: 49 harness tests green (33 unit + 3 render + 8 runner + 5
serve), clippy clean.

## What's there

- **Round-trip pass** (`src/roundtrip.rs`): per wax-opened file, fresh
  `wax export --json … --format xlsx` into a tempdir, SheetJS oracle
  read-back, `wax dump` re-read, model→model compare. Runs inside the
  existing worker pool, so it parallelizes like the dump pass.
- **Comparator**: reuses `cells_have_equal_values` (t + numeric-epsilon
  `values_equal`) for value fidelity; display fidelity is raw `d`
  equality over every M1 cell (denominator = M1 cells, per contract);
  merges compared set-wise per sheet → per-file triage defect rows, not
  a headline metric. Extra M2 cells and sheet-name/count drift are
  structure defects; any defect makes the file unclean. Failed export
  or re-read ⇒ `status:"failed"`, counted in the files-clean
  denominator — never silently excluded. Truncated models skipped,
  counted, rendered.
- **Degradation**: missing `export` subcommand (matches both clap's
  "unrecognized subcommand" and wax's own ``unknown command `export` ``
  spelling) or a structured `internal` response ⇒ per-file
  `xlsxExportUnavailable`; a run with only such evidence renders the
  *whole* section `n/a (xlsx export unavailable)` with `percent: null`
  — no fabricated numbers, run completes.
- **soffice**: PATH → app-bundle → `WAX_SOFFICE_BIN` detection;
  deterministic seeded subset (extension round-robin, 64 MiB source
  cap, ≤200 files); headless convert with a fresh
  `-env:UserInstallation` per file; clean = exit 0 + non-empty output;
  timeout `max(--timeout-ms, 60s)` — the floor matches the measured
  ~41s fresh-profile cold start. Absent/disabled ⇒ loud
  `n/a (soffice unavailable/disabled)`. Flag-gated (`--soffice`), so
  quick loops stay quick.
- **Scoreboard/JSON**: "Writer round-trip" section + additive
  `metrics.roundTrip {filesClean, valueMatch, displayMatch,
  oracleOpenRate, sofficeOpenRate, skippedTruncated, status}`; all
  ratios carry numerators/denominators. Old fields untouched — pinned
  by a new legacy-stability test plus the existing byte-exact
  SCOREBOARD.md snapshot test. `results.jsonl` gains only a
  skip-if-absent `roundTrip` object.
- Tests: three new runner integration tests (mocked export + soffice +
  oracle happy path with honest failed-export bookkeeping; stub
  `internal` ⇒ whole-section n/a; enabled-but-missing soffice ⇒ n/a)
  plus unit tests for comparator epsilon reuse, deterministic subset
  selection, soffice detection, and honest aggregation. The serve.rs
  hung-server test got a legitimate anti-flake headroom bump
  (assertion strengthened, not weakened).

## Findings

1. **minor (required fix)** — `aggregate.rs::aggregate_round_trip`:
   availability evidence is too broad — any per-file failure that isn't
   stage=export (tempdir io failure, export spawn error/timeout) counts
   as evidence that export exists. One such failure in an otherwise
   all-stub run flips the section from the mandated loud n/a to
   `available` with a fabricated-looking 0.00% files-clean row. Fix:
   evidence = files whose export stage actually succeeded.
2. minor — every `code:"internal"` export response is classified
   unavailable. Spec-tolerated today (the stub is indistinguishable);
   post-W4A the harness should stop treating `internal` as
   unavailability so a live-writer regression can't masquerade as n/a.
3. minor (accepted deviation) — fresh soffice profile **per file**, not
   per run: necessary for parallelism (LibreOffice locks its profile),
   at the cost of a cold start per check; flag-gated. The 60s timeout
   floor is coherent with this design.
4. minor (accepted) — the soffice subset is selected from manifest
   entries before open/export outcomes are known, so the effective
   sample can fall somewhat below 200.
5. nits — profile URL not percent-encoded (spacey TMPDIR breaks soffice,
   loudly); export `dropped` parsed and discarded (free signal unused);
   triage's oracle read-back section omits private files without a count
   (fix welcome, requested); zero-attempt runs report
   `status:"available"` with 0/0 ratios; `ColInfo` lacks
   `deny_unknown_fields` unlike its siblings.

## Verdict

Honest bookkeeping throughout: failures are counted, skips are loud,
n/a is never a fake 100% or a fake 0%. The degradation path is
exercised end-to-end by tests, which is exactly what the pre-W4A/W4C
merge order needs. Real corpus numbers land at integration.

## Fix log

- 2026-07-28: coordinator sent finding 1 as the required pre-merge fix
  (+ the triage private-count nit as welcome); finding 2 deferred until
  after W4A's writer merges.
- 2026-07-28 (fixes sealed, commit f5d58ea): availability evidence now
  requires an actually-succeeded export stage (`export_stage_succeeded`),
  pinned by `unavailable_exports_with_tempdir_failure_remain_unavailable`;
  triage's oracle read-back section counts private-file failures without
  leaking paths (no-leak asserted). Merged to main together with W4C.
