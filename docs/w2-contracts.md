# W2 interface contracts (frozen by the coordinator)

W2 replaces the stub reader with calamine (W2A), builds the ECMA-376
number-format interpreter (W2B), and upgrades the harness scoring (W2C).
These contracts are the seams that let the three shards run concurrently.
A shard may not change a contract unilaterally: propose to the coordinator
(buz/seal), the coordinator amends this file on `main`, affected shards
rebase.

`docs/w1-contracts.md` remains binding everywhere it isn't superseded here —
in particular §1 (normalized dump schema, unchanged, still `schema: 1`),
§2 (CLI surface), §6 (scoreboard basics), §7 (ground rules).

**Operator amendments for W2 (2026-07-28, binding):**

1. W2B mines the real corpus **first**: extract every distinct number-format
   code from the 2,048-file corpus with frequency counts, commit that as the
   format test-set, and implement by real-world frequency. The ≥95%
   display-coverage swap gate is measured against corpus formats.
2. The coordinator re-runs `harness/run.sh` and commits the scoreboard delta
   at **every** shard merge, not just wave end.

## 0. Ownership map

```
crates/wax-read/**          calamine reader + normalization        [W2A]
crates/wax-cli/**           wire the new reader                    [W2A]
crates/wax-core/**          model extensions (additive only)       [W2A]
crates/wax-fmt/**           format interpreter (stub on main)      [W2B]
harness/formats/**          corpus format mining + test-set        [W2B]
harness/wax-harness/**      comparator + scoreboard upgrades       [W2C]
harness/run.sh              entry point                            [W2C]
```

Shared file `Cargo.lock`: expect conflicts; the coordinator resolves at
merge. Do not add heavy dependencies without noting it in your seal.

## 1. The reader (W2A)

`CalamineReader` replaces `StubReader` behind the existing
`wax_read::Reader` trait (`read(&self, path, ReaderOptions) -> Document`).
`StubReader` stays in-tree (tests may reference it) but the CLI switches to
`CalamineReader`. Requirements:

- **Formats:** xlsx, xlsm, xlsb, legacy xls, ods via calamine. The W2 gate
  is measured on xlsx; the others are best-effort but must never panic —
  a file calamine rejects is an `ok:false` document with a sensible `code`
  (`bad_zip | unsupported | too_large | timeout | internal`).
- **Values:** numbers, text (shared strings included), booleans, errors.
- **Dates:** 1900/1904 epoch resolved from the workbook; date-typed cells
  emit `t:"d"`, `v` = ISO-8601 per w1-contracts §1. Keep the raw serial on
  hand — the display string comes from `wax_fmt::render` on the **raw
  serial**, not the ISO string. A serial the reader cannot confidently call
  a date stays `t:"n"`.
- **Formulas:** formula text in `f` (no leading `=`, whitespace preserved),
  cached result in `v`/`t`. Absent cached result → `v: null`.
- **Merges:** A1-style, ascending, per sheet.
- **`fmt`:** the cell's number-format code string; `null` for
  General/unknown. If calamine's public API does not expose per-cell format
  codes for a container, parse the style part directly (e.g. `xl/styles.xml`
  cellXfs → numFmtId → code, builtin table for ids 0–49) — do not fork
  calamine.
- **`d`:** always via `wax_fmt::render(code, value, epoch_1904)` — including
  `"General"` when the cell has no explicit format. `None` → `d: null`.
  Never hand-roll display strings in the reader.
- **Caps:** `max_cells` with loud `truncated: true` (both sheet and
  document), `timeout_ms` respected between sheets at minimum.

## 2. The format interpreter (W2B)

Crate `crates/wax-fmt` exists on `main` as a stub. The public signature is
frozen:

```rust
pub enum FmtValue<'a> { Number(f64), Text(&'a str), Bool(bool), Error(&'a str) }
pub fn render(code: &str, value: FmtValue<'_>, epoch_1904: bool) -> Option<String>;
```

- `code` is the raw ECMA-376 format code (`"General"` included; callers pass
  `"General"` for cells with no explicit format). Date serials arrive as
  `FmtValue::Number` with the raw serial; `epoch_1904` selects the epoch.
- Return `Some(display)` only when confident the string is what Excel would
  show. Unsupported code or combination → `None` (callers emit `d: null`).
  **Never guess** — a wrong display string is worse than a null; the
  scoreboard counts nulls honestly.
- **Locale, v1:** en-US separators (`,` group, `.` decimal) — this matches
  Excel's default rendering and the SheetJS oracle we score against.
  Locale-aware rendering (`12 410,50 kr`) is a post-W2 concern; do not
  build locale plumbing into the public signature.
- W2B may add public API *additively* (e.g. a parsed-format cache type) but
  `render` stays as-is; W2A codes against it and nothing else.

### The corpus format test-set (mined FIRST — operator amendment 1)

Before implementing the interpreter, W2B mines the corpus via the SheetJS
oracle dumps (the oracle already emits `fmt` per cell; that covers every
container SheetJS opens, not just xlsx):

- `harness/formats/mine.mjs` (or `.sh`) — runs the oracle over
  `corpus/manifest.jsonl`, aggregates every distinct `fmt` code.
- `harness/formats/corpus-formats.json` — **committed**: one entry per
  distinct format code with `{ code, cellCount, fileCount }`, sorted by
  cellCount descending, plus totals and a generation timestamp. Private
  corpus entries: counts may be included, file names/paths must not.
- Implementation order and test priorities follow this ranking. The ≥95%
  display-coverage swap gate is measured against these corpus formats,
  cell-frequency-weighted.

Tests live in `crates/wax-fmt` (unit + a corpus-formats-driven coverage
test that asserts the supported share and prints the number honestly).

## 3. Harness upgrades (W2C)

Keep every W1 metric; add, without breaking `harness/scoreboard.json`
consumers (additive JSON only):

- **display-string match %** — over cells where *both* tools emit non-null
  `d`, exact string equality. Coverage says how often wax speaks; match
  says whether it says the right thing.
- **Per-extension breakdown** — files-opened % and cell-value match % split
  by manifest `ext` (the W2 gate reads the xlsx row).
- **Per-format-code display coverage** — top-N format codes by cell count
  with wax coverage/match per code, written to
  `harness/format-coverage.json` (+ a short table in SCOREBOARD.md). Joins
  against `harness/formats/corpus-formats.json` when present; degrade
  gracefully (skip the join) when absent — do not depend on W2B's branch.
- **Mismatch triage** — `harness/triage.md` regenerated per run: top
  disagreement categories (open-failure codes, value-mismatch buckets,
  display mismatches) with example files, so adjudication work is targeted.
- Window latency stays `n/a` (no protocol until W3). Never fabricate.

## 4. Ground rules (unchanged from W1, plus)

- Stay in your lane (§0); cross-shard edits need coordinator sign-off.
- `scripts/check.sh` green before sealing: fmt, clippy `-D warnings`,
  tests, oracle npm tests.
- The W2 gate: wax opens ≥90% of xlsx corpus files, cell-value fidelity
  ≥95% on opened files, display coverage climbing with the format test-set
  committed. No gate is relaxed silently.
- Disagreements with the oracle where wax is right go to
  `harness/adjudications.md` with evidence — never auto-lose.
- Commit messages end with your bee name.
