# Review — W3C: safety rails + fuzz (`agent/wax-w3c-safety`, 5db7792)

Reviewer/finisher: CL.d73 (coordinator). Verdict: **merge**.

## Provenance (unusual, recorded honestly)

Two implementor bees were assigned this shard and both wedged mid-turn:
`wax-w3c` (CO.a5b) built the rails then stalled three times at the
long-fuzz-burn step; its successor `wax-w3c2` (CO.9821) stalled twice
without producing a commit. The coordinator checkpointed CO.a5b's
uncommitted work (`d1757f4`), retired both bees, and finished the shard
directly. The predecessor's design is intact and good; commits `82136e6`
onward are the coordinator's.

## What shipped

- `crates/wax-read/src/safety.rs` (~900 lines + tests): input-size cap,
  zip preflight (entry count, per-part and total decompressed caps,
  ratio-bomb check), XML guards (depth, DOCTYPE/DTD rejection, token
  caps, plus a test pinning that quick-xml does not expand custom
  entities), `read_with_deadline` (owned worker, preflight-then-read,
  structured `timeout` at the wall clock, detached worker documented),
  and the legacy-CFB/BIFF preflight.
- `wax dump --max-bytes`; `ReaderOptions` gained additive `max_bytes`,
  `max_declared_cells`, ratio and cap fields, all with defaults.
- The W3A merge seam is closed: `serve.rs` and `main.rs` now call
  `read_with_deadline(CalamineReader, …)` and map `maxBytes` into
  `ReaderOptions.max_bytes`.
- `fuzz/`: three targets (container preflight, xlsx reader, legacy xls
  reader) with committed seed corpora, wired into `scripts/check.sh` and
  a nightly CI job.

## Fuzz findings — five guarded, one open

Every one is a defect in `calamine 0.36.1` reached through wax; wax does
not fork calamine, it refuses the input first. All five have committed
seeds and regression tests:

1. Truncated CFB / partial sector (predecessor).
2. Zero-length BOF record (predecessor).
3. `BOUNDSHEET` sheet offset past the Workbook stream → slice panic at
   `xls.rs:606`.
4. Lying CFB header sector counts → 13.9 GB up-front FAT allocation.
5. `MulRk` reversed column bounds → underflow panic at `xls.rs:936`;
   plus a fixed-header minimum-length table covering every record
   calamine reads at fixed offsets — including three whose *match guards*
   read a u16 before the arm body runs (`FilePass`, `CodePage`,
   `DateMode`).

**Open, quarantined:** `fuzz/known-findings/legacy_xls_reader/` holds a
5,640-byte input where calamine's own CFB reader grows a `Vec` unbounded
(`malloc(137438953472)`). wax returns a structured `timeout` document
rather than crashing, but the abandoned worker reaches **87 GiB peak
RSS** — a host-OOM risk for Apiary. The README there records the
diagnosis (including two upstream smells: an acknowledged
`//TODO: check if in infinite loop` in the DIFAT walk, and a DIFAT length
read from header offset 62 instead of 72) and three W5 fix options, of
which running the parse in a child process under an address-space rlimit
is the one that also caps every future unknown of this class.

## Corpus fit (coordinator work, the part that mattered most)

The rails as first written rejected **126 previously-opening corpus
files**. Root causes and fixes:

- Requiring whole-sector CFB alignment: real xls files carry trailing
  padding. Removed; the finer per-record and header-count guards give the
  panic-safety the alignment check was standing in for.
- Scanning the Workbook stream as one flat record sequence: calamine reads
  a *globals* substream then each BOUNDSHEET-declared *sheet* substream,
  each terminated by EOF, and never reads the slack between them. The
  walk now mirrors that exactly.
- A 10 MiB ratio-bomb floor flagged legitimate large-shared-string files;
  raised to 256 MiB.
- The declared-extent check rejected reversed DIMENSIONS bounds outright,
  but calamine's `saturating_mul` makes those harmless when the other
  dimension is 0. It now reproduces calamine's arithmetic bit for bit
  (wrapping subtraction, `- 1` exclusive-bound conversion, saturating
  product) and judges the product.

Final sweep over all 1,964 previously-opening files: **4 rejections, all
adjudicated** (2× the POI 51535 extent bomb, 2× ClusterFuzz-crafted
xlsx with failing entry checksums) in `harness/adjudications.md`.

## Gate evidence

- POI `51535.xls`: peak RSS **1,077,821,440 → 2,424,832 bytes**, now a
  `bomb` error naming the declared `65536x256` extent, in ~2 ms. This
  removes the mission's 1 GiB RSS outlier.
- `scripts/check.sh` fully green including the fuzz stage.
- The fuzz stage is now a **deterministic** seed+artifact replay
  (`-runs=0`), not a timed discovery run — a gate must not fail because a
  random mutation found something new. Timed discovery moved to
  `--fuzz-burn` (default 300 s, `WAX_FUZZ_BURN_SECONDS`) and the nightly
  CI job, where a new finding is a task rather than a broken build.

## Findings against the shipped code (non-blocking)

1. The preflight buffers the whole Workbook stream. Bounded by
   `max_bytes` (100 MiB default) and calamine buffers it anyway, but it
   is a real per-open allocation worth noting.
2. Minimum-length table is empirical (fuzz-derived), not exhaustive
   against MS-XLS. Fine as defense-in-depth behind `catch_unwind`;
   completeness is a W5 concern.
3. `read_with_deadline` detaches timed-out workers by design — the open
   finding above shows the cost: a detached worker can keep allocating.
   Strengthens the case for the child-process rlimit option in W5.
