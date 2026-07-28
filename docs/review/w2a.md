# Review — W2A calamine reader + normalization (agent/wax-w2a-reader, 0410261)

Reviewer: CL.988e (coordinator). Verdict: **merge** (merged, validated by
full-corpus differential run).

## Scope check

One commit touching `crates/wax-read/**`, `crates/wax-cli/**`, workspace
`Cargo.toml`/`Cargo.lock` (adds calamine 0.36.1), and a two-derive additive
change in `wax-core` (`Copy`/`Eq` on `CellType`) — in-lane. `wax-fmt` is
consumed strictly through the frozen `render` signature, as required.

## What it adds

- `CalamineReader` behind the existing `Reader` trait; CLI switched to it,
  `StubReader` retained. Panic containment via `catch_unwind` → `internal`
  error documents (a crashing file is a data point, not a harness crash).
- xlsx/xlsm via calamine's streaming cells reader with formula metadata and
  shared-formula expansion; xlsb/xls/ods best-effort via range + formula
  ranges. Timeout re-checked every 4,096 cells and between sheets.
- Per-cell number-format codes for OOXML from a supplemental styles reader
  (cellXfs → numFmtId → code, builtin 0–49 table) since calamine doesn't
  expose them; degradation to a warning + null fmt when styles are
  unreadable. xls/xlsb/ods fmt codes and (xls/ods) merges stay null/empty —
  honest, documented in warnings.
- Contract-faithful normalization: dates only when calamine is confident
  (`t:"d"`, ISO value, raw serial kept for display rendering); error cells
  carry canonical `#…!` text; `fmt` null for General; display strings
  exclusively from `wax_fmt::render`; loud symmetric truncation.

## Antagonist findings

1. `Data::Float(NaN/inf)` maps to `t:"n", v:null` — defensible (no JSON
   representation), and empty-with-formula cells are kept per contract.
2. ODS/xls formula normalization strips `of:=`/`oooc:=`/`=` prefixes —
   whitespace inside formulas is preserved, matching the comparator's
   normalization. OK.
3. Known risk (seal, confirmed in full run): one legacy xls drives peak RSS
   to ~1.0 GiB. That is the W3 windowed-store/safety-rails problem; wax p50
   RSS is 2.39 MiB vs SheetJS 88.97 MiB.
4. Formula fidelity is 18.80% corpus-wide because calamine does not expose
   formula text for most legacy xls/xlsb — on xlsx the shard's sample
   measured 99.96%. Not a blocker for the W2 gate (which is opens+values);
   worth a W3+ decision on whether legacy formula text matters.
5. xlsb cell-value match (73.89%) lags xlsx (99.57%) — best-effort lane;
   candidates for adjudication/triage in W3.

## Verification (coordinator, merged main)

- `scripts/check.sh --fast` fully green; release build clean.
- Full 2,048-file `harness/run.sh` after merge:
  opens 29.15% → **95.90%**, cell-value match 64.63% → **92.12%**,
  cached-result fidelity 59.01% → 52.73% (denominator grew 6×: 455k formula
  cells now visible vs 73k), p95 parse 2 ms vs SheetJS 65 ms.
- **W2 xlsx gate: opens 94.20% (≥90% ✓), cell-value match 99.57% (≥95% ✓).**
- Display coverage still 0% by design — wax-fmt stub until W2B merges.
