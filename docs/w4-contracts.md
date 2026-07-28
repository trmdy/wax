# W4 interface contracts (frozen by the coordinator)

W4 builds the writer: model → styled xlsx via `rust_xlsxwriter` and CSV,
with export-a-copy semantics — the output is always a new file derived from
the normalized model, never an in-place edit, and every feature the export
cannot preserve is reported **loudly** in the export response, never
silently. Validation is a read-back differential (wax write → wax read +
SheetJS oracle read → compare) plus `soffice --headless` does-it-open
checks. The `export` op goes fully live over the serve protocol.

Shards: **W4A** writer core (`wax-write`), **W4B** round-trip validation +
scoreboard, **W4C** serve/CLI export wiring + CI-green duty, **W4D** reader
extraction of column widths + basic styles (xlsx). A shard may not change a
contract unilaterally: propose to the coordinator (buz/seal), the
coordinator amends this file on `main`, affected shards rebase.

`docs/w1-contracts.md`, `docs/w2-contracts.md` and `docs/w3-contracts.md`
remain binding where not superseded here. The normalized dump stays
`schema: 1`; all W4 model additions are strictly additive and invisible
when absent (asserted by wax-core tests: a pre-W4 dump serializes
byte-identically).

**New since W3 (context):** the repo is public at `github.com:trmdy/wax`;
CI runs on GitHub Actions for real. Push to a branch only in your worktree;
the coordinator merges to `main` and pushes. LibreOffice 26.2 is installed
on this machine (`soffice` on PATH, also at
`/Applications/LibreOffice.app/Contents/MacOS/soffice`). The corpus payload
overlay is machine-local and gitignored — never commit corpus payloads.

## 0. Ownership map

```
crates/wax-write/**                        writer implementation      [W4A]
crates/wax-cli/**                          serve export + `wax export`[W4C]
crates/wax-read/**                         style/width extraction     [W4D]
crates/wax-core/**                         additive model only        [W4D, sign-off first]
crates/wax-store/**                        additive only              [W4D, sign-off first]
harness/wax-harness/**                     round-trip + scoreboard    [W4B]
harness/run.sh                             entry point                [W4B]
harness/adjudications.md                   verdicts (append-only)     [W4B]
scripts/**, .github/**                     CI                         [W4C]
```

`Cargo.lock` conflicts are expected; the coordinator resolves at merge.
W4A owns the `rust_xlsxwriter` dependency choice (pin a version, note it in
the seal). Nobody edits another shard's lane; if you need a change there,
buz the coordinator first.

## 1. What the coordinator already scaffolded on `main` (read it)

- **Model (`wax-core`)**: `Cell.s: Option<u32>` (index into
  `Document.styles`), `Sheet.col_infos: Vec<ColInfo>` (`colInfos` on the
  wire; explicit widths only, Excel character units), `Document.styles:
  Vec<CellStyle>` (bold/italic/underline/strike, `font_size`, `font_name`,
  `font_color`, `fill_color`; colors `#RRGGBB`). All additive: absent ⇒
  serialized to nothing ⇒ pre-W4 dumps byte-identical (pinned by test).
  Anything richer (borders, alignment, gradients, themes) is **out of the
  v1 model** — the writer must report such source features as dropped only
  where it can know about them; it never invents fidelity.
- **Store (`wax-store`)**: retains the style table, per-cell style ids
  (`WindowCell.s`, *not* exposed on the proto v0 window wire format), and
  per-sheet `col_infos`. Writer-facing accessors, additive to the frozen
  five-call API: `styles()`, `sheet_col_infos(sheet)`,
  `sheet_merges(sheet)` (full unclipped A1 ranges), and
  `scan_sheet(sheet, |r, c, cell| …)` visiting every stored cell in
  row-major order with window-identical materialization.
  `WorkbookStoreBuilder::add_ordered_sheet` gained a `col_infos` parameter
  and the builder a `set_styles`.
- **Writer seam (`wax-write`)**: the frozen API below, stubbed to return
  structured `internal` errors so dependent shards build and degrade
  gracefully today.

## 2. The writer seam (W4A) — FROZEN unless amended here

```rust
wax_write::write_xlsx(store: &WorkbookStore, out: &Path, cancel: &AtomicBool)
    -> Result<ExportOutcome, WriteError>
wax_write::write_csv(store: &WorkbookStore, sheet: u32, out: &Path, cancel: &AtomicBool)
    -> Result<ExportOutcome, WriteError>
// ExportOutcome { bytes: u64, dropped: Vec<String> }
// WriteError { code: String /* proto ErrorCode spellings */, msg: String }
```

Rustdoc in the crate is normative. Behavior:

