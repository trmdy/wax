# Review — W2B number-format interpreter (agent/wax-w2b-formats, a3354c2 + 1e16d68)

Reviewer: CL.988e (coordinator). Verdict: **merge** (merged, validated by
full-corpus differential run).

## Scope check

Two commits, all inside `crates/wax-fmt/**` and `harness/formats/**` —
in-lane. No runtime dependencies added (serde/serde_json dev-only, already
in the workspace). The frozen `render` signature is untouched; `is_supported`
added additively for coverage reporting.

## What it adds

- **Phase 1 (operator amendment, done first):** `harness/formats/mine.mjs`
  (oracle-driven, polite, resumable) + committed
  `corpus-formats.json`: 487 distinct codes over 909,293 explicitly
  formatted cells from all 2,048 corpus files; no private paths in output.
  Cherry-picked to main mid-shard (7514816) so W2C could join against it.
- **Phase 2:** parser (sections, conditions, colors, escapes, `_`/`*`),
  General (Excel 11-sig-digit rules), decimal/grouping/accounting/currency,
  percent, scientific, fractions, text `@`, date/time on both epochs with
  the 1900 leap-bug compatibility, elapsed `[h]`/`[mm]`/`[ss]`, fractional
  seconds. Conservative by design: anything unsupported → `None`, never a
  guess.
- Corpus-driven coverage test: **906,422/909,293 formatted cells
  (99.68%), 476/487 codes** — asserted in `cargo test`, honest print.
  Remaining 0.32% is dominated by syntactically broken trailing
  `_`/`\` codes plus `[DBNum1]` — intentionally null.
- No-panic property test over all mined codes + 4,096 junk codes across
  every value class including non-finite and extreme values.

## Antagonist findings

1. Deliberate approximations are the same ones the SheetJS baseline makes
   (`_x` → one space, `*x` fill → no-op) — verified: wax matches SheetJS
   SSF **byte-for-byte** on probe values for the top corpus codes,
   including the four-section accounting formats.
2. Bool under numeric codes renders `TRUE`/`FALSE` (Excel behavior), error
   values pass through — matches oracle handling.
3. Text with fewer than 4 sections passes through unchanged — correct per
   ECMA-376; matched the oracle in the corpus run.

## Verification (coordinator, merged main)

- `scripts/check.sh --fast` fully green post-merge (58 workspace tests
  incl. 22 wax-fmt; standalone harness 30; oracle 9).
- End-to-end sanity: `wax dump` now emits display strings through the real
  interpreter (dates render via fmt codes, e.g. `mm-dd-yy` → `01-02-04`).
- Full 2,048-file harness: display-string coverage 0% → **99.90%**;
  display-string match 76.15% overall, split by container:
  **xlsx 97.24%**, xlsm 99.57%, ods 96.01%, xls 74.17%, xlsb 57.50%.
  Probed the worst-scoring codes directly: wax render == SheetJS SSF on
  all probes — the xls/xlsb gap is **missing per-cell format codes in
  those containers** (calamine doesn't expose their style records; wax
  falls back to General), not formatter error. That extraction gap is a
  W3 candidate, owned by the reader lane, not wax-fmt.
