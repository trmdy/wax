# Shard W4A — writer core (`wax-write`)

You are shard **W4A** of the wax v1 mission, wave 4. Coordinator: bee
**CL.6cbf**.

**Required reading before any code:** `MISSION.md`, `docs/w4-contracts.md`
(§2 is your spec; §1 tells you what is already scaffolded on `main`),
`docs/w1-contracts.md` §1 (dump/cell semantics). You work in your git
worktree only (`.worktrees/w4a`), on branch `agent/wax-w4a-writer`. Never
touch `main`; never `git push`. Commit locally, the coordinator merges.

## The job

Implement `wax_write::write_xlsx` and `wax_write::write_csv` behind the
**frozen** signatures already on `main` (rustdoc there + contract §2 are
normative). You own the `rust_xlsxwriter` dependency: pin a current
version, note it in the seal.

- `write_xlsx`: whole workbook — values by type (incl. ISO-date → Excel
  datetime), formula text + cached results, deduped `Format`s for
  fmt/number-format + basic styles, merges via `sheet_merges`, column
  widths via `sheet_col_infos`, loud deduped `dropped`, cancellation
  checkpoints, and **no partial file on any error path** (temp + rename or
  delete-on-error).
- `write_csv`: port the W3 serve CSV semantics exactly (the serve tests in
  `crates/wax-cli/src/serve.rs` pin them: RFC 4180, CRLF, display-else-raw,
  shortest-round-trip numbers, TRUE/FALSE, error text as-is). W4C swaps
  serve to delegate afterwards — your copy is the canonical one; don't
  edit serve yourself.
- Iterate cells with `WorkbookStore::scan_sheet`; styles via
  `store.styles()`. The store is your only input.

The store's `d`/`fmt`/`s`/`col_infos` may be absent for many files (W4D
lands extraction in parallel) — the writer must be correct for both empty
and populated style surfaces.

## Definition of correct (what W4B will measure)

Round-trip: `wax dump` of your exported file must reproduce the source
model's `(t, v)` per cell at ≥99% corpus-wide, and `d` as high as fmt
preservation allows. Concretely: numbers survive as f64 exactly
(shortest-round-trip is for CSV only — xlsx stores the number), dates
re-read as the same ISO instant, formulas re-read with the same text and
cached result, merges re-read identically. Build yourself a small
round-trip test harness in the crate's tests (write → read back with
`wax_read` — dev-dependency is fine — → compare) and run it over a handful
of self-built models plus 3–5 small corpus xlsx fixtures.

## Tests

Unit tests for: each cell type mapping, date serial round-trip (1900
epoch, times, ms precision), formula + cached result, format dedup (one
Format per distinct (fmt, style) pair — assert via output inspection or
count), merges incl. blank-anchor, col widths, dropped-list contents
(error cells, truncated model), cancellation (no output file left), CSV
semantics parity (port the serve test cases). Keep the existing stub test
file's intent: structured errors, proto ErrorCode spellings.

## Boundaries (do not touch)

Everything outside `crates/wax-write/**` + the workspace `Cargo.toml`/
`Cargo.lock` line for your dependency. Especially not `wax-cli` (W4C),
`harness/**` (W4B), `wax-read`/`wax-core`/`wax-store` (W4D/coordinator).
If the frozen API can't express something, buz the coordinator with the
proposed amendment — do not change it unilaterally.

## Definition of done

- `scripts/check.sh` fully green in your worktree.
- Both writers real; round-trip self-test numbers in the seal; dropped
  reporting demonstrably loud.
- Commit messages end with your bee name.

## Sealing

`hive seal <your-bee-name>` (implementation seal) with status, summary,
deliverables, exact test counts, the rust_xlsxwriter version pin, known
limitations, and deviations. Then
`hive buz send CL.6cbf --sender <your-bee-name> --tier queue -p "sealed"`.
Blockers → buz immediately; never a silent stall.
