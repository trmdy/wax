# Known open fuzz findings

Inputs here reproduce a defect that wax does **not** yet fully contain.
They are deliberately kept out of `fuzz/corpus/` and `fuzz/artifacts/` so
that `scripts/check.sh`'s deterministic replay stays a regression gate for
*fixed* findings. Every entry must name the defect, the current behavior,
and the intended fix.

**Status (W5, 2026-07-30): two open findings, both upstream in
`calamine 0.36.1`, both contained by wax.** Neither can crash the shipped
binary: the xlsx family is inert in release (measured), and the legacy
family is caught by the reader's panic boundary and returned as a
structured error. Extended burns closed four other findings outright.
Both are declared in the W5 seal rather than counted as clean burns.

## xlsx_reader / unguarded arithmetic in calamine's `get_dimension`

- **Inputs:** two artifacts, both from 1800 s burns (2026-07-29/30):
  `xlsx_reader/calamine-column-accumulator-overflow.xlsx` and
  `xlsx_reader/calamine-dimension-subtract-underflow.xlsx`.
- **Defect:** `calamine 0.36.1`'s reference parsing does unguarded
  integer arithmetic on attacker-controlled A1 refs. Two sites found so
  far, and the family is what matters rather than either instance:
  - `get_row_and_optional_column` (`src/xlsx/mod.rs:2838`) accumulates a
    column with `col = col * 26 + …`; seven or more letters overflow
    `u32`.
  - `get_dimension` (`src/xlsx/mod.rs:2793`) computes
    `parts[1].0 - parts[0].0` without ordering the pair, so a reversed
    range (`B9:A1`) underflows.
  Both panic under the overflow checks the fuzz targets build with, and
  **wrap silently** in release.
- **Current wax behavior — release (what ships) is unaffected.** Measured
  on both artifacts and on three purpose-built hostile workbooks
  (overflowing column run, reversed rows `B9:A1`, reversed columns
  `Z1:A1` — the last two open inertly at 1.4-3.3 MB):
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
- **Intended fix (post-v0.1.0), in preference order:** (a) upstream
  `checked_mul` / `checked_sub` (or an ordered pair) guards to calamine's
  reference parsing — smallest, fixes the whole family for everyone; (b) reject packages whose entry set differs between the `zip`
  crate and a second minimal reader, closing the whole
  preflight-blindness class rather than this one symptom; (c) vendor a
  patched calamine. A broader "any `ref` attribute" rail was tried and
  rejected: it also matches XSD `<xs:element ref="…">` in the custom-XML
  parts real workbooks carry and cost **14 corpus opens**.

## legacy_xls_reader / unchecked record and chain parsing in calamine

- **Input:** `legacy_xls_reader/calamine-cfb-empty-fat-index.xls`.
- **Defect:** calamine's legacy path indexes attacker-controlled
  structures without bounds checks. Four instances surfaced in W5 burns;
  **three were fixed in wax's preflight** and are now committed regression
  seeds:
  - *fixed* — `parse_lbl` reads `data[3]` / `data[4..]` on a short record
    (`xls.rs:791`, index-out-of-bounds). Rail: `Lbl` (0x0018) requires its
    14-byte fixed header.
  - *fixed* — `parse_lbl` takes `data[data.len() - cce..]` with `cce` read
    from the record (`xls.rs:801`, subtract underflow). Rail: structural
    check that the declared formula length fits the record.
  - *fixed (earlier)* — `Range::from_sparse` densifying an observed
    65,536 × 65,536 span. Rail: `ObservedExtent`. See Closed, below.
  - **open** — `get_chain` evaluates `fats[sector_id as usize]` on an
    empty FAT (`cfb.rs:330`). The chain being walked is the ministream,
    whose start comes from a parsed directory entry; wax's
    `preflight_cfb_chains` validates the directory and mini-FAT chains but
    cannot reach that one without reimplementing directory parsing.
- **Current wax behavior:** contained. `wax dump` returns a structured
  `{"ok":false,"code":"internal","msg":"calamine panicked while reading
  the workbook"}` in **~0 ms at 1.3 MB** — the reader runs on an owned
  worker thread whose panic is caught and converted, so no crash reaches a
  caller. It is `internal` rather than `bad_zip`, which by wax's own
  contract reads as "wax bug"; that is the honest cost of not having a
  rail for it.
- **Why this is declared rather than chased:** each burn round surfaced
  the next unchecked access in the same dependency. Three were worth
  closing because each had a precise, corpus-safe rail in a mechanism that
  already existed. This one would require wax to reimplement CFB
  directory parsing to reach the ministream chain — a large new attack
  surface of our own to guard against a dependency's missing bounds check.
- **Intended fix (post-v0.1.0), in preference order:** (a) upstream
  bounds checks to calamine's `cfb.rs` chain walk — one `get()` instead of
  an index; (b) map caught reader panics to `bad_zip` rather than
  `internal` so hostile input is not reported as a wax defect; (c) mirror
  directory parsing in preflight (last resort).

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
