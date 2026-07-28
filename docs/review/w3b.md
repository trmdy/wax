# Review — W3B: columnar windowed store (`agent/wax-w3b-store`, f701c08)

Reviewer: CL.d73 (coordinator). Verdict: **merge**.

## Scope check

Diff touches exactly one file: `crates/wax-store/src/lib.rs` (+889/−103).
Frozen five-call API byte-identical; no wax-core changes (the brief allowed
additive ones — none were needed). All pre-existing store tests kept
passing unchanged, which pins the clipping/merge semantics as required.
`scripts/check.sh` and the ignored 5M bench re-verified green by the
coordinator in the shard worktree.

## Design

Structure-of-arrays per sheet: contiguous `cols`/`types`/`value_tags`/
`value_refs` columns; `f64` column for numbers; bitset for booleans;
workbook-wide string table with `u32` refs for text/display/formula/fmt
(one interner across sheets, so repeated fmt codes and display strings —
the dominant repetition — cost 4 bytes per reference). Sparse `row_index`
holds only populated rows, with implicit range ends (next row's start) —
8 bytes per populated row. `window()` is `partition_point` on the row
index, then `partition_point` per row for the column start: O(log rows +
output). Duplicate-coordinate last-wins is preserved via stable sort +
overwrite order — verified against the naive behavior by a randomized
brute-force oracle test.

Additive API: `WorkbookStoreBuilder` with `add_ordered_sheet` streaming
ingestion (no full-Document retention; ordering violations are a typed
`CellOrderError`), exactly the incremental path the contract invited.

## Measured targets (seal claims re-verified)

- 5M-cell numeric sheet: `approx_bytes` 151,600,363 (144.6 MiB) — under
  the ≤200 MB target; process RSS 192 MB; documented 30 B/cell dense
  layout in the crate docs.
- 64×24 window p95: **21 µs** over 1,000 samples — three orders under the
  10 ms target.
- Mixed-text interning: 300 KB vs 1.05 MB naive string payload (3.5×),
  payload stored once (asserted, not just printed).
- 1 GiB outlier: profiled to calamine's dense legacy-XLS range
  materialization (41,984-byte file, 65,536×256 declared extent,
  1,076,297,728 B peak with zero cells emitted) — evidence documented in
  the crate's memory-bound docs and routed to W3E's extent-bomb guard
  (contract §5 amendment, already in place before this seal landed).

## Findings (non-blocking)

1. `approx_bytes` excludes the transient `StringInterner` HashMap during
   build (retained-store accounting only) — correct per docs, worth
   remembering when reasoning about peak-during-ingest.
2. `intern` allocates the owned key on first sight only; lookups borrow.
   Fine. A `raw_entry`-style zero-alloc miss path would shave ingest
   allocations — not worth it now.
3. Interner panics (`expect`) past `u32::MAX` distinct strings /
   per-sheet cell count past `u32::MAX` — unreachable behind serve's
   `maxCells` cap (5M default, u32-checked at parse); acceptable.

## Deviations from seal

None found. Seal numbers reproduced within noise.
