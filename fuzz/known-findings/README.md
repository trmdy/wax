# Known open fuzz findings

Inputs here reproduce a defect that wax does **not** yet fully contain.
They are deliberately kept out of `fuzz/corpus/` and `fuzz/artifacts/` so
that `scripts/check.sh`'s deterministic replay stays a regression gate for
*fixed* findings. Every entry must name the defect, the current behavior,
and the intended fix.

**Status (W5, 2026-07-29): one open finding, below.** It is an upstream
defect with **no reachable effect in the shipped release binary** — see the
measured evidence — and it is declared in the W5 seal rather than counted
as a clean burn.

## xlsx_reader / calamine column-accumulator overflow

- **Input:** `xlsx_reader/calamine-column-accumulator-overflow.xlsx`
  (recovered from a 1800 s burn, 2026-07-29; libFuzzer
  `crash-f8682aa9e142ffee57dc4ad26ebc1506700717a8`).
- **Defect:** `calamine 0.36.1`'s `get_row_and_optional_column`
  (`src/xlsx/mod.rs:2838`) accumulates a reference's column with an
  unguarded `col = col * 26 + …`. Seven or more letters overflow `u32`:
  a panic under the overflow checks the fuzz targets build with, and a
  **silent wrap** in release, producing a column index unrelated to the
  stored reference.
- **Current wax behavior — release (what ships) is unaffected.** Measured
  on this artifact and on a purpose-built overflowing workbook:
  the artifact returns a structured `bad_zip` in ~2 ms at ~3 MB peak RSS;
  a crafted `<dimension ref="A1:AAAAAAA1048576">` is rejected by the rail
  below; and when a wrapped index does reach the reader it is inert,
  because calamine reads xlsx cells rather than trusting the dimension
  (verified: opens at 1.8 MB, no allocation growth). There is no
  memory-safety or resource hazard in the shipped binary — this is a
  debug/ASAN-build panic.
- **Partial containment already in place:** `check_cell_reference_attributes`
  (`crates/wax-read/src/safety.rs`) rejects references with more than
  three column letters (Excel's last column is `XFD`) on the attributes
  calamine feeds that parser — `@r` on `<c>`/`<row>`, `@ref` on
  `<dimension>`/`<mergeCell>`/`<autoFilter>`/`<hyperlink>` — matched by
  *local* name so a namespace prefix cannot bypass it.
- **Why the rail does not close it — the structural finding.** This
  artifact's sheet part is **invisible to preflight**: the `zip` crate
  cannot enumerate that member (Python's `zipfile` also refuses it:
  `Bad magic number for file header`) while calamine's own zip reader
  parses it happily. So preflight validated a different set of parts than
  the reader consumed. No XML-layer rail can fix that; the two zip
  parsers disagree about what the container holds.
  *This applies to every XML rail wax has, not just this one — it is the
  most useful thing this finding taught us.*
- **Intended fix (post-v0.1.0), in preference order:** (a) upstream a
  `checked_mul`/letter-count guard to calamine — smallest, fixes it for
  everyone; (b) reject packages whose entry set differs between the `zip`
  crate and a second minimal reader, closing the whole
  preflight-blindness class rather than this one symptom; (c) vendor a
  patched calamine. A broader "any `ref` attribute" rail was tried and
  rejected: it also matches XSD `<xs:element ref="…">` in the custom-XML
  parts real workbooks carry and cost **14 corpus opens**.

Keep this file as the ledger format for anything future burns turn up.

## Closed

### calamine unbounded growth on a hostile legacy XLS (closed 2026-07-29)

- **Input:** now a committed regression seed,
  `fuzz/corpus/legacy_xls_reader/calamine-observed-extent-bomb.xls`
  (5,640 bytes; recovered from a 5-minute burn, 2026-07-28). The
  byte-identical `oom-bc197d861c-original-artifact.xls` was the same
  finding and is gone with it.
- **Defect as originally diagnosed (W3C):** a corrupt CFB sector chain
  driving unbounded `Vec` growth inside `calamine 0.36.1`'s own CFB
  reader. That diagnosis was **wrong**. Instrumenting the allocator (a
  global-allocator watchdog aborting on any single allocation over 1 GiB)
  put the 137,438,953,472-byte allocation in
  `calamine::Range::from_sparse`, reached from `Xls::new_with_options` —
  the CFB layer never misbehaved.
- **Actual mechanism:** `Range::from_sparse` densifies the span *observed*
  in the collected cell records: it takes min/max row and column over the
  cells and allocates `rows * cols` defaults. No DIMENSIONS record is
  involved, so wax's existing declared-extent rail never saw it. The
  artifact holds cell records at opposite corners of the BIFF grid, so the
  span is 65,536 x 65,536 = 2^32 cells.
- **Fix (W5A):** the BIFF preflight now tracks the row/column span of every
  cell-bearing record it walks (Blank, Number, Label, BoolErr, RString,
  RK, LabelSst, Formula, MulRk, MulBlank) and applies the same
  `max_declared_cells` cap the DIMENSIONS rail uses — `ObservedExtent` in
  `crates/wax-read/src/safety.rs`. The DIFAT cycle guard and the CFB
  directory/mini-FAT chain walks added while chasing the original
  diagnosis were kept: they bound a real class (calamine reads the
  directory chain with `usize::MAX` as its length bound), they are cheap,
  and they cost no corpus opens.
- **Result:** `wax dump` on the artifact returns a structured
  `{"ok":false,"code":"bomb"}` in **0.26 s at 1.2 MB peak RSS**, versus
  33 s and 24.4 GiB before. The input is a committed corpus seed, so
  `scripts/check.sh`'s deterministic replay is its regression gate, and
  `legacy_biff_rejects_observed_extent_bomb_without_a_dimensions_record`
  pins the exact error.
