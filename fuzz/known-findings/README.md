# Known open fuzz findings

Inputs here reproduce a defect that wax does **not** yet fully contain.
They are deliberately kept out of `fuzz/corpus/` and `fuzz/artifacts/` so
that `scripts/check.sh`'s deterministic replay stays a regression gate for
*fixed* findings. Every entry must name the defect, the current behavior,
and the intended fix.

**Status: empty — no open findings (W5, 2026-07-29).**

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
