# Shard W2A — calamine reader + normalization

You are shard **W2A** of the wax v1 mission, wave 2. Coordinator: bee
**CL.988e**.

**Required reading before any code:** `MISSION.md`, `docs/w1-contracts.md`
(§1 dump schema and §2 CLI are unchanged and binding), `docs/w2-contracts.md`
(§0 ownership, §1 is your spec, §2 is the fmt API you call). You work in
your git worktree only (`.worktrees/w2a`), on branch
`agent/wax-w2a-reader`. Never touch `main`. No remote; commit locally, the
coordinator merges.

## Deliverables

1. **`CalamineReader`** in `crates/wax-read`, implementing the existing
   `Reader` trait, replacing `StubReader` as the CLI's reader (keep the
   stub in-tree). Add `calamine` (latest stable, MIT) via the workspace
   `[workspace.dependencies]`.
2. **Containers:** xlsx/xlsm (gate target), plus xlsb, legacy xls, ods as
   best-effort. Any calamine failure → `ok:false` document with a sensible
   error code; never a panic, never a non-zero exit with output lost.
3. **Normalization** per w2-contracts §1: shared-string text, booleans,
   error cells; 1900/1904 epoch resolution with `t:"d"`/ISO-8601 values
   (serial kept for display rendering); formula text + cached results;
   merges; per-cell `fmt` codes (parse the style part yourself if
   calamine's API doesn't expose codes for a container — builtin numFmtId
   0–49 table included; do not fork calamine).
4. **Display strings:** every cell's `d` comes from
   `wax_fmt::render(code_or_general, raw_value, epoch_1904)`. The crate is
   a stub on `main` (returns `None` → `d: null`) — code against the frozen
   signature; another shard fills it in. Never hand-roll display strings.
5. **Caps:** `--max-cells` truncation (loud, symmetric with W1 semantics)
   and `--timeout-ms` honored at least between sheets.
6. **Tests:** fixtures under `crates/wax-read/tests/fixtures/` covering:
   shared strings, 1904-epoch dates, formulas with and without cached
   results, merges, fmt-code extraction (custom + builtin ids), xls and
   ods smoke files, truncation, corrupt zip. Real tiny files are fine
   (generate with a script or commit hand-built ones, note provenance in
   the test file).

## Measurement (your definition of success)

The W2 gate on your work: wax opens **≥90% of xlsx corpus files** with
**cell-value fidelity ≥95%** on opened files (scoreboard vs the SheetJS
oracle). While iterating, run the harness yourself:
`harness/run.sh --limit 300` (or full) after `cargo build --release`.
Chase open-failures and value mismatches by frequency. Where you believe
wax is right and SheetJS is wrong, record evidence in your seal — the
coordinator adjudicates into `harness/adjudications.md`.

## Boundaries (do not touch)

`crates/wax-fmt/**` (frozen stub — buz the coordinator if the signature
blocks you), `harness/**`, `corpus/**`, `scripts/**`, `.github/**`,
`docs/**`, `ASSIGNMENTS.json`. `crates/wax-core` extensions must be
additive and keep dump JSON at `schema: 1`.

## Definition of done

- `scripts/check.sh` fully green (fmt, clippy `-D warnings`, all tests).
- `wax dump --json` on a real corpus xlsx emits contract-valid JSON with
  values, dates, formulas, merges, fmt codes populated.
- Harness run shows xlsx opens ≥90% and value fidelity ≥95% — or your seal
  documents exactly what's short and why.
- All work committed on your branch; commit messages end with your bee name.

## Sealing

When done (or blocked), seal:
`hive seal <your-bee-name> --from seal.json` with status, summary,
deliverables, exact test counts, harness numbers you measured, and
deviations. Then buz the coordinator:
`hive buz send CL.988e --sender <your-bee-name> --tier queue -p "sealed"`.
Blockers surface as buz immediately — never a silent stall.
