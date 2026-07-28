# Review — W3E: reader carry-overs (`agent/wax-w3e-reader`, c73e072)

Reviewer: CL.d73 (coordinator). Verdict: **merge**.

## Scope check

Diff: `crates/wax-read/**` (calamine_reader.rs wiring + new
`xls_styles.rs` / `xlsb_styles.rs` modules), `Cargo.{toml,lock}`, six
append-only `harness/adjudications.md` rows — exactly the assigned lane,
no collision with W3C's `safety.rs`. New deps: `cfb 0.14` (OLE2 access,
flagged in the seal per brief), plus direct declarations of
already-transitive `codepage`/`encoding_rs`. `scripts/check.sh` re-verified
green by the coordinator in the shard worktree.

## What's there

- **xls**: own BIFF record walk over the OLE `Workbook` stream (via cfb)
  — `FORMAT` (0x041E, BIFF5 bytes + BIFF8 unicode variants, CODEPAGE-aware
  via `codepage`/`encoding_rs`), `XF` (0x00E0) numFmtId table,
  `BOUNDSHEET` offsets, then per-sheet cell-record scan (Formula, Blank,
  Label, BoolErr, Number, RString, RK, LabelSst, MulRk with run
  expansion) → per-cell `(row,col) → fmt code` map. Calamine untouched
  (no fork), supplement degrades to a warning when the container resists.
- **xlsb**: `xl/styles.bin` `BrtFmt`/`BrtXF` parsing plus worksheet
  style-ref extraction, mirroring the xlsx styles path; also recovers
  `BrtFmlaError` cached error values calamine drops.
- Display strings still exclusively via `wax_fmt::render`; recovered
  codes feed conservative date typing (elapsed-duration formats stay
  numeric — see adjudications).
- **Adjudications**: six evidence-backed rows, including two systematic
  SheetJS bugs that account for ~184K xlsb value mismatches (fractional
  seconds truncated; `[h]`-family durations converted to pseudo-dates),
  XML-bomb fixtures SheetJS wrongly opens, and an amplification fixture
  that SIGABRTs the oracle at V8's heap limit while wax fails structured.
- **37 xlsx open-failure classification**: 4 encrypted, 16 corrupt/empty
  zips, 5 mislabeled payloads, 5 XML-security fixtures, 3 nonstandard
  OOXML, 3 malformed relationships, 1 empty archive — i.e. the remaining
  xlsx gap is dominated by files that *should* fail; no cheap wins left
  except one nonstandard part name calamine hard-codes away.

## Measured (seal claims, re-verified by post-merge corpus run)

Whole-corpus display match 76.15% → ~97.0%; xls 74.17% → ~99.1%; xlsb
57.50% → ~93.1%; cached-result fidelity +1.1pt; opens unchanged. The
biggest single scoreboard movement of the mission so far, from the
capacity lane.

## Findings (non-blocking)

1. The extent-bomb guard (contract §5 scope addition) is **not** in this
   branch — the addition was buzzed mid-flight and the shard sealed
   without it. Transferred to W3C (safety lane) before their seal;
   tracked there.
2. `parse_sheet_styles` trusts BOUNDSHEET offsets into the stream (bounds
   checked, degrades to empty map) — fine; hostile-input robustness for
   this path is covered by W3C's fuzz targets going forward.
3. xlsb merged regions remain best-effort (explicit warning) — carried
   from W2A, unchanged.

## Deviations from seal

None found. The "nonstandard workbook-part name not rewritten" deviation
is documented and reasonable (would require forking calamine).
