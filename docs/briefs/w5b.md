# Shard W5B — corpus triage to green

You are shard **W5B** of the wax v1 mission, wave 5 (hardening + release).
Coordinator: bee **CL.7c63**.

**Required reading before any code:** `MISSION.md` (§The corpus and the
oracle — adjudication rules), `harness/triage.md` (your worklist),
`harness/adjudications.md` (the ledger + evidence bar),
`docs/w2-contracts.md` (cell-model semantics), `SCOREBOARD.md`. You work
in your git worktree only (`.worktrees/w5b`), on branch
`agent/wax-w5b-triage`. Never touch `main`; never `git push`. Commit
locally, the coordinator merges.

## The job

Drive every bucket in `harness/triage.md` to a terminal state. For each
open-failure file and each mismatch bucket the outcome is exactly one of:

1. **Fixed** — wax now opens/matches; regression-tested.
2. **Adjudicated** — per-file evidence in `harness/adjudications.md`
   (append-only) showing wax is right and SheetJS is wrong. The existing
   entries set the evidence bar: stored bytes, format semantics, or
   structural corruption — never "we differ".
3. **Known limitation** — last resort, needs coordinator sign-off via buz
   before you write it down.

No silent losses: the sum of your ledger must account for every bucket in
the current triage doc. Write the summary ledger (bucket → outcome →
evidence pointer) to `docs/w5-triage.md`.

### Priorities (by impact, work top-down)

- **3 `internal` open failures** (`64130.xls`, `Simple.xlsb`,
  clusterfuzz POIHSSFFuzzer xls): `internal` means a wax bug by contract —
  fix the crash/panic path to a proper open or a structured error. These
  are mandatory fixes, not adjudicable.
- **66 `bad_zip`**: several classes are already adjudicated (empty files,
  entity bombs, truncated rels, checksum failures). Sort the remainder:
  files SheetJS opens that are *not* structurally corrupt are reader bugs
  (e.g. `nonstandard_workbook_name.xlsx` smells like a real workbook-part
  discovery miss — POI `58616.xlsx`, `60825.xlsx` need individual looks).
  Fix those; adjudicate the genuinely corrupt with per-file evidence.
- **16 `bomb`**: verify each is a genuinely hostile/declared-extent file
  (51535.xls is already adjudicated); adjudicate the set, or flag any
  false positive to W5A (they own `safety.rs` — do not edit it yourself).
- **3 `unsupported` xlsb**: understand why calamine refuses them; fix if
  cheap, else document with evidence.
- **`###0.00;-###0.00` display: 1.19% match over 14,768 cells** — one
  format code, near-total failure: almost certainly a single wax-fmt bug.
  Find it, fix it, add format tests.
- **`[h]`/`[hh]`/`h` elapsed-hour codes: ~54% match over ~12,5k cells
  each** — partially adjudicated for xlsb durations; split what is a real
  wax-fmt defect from what extends the existing duration adjudication.
- **`wax:missing / SheetJS:s` (23,268 cells, mostly legacy xls)** — wax
  drops strings SheetJS sees. Likely continue-record / string-table gaps
  in calamine or our extraction; diagnose, fix what is ours, adjudicate or
  document what is upstream (with a concrete upstream pointer).
- **`wax:n / SheetJS:d` (78,005) and `wax:d / SheetJS:d` (112,483)** —
  large chunks are covered by the existing xlsb duration/millisecond
  adjudications; extend those entries with the additional file classes
  (LONumbers, test_datetime, etc.) only where the same reasoning genuinely
  holds; investigate the rest (POI charts xlsx, 34775.xls…).

You will not fix everything; you must *account* for everything. Ranked
honesty beats heroics.

## Tests

Every reader/fmt fix gets a unit test (or a corpus-fixture-driven test)
pinning the exact case. Refresh the harness numbers as you go
(`harness/run.sh`, no `--soffice` needed) — your seal reports the
scoreboard delta: opened %, value match %, display match %.

## Boundaries (do not touch)

Your lane: `crates/wax-read/**` (except `safety.rs` + its lib.rs hooks —
W5A's), `crates/wax-fmt/**`, `harness/adjudications.md` (append-only),
`docs/w5-triage.md`. Not `harness/wax-harness/**` code (W5D) — if triage
reporting itself needs a change, buz the coordinator. Not
`crates/wax-write/**` (W5D), `.github/**`/`scripts/**` (W5C),
`fuzz/**` (W5A).

## Definition of done

- `docs/w5-triage.md` accounts for every current triage bucket with
  outcome + evidence.
- All three `internal` failures fixed.
- Measurable scoreboard delta reported; zero regressions on previously
  passing files.
- `scripts/check.sh` fully green in your worktree.
- Commit messages end with your bee name.

## Sealing

`hive seal <your-bee-name>` (implementation seal) with status, summary,
deliverables, exact test counts, scoreboard delta, ledger totals
(fixed / adjudicated / limitation counts), and deviations. Then
`hive buz send CL.7c63 --sender <your-bee-name> --tier queue -p "sealed"`.
Blockers → buz immediately; never a silent stall.