- **write_xlsx** writes the *whole workbook* (every sheet, preserving sheet
  order and names) via `rust_xlsxwriter`:
  - Values by type: `n` → number; `s` → string; `b` → boolean; `e` → the
    error text as string (rust_xlsxwriter has no native error-value write;
    add `"error cells written as text"` to `dropped` when any occur);
    `d` → the model's ISO-8601 text parsed and written as an Excel
    datetime. Serial round-tripping matters more than string parsing
    elegance: what `wax dump` reads back must equal the source model.
  - Formulas: cells with `f` write the formula text with the cached
    result attached (`Formula::set_result` or equivalent) so consumers see
    the cached value without recalculation — this is the formula-fidelity
    contract from the mission. Formula text in the model has no leading
    `=`; add it as the library requires.
  - Number formats: cell `fmt` → `Format::set_num_format`. Intern/dedup
    `Format` objects — corpus files repeat a handful of formats across
    millions of cells; one `Format` per distinct (fmt, style) pair.
  - Basic styles: cell `s` → the store's `CellStyle` mapped onto the same
    `Format` object (bold/italic/underline/strike/size/name/font
    color/fill color).
  - Merges: `sheet_merges` ranges via `merge_range`. In the normalized
    model only the anchor cell carries a value; write the anchor value
    into the merge. A merge whose anchor is empty is written as a blank
    merge.
  - Column widths: `sheet_col_infos` → `set_column_width`.
  - **Loud drops**: every model feature the writer cannot express appends
    a distinct human-readable `dropped` entry (deduplicated), e.g.
    `"error cells written as text"`. Never an empty `dropped` when
    something was lost. Truncated models (`SheetMeta.truncated`) must add
    `"source truncated at read time; export is the truncated model"`.
  - **Cancellation**: check `cancel` at row-granularity checkpoints;
    cancelled ⇒ `WriteError { code: "cancelled" }` and **no partial output
    file left behind** (write to a temp sibling + rename, or delete on
    error — same guarantee for any error path).
  - `bytes` = the size of the finished file on disk.
- **write_csv** replicates the W3 serve CSV semantics *exactly* (RFC 4180
  quoting, UTF-8, CRLF, display string when present else raw value —
  numbers shortest-round-trip, TRUE/FALSE, error text as-is; the serve
  tests pin these). Move/port that logic into `wax-write` with tests; W4C
  swaps serve to delegate and deletes the serve-local copy. Bad `sheet` ⇒
  `bad_request`. CSV always drops: formulas (cached values only), number
  formatting beyond display strings, merges, styles, widths — the same
  loud `dropped` list serve emits today.
- No formula evaluation, ever. No reading of arbitrary files: the writer's
  only input is the store.

## 3. Protocol + CLI wiring (W4C)

- **serve `export` op**: `format:"xlsx"` goes live via
  `wax_write::write_xlsx` (op shape unchanged from proto v0/W3: `handle`,
  `format`, `out`, optional `sheet` — `sheet` is ignored for xlsx, it
  exports the whole workbook; a provided `sheet` that is out of range is
  still `bad_request` for both formats). CSV delegates to
  `wax_write::write_csv`. Response stays `{"id":n,"ok":true,"bytes":…,
  "dropped":[…]}`; the handle's open-time warnings (charts ignored, etc.)
  are **appended** to the writer's `dropped` so the response tells the
  whole export-a-copy truth. Writer `WriteError` maps straight onto the
  wire (`code`/`msg`). Export runs on the existing worker/cancel
  infrastructure like CSV does today.
- **`wax export` subcommand** (for humans, the harness, and Apiary
  debugging):
  `wax export --json <in> <out> --format xlsx|csv [--sheet N]
  [--max-cells N] [--max-bytes N] [--timeout-ms N]`
  Reads `<in>` with the same reader options/deadline machinery as `dump`,
  builds the store, calls the writer, prints one JSON line:
  `{"ok":true,"bytes":…,"dropped":[…]}` or
  `{"ok":false,"code":"…","msg":"…"}` (reader failures use the reader's
  code; exit 0 either way like `dump`). Usage line added to `wax` help;
  `--version` output unchanged.
- **CI-green duty**: after every W4 merge the coordinator pushes `main`;
  W4C owns fixing any CI breakage on GitHub Actions (`gh run list/view`)
  fast — including pre-existing workflow rot uncovered by the first live
  runs. Keep `scripts/check.sh` the single source of truth CI executes.
- Serve integration tests: export xlsx over the protocol end-to-end (open
  a fixture, export, `wax dump` the output, assert values), plus error
  paths (`bad_handle`, bad `sheet`, unwritable `out` path ⇒ `internal`
  with the io context, `format:"pdf"` ⇒ `unsupported`).

## 4. Round-trip validation + scoreboard (W4B)

Keep every existing metric; scoreboard JSON stays additive.

