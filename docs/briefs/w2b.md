# Shard W2B — ECMA-376 number-format interpreter

You are shard **W2B** of the wax v1 mission, wave 2. Coordinator: bee
**CL.988e**.

**Required reading before any code:** `MISSION.md`, `docs/w1-contracts.md`
§1, `docs/w2-contracts.md` (§2 is your spec — including the operator
amendment that reorders your work). You work in your git worktree only
(`.worktrees/w2b`), on branch `agent/wax-w2b-formats`. Never touch `main`.
No remote; commit locally, the coordinator merges.

## Phase 1 — mine the corpus FIRST (operator amendment, binding)

Before any interpreter code:

1. `harness/formats/mine.mjs` — walk `corpus/manifest.jsonl`, run the
   SheetJS oracle (`node harness/oracle/run.js <file>`) per file, aggregate
   every distinct per-cell `fmt` code. Parallelize politely (~8 jobs);
   full corpus is 2,048 files. Skip files whose local payload is missing.
2. **Commit `harness/formats/corpus-formats.json`**: one entry per distinct
   code — `{ code, cellCount, fileCount }` — sorted by cellCount desc,
   plus totals (files scanned, cells seen, distinct codes) and a generated
   timestamp. Private corpus entries: counts yes, paths/names no.
3. Commit this before starting Phase 2 and buz the coordinator the
   headline numbers (distinct codes, top 10 by cell count).

This ranking *is* your implementation order: the ≥95% display-coverage swap
gate is measured cell-frequency-weighted against these corpus formats.

## Phase 2 — the interpreter (`crates/wax-fmt`)

Implement behind the frozen signature (do not change it):

```rust
pub fn render(code: &str, value: FmtValue<'_>, epoch_1904: bool) -> Option<String>
```

Scope, in corpus-frequency order (expected to include): `General` (Excel's
11-significant-digit rules, integer/decimal/E-notation switchover);
fixed/thousands decimals (`0`, `0.00`, `#,##0.00`, …); percentages;
currency/literal text and escapes (`"kr"`, `\ `, `_`, `*` fill — fill may
degrade to no-op, note it); date/time codes (yy/yyyy, m/mm/mmm/mmmm/mmmmm,
d/dd/ddd/dddd, h/hh, m as minutes contextually, s/ss, AM/PM, elapsed
`[h]`/`[mm]`/`[ss]`, fractional seconds) on both epochs with Excel's 1900
leap-year bug compatibility; sections (`pos;neg;zero;text`) and section
conditions (`[<100]`); color tags (`[Red]` — strip, they don't affect the
string); scientific; fractions (`# ?/?`, `# ??/??`); text section `@`;
builtin numFmtId semantics where codes arrive already-resolved as strings.

Rules:

- `Some` only when confident it's what Excel shows; else `None` (`d: null`).
  **Never guess.** A wrong string is worse than a null.
- en-US separators in v1 (matches Excel default + the SheetJS oracle we
  diff against). No locale plumbing in the public API.
- Match Excel semantics, not SheetJS's — where SheetJS's `w` is wrong and
  you can prove it (ECMA-376 §18.8.30/31 or real-Excel behavior), document
  the case in your seal for adjudication; don't copy the bug.

## Tests

- Unit tests per feature area (dates on both epochs, sections, fractions,
  General edge cases like 0.1 float noise, big/small switchovers).
- A coverage test driven by your committed `corpus-formats.json`: assert
  the cell-frequency-weighted supported share, print it honestly. Target
  ≥95%; if short at seal time, the seal says exactly which codes remain.
- Property-ish sanity: `render` never panics on arbitrary code strings
  (throw the corpus's distinct codes plus junk at it).

## Boundaries (do not touch)

`crates/wax-read/**`, `crates/wax-cli/**`, `crates/wax-core/**`,
`harness/wax-harness/**`, `harness/run.sh`, `harness/oracle/**` (you *run*
the oracle, you don't edit it), `corpus/**`, `scripts/**`, `.github/**`,
`docs/**`, `ASSIGNMENTS.json`. You own `crates/wax-fmt/**` and
`harness/formats/**` only. Public API additions to wax-fmt are fine;
`render`'s signature is frozen — buz the coordinator if it blocks you.

## Definition of done

- `scripts/check.sh` fully green.
- `corpus-formats.json` committed (Phase 1) + interpreter with
  frequency-weighted coverage measured and reported.
- All work committed on your branch; commit messages end with your bee name.

## Sealing

`hive seal <your-bee-name> --from seal.json` with status, summary,
deliverables, exact test counts, the measured coverage number, remaining
unsupported codes by frequency, and deviations. Then
`hive buz send CL.988e --sender <your-bee-name> --tier queue -p "sealed"`.
Blockers → buz immediately; never a silent stall.
