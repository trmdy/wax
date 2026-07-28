# Shard W3B — columnar windowed store

You are shard **W3B** of the wax v1 mission, wave 3. Coordinator: bee
**CL.d73**.

**Required reading before any code:** `MISSION.md`, `docs/w1-contracts.md`
§1, `docs/w3-contracts.md` §2 (your spec) and §1 (so you know how the
store is consumed). You work in your git worktree only (`.worktrees/w3b`),
on branch `agent/wax-w3b-store`. Never touch `main`. No remote; commit
locally, the coordinator merges.

## The job

`crates/wax-store` exists on `main` as a naive stub behind a **frozen**
five-call public API (`from_document`, `sheet_count`, `sheet_meta`,
`window`, `approx_bytes` — rustdoc in the crate is normative, and the
existing unit tests pin the clipping/merge semantics). Rewrite the
internals into the real thing:

- **Columnar representation** per sheet: cells bucketed by row or
  row-block, typed columns (type tag + f64 for numbers/dates, bitset or
  u8 for bools, u32 indexes into an interned string table for
  text/error/display/formula/fmt strings). `fmt` codes and display
  strings repeat massively across cells — interning is where the memory
  win lives. Sparse layout: empty cells cost nothing.
- **O(window) `window()`**: row index → cell range lookup; never scan the
  whole sheet. Semantics identical to the stub (effective-bounds
  clipping, `nr×nc` rows arrays, unclipped intersecting merges) — the
  existing tests must keep passing unchanged.
- **Honest `approx_bytes()`** measuring the real footprint (columns +
  string table + indexes).
- Additive API is welcome (e.g. an incremental builder ingesting one
  sheet at a time so the peak never holds Document + store fully
  doubled); the five frozen calls stay byte-compatible. W3A codes against
  them concurrently.
- `crates/wax-core` changes: **additive only**, and only if genuinely
  needed.

## Targets (measure, don't guess)

- Synthetic 5M-cell numeric sheet: store ≤ ~200 MB (`approx_bytes` +
  process RSS in a `#[ignore]`d bench-style test that prints both).
- Mixed text-heavy sheet: show the interning win vs naive (numbers in the
  seal).
- `window(64×24)` p95 well under 10 ms on the 5M-cell sheet (measured,
  printed by the bench test).
- **The 1.0 GiB legacy-xls outlier**: `harness/results.jsonl` on `main`
  records per-file `peakRssBytes` — find the offending xls, profile where
  the memory actually lives (calamine transient parse vs our Document vs
  store). If it's calamine-internal, document it with numbers and say so
  in the seal — that routes the fix to W3C's caps instead; don't force a
  store-side fix that can't work. Write down the resulting documented
  memory bound (docs comment in the crate + seal).

## Tests

- Keep every existing wax-store test green unchanged.
- Add: interning correctness (same string → same index, lookups intact
  across sheets), row-index windowing against a randomized oracle (build
  random sparse sheets, compare `window()` output with a brute-force
  reference), builder ingestion equivalence with `from_document`,
  `approx_bytes` sanity (grows with content, counts the string table
  once), empty sheet / single cell / full-window edge cases.

## Boundaries (do not touch)

`crates/wax-proto/**`, `crates/wax-cli/**`, `crates/wax-read/**`,
`crates/wax-fmt/**`, `harness/**`, `corpus/**`, `scripts/**`,
`.github/**`, `docs/**`, `ASSIGNMENTS.json`. You own
`crates/wax-store/**` and additive `crates/wax-core/**`. If the frozen
API genuinely can't express something you need, buz the coordinator with
the proposed amendment — do not change it unilaterally.

## Definition of done

- `scripts/check.sh` fully green in your worktree.
- Columnar internals + O(window) windowing landed; targets measured with
  real numbers in the seal; memory bound documented.
- Commit messages end with your bee name.

## Sealing

`hive seal <your-bee-name> --from seal.json` with status, summary,
deliverables, exact test counts, the measured memory/latency numbers, the
1 GiB-outlier finding, and deviations. Then
`hive buz send CL.d73 --sender <your-bee-name> --tier queue -p "sealed"`.
Blockers → buz immediately; never a silent stall.