- **Round-trip differential**, per corpus file that wax currently opens
  (`results.jsonl` has them): M1 = normalized model from the source (the
  harness already produces it); export xlsx via **`wax export`** (fresh
  process, like the dump path; serve-based export is W4C's integration
  test, not the harness bulk path); M2 = `wax dump --json` of the exported
  file; compare M1 → M2:
  - `round-trip value fidelity %` — cells matching on (t, v) with the
    harness's existing value-equality rules (numeric epsilon, date
    handling). Denominator: M1 cells of round-tripped files.
  - `round-trip display fidelity %` — same for `d`.
  - Merges: compared set-wise per sheet; mismatches count as a per-file
    defect row in triage output (not a new headline metric).
  - A file whose export or re-read *fails* is a round-trip failure counted
    in a `round-trip files clean %` metric — never silently excluded.
  - Truncated models: skip, count, and report the skip count loudly.
- **Oracle read-back**: the SheetJS oracle reads each exported xlsx;
  report `oracle read-back open %` (does SheetJS open our output) — value
  comparison stays wax-vs-wax (M1/M2); the oracle read-back is an
  interoperability check, not ground truth for our own model.
- **soffice does-it-open**: deterministic validation subset (~200 exported
  files, seeded selection across source extensions, size-capped) through
  `soffice --headless --convert-to xlsx --outdir <tmp>` with a fresh
  `-env:UserInstallation` profile per run, time-boxed per file; clean =
  exit 0 + non-empty output. Row: `soffice-open rate`. Detect soffice on
  PATH then the app-bundle path; absent ⇒ `n/a (soffice unavailable)`
  loudly, never a fake 100%.
- **Scoreboard**: new rows under a "Writer round-trip" section + additive
  camelCase JSON under `metrics`:
  `roundTrip {filesClean, valueMatch, displayMatch, oracleOpenRate,
  sofficeOpenRate, skippedTruncated, status}`. Percentages carry
  numerators/denominators like every existing row.
- Until W4A/W4C merge, build against the stub (`wax export` may not exist;
  xlsx export returns a structured `internal` error): degrade to
  `n/a (xlsx export unavailable)` — never fail the whole run, never
  fabricate.
- Where wax's round-trip is right and SheetJS's read-back disagrees,
  adjudicate with evidence in `harness/adjudications.md`, never auto-lose.

## 5. Reader extraction (W4D) — xlsx col widths + basic styles

Populate what the scaffold modeled, for xlsx/xlsm only (Apiary v1 is
xlsx-only; xls/xlsb/ods extraction is a W5+ candidate, not silently faked):

- **Column widths**: worksheet XML `<cols>` entries (`min`/`max`/`width`,
  honoring `customWidth` semantics) → `Sheet.col_infos`, one `ColInfo` per
  explicit column (expand min..=max ranges; cap expansion at the sheet's
  used-column extent + a sane bound so a `min=1 max=16384` default-width
  declaration doesn't balloon the model — cells beyond real content don't
  need widths).
- **Basic styles**: extend the existing `xl/styles.xml` XF path (which
  already maps `numFmtId` per cell) to also resolve each XF's `fontId` →
  bold/italic/underline/strike/size/name/color and `fillId` → solid
  `patternFill` foreground color. Colors: `rgb` attrs and `indexed` via
  the standard legacy palette; `theme`/`tint` are **dropped** (no theme
  resolution in v1 — do not guess). Deduplicate into `Document.styles`;
  cells reference via `Cell.s`. Cells with a fully-default style get
  `s: None`, not a pointless index — keep the table small.
- All through the existing safety rails (XML guards apply; styles parsing
  must not regress open rates — a malformed styles part degrades to
  no-styles + a warning, it does not fail the open).
- Tests: unit fixtures for cols/fonts/fills/indexed colors + a corpus
  spot-check test; `wax dump` of a pre-W4 fixture without explicit
  widths/styles stays byte-identical (the additive-invisibility contract).
- Measure and report in the seal: dump-size and RSS impact on the corpus
  (expected small; style table is deduped, `s` is 4 bytes/cell in the
  store).

## 6. The W4 gate

- Round-trip value fidelity **≥99%** on corpus-derived models.
- soffice-open **clean** on the validation set.
- `export` (xlsx + csv) live over serve, driven end-to-end by tests.
- CI **green on origin/main**.
- `scripts/check.sh` fully green at every seal; scoreboard delta committed
  at every shard merge; commit messages end with your bee name. No gate is
  relaxed silently — deviations go in the wave seal.

## 7. Ground rules (unchanged)

Stay in your lane (§0). Tests for logic. Blockers → buz the coordinator
immediately; never a silent stall (a shard silent >30 min mid-task gets
nudged once, then retired). Nothing touches the apiary repo. Never commit
corpus payloads.
