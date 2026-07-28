# Review — W4A: writer core (`agent/wax-w4a-writer`, c8e9dd6)

Reviewer: CL.6cbf (coordinator, agent-assisted deep review with probe
tests). Verdict: **merge after fast-follow fixes** (sent to the shard
2026-07-28; see the fix log at the bottom).

## Scope check

Diff touches `crates/wax-write/{Cargo.toml,src/lib.rs}` (+1,841/−24), the
workspace `Cargo.toml` (one line: `rust_xlsxwriter = "=0.96.0"`, pinned as
the brief required) and `Cargo.lock`. In-lane per the brief. Frozen §2
signatures byte-identical to the stub on `main`; `ExportOutcome`/
`WriteError` unchanged; proto error spellings exact. `cargo fmt`, clippy
(`--all-targets`) and the full workspace suite re-verified green in a
detached review worktree; 14 unit tests pass, the ignored 5-fixture corpus
round-trip reproduces at 30/30 exact (t, v) once pointed at the corpus
overlay.

## Design

`write_xlsx` drives `rust_xlsxwriter` off `scan_sheet` with interned
`Format`s (one per distinct (fmt, style) pair, asserted via `cellXfs`
count in the output XML), blank-first merges overwritten by typed anchors,
and per-row cancellation checkpoints. The clever part is the post-save
**formula normalization pass**: rust_xlsxwriter rewrites formula text
(`_xlfn.` prefixes) and types every cached result as `str`, so the writer
re-zips the saved file and patches each formula `<c>`: exact model formula
text restored, cached result re-typed (`b`/`e`/`str`, untyped numeric,
date serial), with a formulas-seen count invariant that fails loudly if a
patch went unapplied. Probed: XML-special chars in formulas and cached
strings escape correctly; `XLOOKUP` text survives byte-exact; the
`sheetN.xml` index mapping is correct past sheet 10; duplicate-coordinate
last-wins interacts correctly with the patch list.

Atomicity holds on every path constructed in review: temp sibling +
`persist`, both temps drop-deleted on all error/cancel returns, fsync
before rename, existing outputs never clobbered on failure (tested).

`write_csv` is byte-parity with the W3 serve path: `write_csv_field` is a
verbatim port and the row/extent semantics are equivalent because the
store drops out-of-extent cells at ingestion and windows are
last-duplicate-wins. The serve-pinned test cases are replicated and
extended (RFC 4180 quoting, shortest-round-trip numbers, TRUE/FALSE,
error text, `cols == 0`).

## Findings

1. **major — degenerate/overlapping merges kill the whole export**
   (lib.rs:147). wax-read collapses `A1:A1` to `"A1"`; `merge_range`
   rejects single-cell and overlapping ranges ⇒ `internal`, no output, a
   round-trip files-clean loss. Scanned all 1,244 corpus xlsx: zero
   occurrences — but xls/xlsb/ods merges flow through calamine too.
   Fix (per §2 amendment): skip unwritable merges + loud `dropped` entry.
2. Unrepresentable sheet names (>31 chars, `[]:*?/\` — legal in ODS
   sources) fail the export (lib.rs:115). Same class as 1; sanitize +
   dropped per §2 amendment.
3. `ExcelDateTime::parse_from_str` silently mis-parses tz-offset ISO
   strings (`…T13:45:09+02:00` → 13:45:00, probed); wax-read strips only
   trailing `Z`, so strict-OOXML `t="d"`/ODS offsets reach it. The one
   silent-wrong-data path found; pre-validate and reject loudly.
4. A zero-sheet store exports successfully with an invented blank
   "Sheet1" (probed) — "never invents fidelity"; now `bad_request` per §2
   amendment.
5. Width outside 0..=255 hard-fails (lib.rs:128); `ColInfo` documents no
   bound and the OOXML schema is unbounded — clamp + dropped per §2
   amendment (bound coordinated with W4D).
6. minor — exports inherit `NamedTempFile`'s 0600 mode vs serve's
   0644-umask today — wire-visible once W4C delegates.
7. nit — `write_csv` omits the truncation `dropped` entry (serve-parity,
   so spec-compliant; loudness amendment candidate for later).
8. nit — ignored corpus test's `../../../..` path only resolves under
   `.worktrees/<shard>/` nesting (lib.rs:1783).
9. nit — re-zip: `large_file` off-by-one at exactly 0xFFFFFFFF and keyed
   to input entry size; untouched entries re-compressed where
   `raw_copy_file` would do. Theoretical at corpus scale.
10. observation — memory: full workbook in RAM + per-formula patch clones
    + second zip pass; `save()`/zip-finish uncheckpointed (row-granularity
    cancel contract still met). Watch peak RSS on formula-heavy giants in
    the W4B round-trip run.

## Deviations from seal

None found. Test counts, pin (`rust_xlsxwriter =0.96.0`), and round-trip
claims reproduced; the corpus self-test needed a path fix to reproduce
from outside the shard worktree (finding 8).

## Fix log

- 2026-07-28: coordinator amended `docs/w4-contracts.md` §2 (loud
  degradation for unwritable merges/sheet names/widths; tz-offset dates
  rejected; zero-sheet store `bad_request`) and sent findings 1–5 + 8 to
  the shard as required pre-merge fixes, plus a permissions-convention
  addendum (finding 6).
- 2026-07-28 (fixes sealed, commit d8aa52b): all six fixes landed with
  dedicated tests — unrepresentable merges skipped with a deduped
  `dropped` entry; sheet names sanitized + kept unique; tz-offset ISO
  dates rejected loudly before `ExcelDateTime` parsing; zero-sheet store
  → `bad_request` "empty workbook"; widths clamped to 0..=255 with a
  `dropped` entry; outputs chmod'd to `File::create`-equivalent mode via
  a race-free same-directory umask probe (0644 verified under umask
  022). wax-write: 20 tests + 1 ignored corpus round-trip (30/30 exact).
  Merged to main as 8b1fc8e.
