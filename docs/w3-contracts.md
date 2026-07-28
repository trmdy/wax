# W3 interface contracts (frozen by the coordinator)

W3 builds the product's spine: the NDJSON stdio protocol server (W3A), the
columnar windowed store (W3B), safety rails + fuzz targets (W3C), the
harness protocol client + scoreboard upgrades (W3D), and — capacity
permitting, never displacing core W3 — the reader-lane carry-overs (W3E).
These contracts are the seams that let the shards run concurrently. A shard
may not change a contract unilaterally: propose to the coordinator
(buz/seal), the coordinator amends this file on `main`, affected shards
rebase.

`docs/w1-contracts.md` and `docs/w2-contracts.md` remain binding where not
superseded here. The normalized dump stays `schema: 1`; `wax dump` behavior
is unchanged (the harness differential still uses it).

**Operator priorities for W3 (2026-07-28, binding):** core = W3A + W3B +
W3C (+ W3D, which carries the gate measurement). Reader carry-overs (W3E)
run only as capacity allows. Apiary v1 ships xlsx-only; the whole-corpus
numbers stay honest but do not gate. Scoreboard must gain per-extension
formula-text fidelity rows (xlsx measured 99.96% in W2 — make that visible
instead of the misleading 18.80% corpus-wide figure) and a real
window-latency row.

## 0. Ownership map

```
crates/wax-proto/**             wire types, error codes, proto version [W3A]
crates/wax-cli/**               `wax serve` loop + CLI                 [W3A]
crates/wax-store/**             columnar store internals               [W3B]
crates/wax-core/**              additive model changes only            [W3B]
crates/wax-read/src/safety.rs   container/XML rails (new file)        [W3C]
crates/wax-read/src/lib.rs      rail hooks only (small, surgical)      [W3C]
fuzz/**                         cargo-fuzz targets + seed corpus       [W3C]
scripts/check.sh                fuzz stage wiring                      [W3C]
.github/workflows/**            CI fuzz job                            [W3C]
harness/wax-harness/**          protocol client + scoreboard           [W3D]
harness/run.sh                  entry point                            [W3D]
crates/wax-read/src/calamine_reader.rs + new style modules  [W3E]
harness/adjudications.md        verdict entries (append-only)          [W3E]
```

`Cargo.lock` conflicts are expected; the coordinator resolves at merge.
W3C and W3E share `crates/wax-read`: W3C stays in `safety.rs` + minimal
hooks in `lib.rs`; W3E stays in `calamine_reader.rs` + new modules. If
either needs the other's file beyond a `mod` line, buz the coordinator
first. Nobody edits `crates/wax-store/src/lib.rs` except W3B; nobody edits
`crates/wax-proto` except W3A.

## 1. Protocol v0 — the wire spec (FINAL unless amended here)

`wax serve` speaks NDJSON: one JSON object per line, requests on stdin,
responses on stdout (one line each, flushed), diagnostics on stderr. Every
request carries a client-chosen `id` (u64, unique per in-flight request);
every response echoes it. Responses may arrive **out of order** — ops may
execute concurrently. There is no chunking in v0: window caps (below)
bound response-line size; this is a recorded deviation from the mission
sketch's "big payloads chunked", accepted for v1.

Errors, uniformly: `{"id":n,"ok":false,"code":"…","msg":"…"}` with `code`
from `wax-proto::ErrorCode`: `unsupported | bad_zip | too_large | timeout |
internal | bomb | bad_request | bad_handle | cancelled` (already on `main`).
A line that isn't valid JSON or lacks a usable `id` gets
`{"id":null,"ok":false,"code":"bad_request","msg":"…"}`. Unknown `op` →
`bad_request`.

### Ops

**version** `{"id":1,"op":"version"}` →
`{"id":1,"ok":true,"proto":0,"version":"0.1.0"}`

**open** `{"id":2,"op":"open","path":"/abs/file.xlsx","maxCells":5000000,
"maxBytes":104857600,"timeoutMs":30000}` (the three options are optional;
defaults as shown — note serve's `maxCells` default is 5,000,000, *not*
dump's 200,000) →
`{"id":2,"ok":true,"proto":0,"handle":"h1","truncated":false,
"sheets":[{"name":"Costs","rows":195114,"cols":12,"truncated":false}],
"warnings":["charts ignored (2)"]}`
Handles are `h1, h2, …` monotonic per process. A file the reader rejects
returns the reader's error code (`bad_zip`, `too_large`, `bomb`, …) and no
handle. `open` responses carry `proto` — that is the version handshake.

**meta** `{"id":3,"op":"meta","handle":"h1"}` → the same
`sheets`/`truncated`/`warnings` payload as `open` (no `proto`, no new
handle).

**window** `{"id":4,"op":"window","handle":"h1","sheet":0,"r0":0,"c0":0,
"nr":64,"nc":24}` →
`{"id":4,"ok":true,"sheet":0,"r0":0,"c0":0,"nr":64,"nc":24,
"rows":[[{"t":"n","v":1.5,"d":"1.50"},null,…],…],"merges":["A1:B2"]}`
- Request cap: `nr*nc ≤ 262144` and `nr,nc ≥ 1`, else `bad_request`.
- The server clips to the sheet extent and echoes the **effective**
  `r0/c0/nr/nc`; `rows` is exactly `nr` arrays of exactly `nc` entries.
  A window fully outside the extent is `ok:true` with `nr:0,nc:0,rows:[]`.
- Cell fields are dump semantics (`t,v,d,f,fmt`) minus coordinates; `null`
  fields may be omitted on the wire (`v` stays, even when `null`). Empty
  cells are literal `null`.
- `merges`: full unclipped A1 ranges intersecting the effective window.
- `sheet` out of range → `bad_request`.

**export** `{"id":5,"op":"export","handle":"h1","format":"csv",
"out":"/abs/copy.csv","sheet":0}` →
`{"id":5,"ok":true,"bytes":18320,"dropped":["formulas (cached values
only)","number formatting beyond display strings","merges"]}`
W3 implements `format:"csv"` only; `"xlsx"` → `unsupported` (W4 adds it —
same op shape). CSV: RFC 4180 quoting, UTF-8, CRLF, `sheet` defaults to 0;
each cell emits its display string `d` when non-null, else the raw value
(number via shortest round-trip, bool as TRUE/FALSE, error text as-is).
`dropped` is loud and honest, never empty-when-lossy.

**close** `{"id":6,"op":"close","handle":"h1"}` → `{"id":6,"ok":true}`.
Unknown/expired handle → `bad_handle`.

**cancel** `{"id":7,"op":"cancel","target":2}` →
`{"id":7,"ok":true,"found":true}`. Best-effort: if request 2 is still in
flight it additionally answers `{"id":2,"ok":false,"code":"cancelled"}`.
Cancellation is cooperative — a parse blocked inside calamine cancels at
the next checkpoint; the watchdog timeout is the hard backstop.

**stats** `{"id":8,"op":"stats"}` →
`{"id":8,"ok":true,"peakRssBytes":52428800,"handles":1,
"storeBytes":1234567}` — self-reported via getrusage + store
`approx_bytes()` sums. The harness uses this for the serve-RSS row.

### Lifecycle

- `wax serve [--idle-timeout-ms N] [--max-handles N]` — defaults 300000 /
  16. An idle handle (no op touching it for the window) expires; later use
  → `bad_handle` with `"expired"` in the msg. Opening beyond
  `--max-handles` → `bad_request`.
- EOF on stdin: finish/cancel in-flight ops, exit 0. SIGTERM: exit
  promptly and cleanly (no partial response line on stdout).
- Per-op `timeoutMs` (open) is wall-clock; on expiry the op answers
  `timeout` even if a worker thread is still stuck (abandoned threads are
  acceptable in v0; document it).
- `wax --version` already prints `wax <semver> (proto 0)` — unchanged.

## 2. The store seam (W3A ↔ W3B)

`crates/wax-store` is on `main` with a naive implementation and this
**frozen** public API (see rustdoc in the crate):

```rust
WorkbookStore::from_document(Document) -> WorkbookStore
sheet_count() -> u32
sheet_meta(sheet: u32) -> Option<SheetMeta>          // name, rows, cols, truncated
window(sheet, r0, c0, nr, nc) -> Option<Window>       // clipped; None = bad sheet
approx_bytes() -> usize
```

W3A codes against exactly this and nothing deeper. W3B rewrites the
internals: typed columnar storage per sheet (type tags + f64/bool columns,
interned string table for text/display/formula/fmt strings — fmt codes and
display strings repeat massively, intern them), `window()` in O(window)
via per-row or row-block indexing, and an honest `approx_bytes()`.
Additive API (e.g. a builder ingesting sheets incrementally) is welcome;
changing or removing the five calls above requires a coordinator
amendment. Targets: 5M-cell numeric sheet ≤ ~200 MB resident in the store;
`window(64×24)` p95 well under 10 ms once parsed; the ~1.0 GiB legacy-xls
RSS outlier understood and reduced or capped (coordinate with W3C's caps —
if the memory lives inside calamine's transient parse, say so with
numbers; that finding routes the fix, possibly to a cap not a store
change).

## 3. Safety rails (W3C)

All rails live in `wax-read` (new `safety.rs` + hooks), run **before**
calamine sees a byte where possible, and produce structured errors, never
panics:

- **Container preflight** (zip-based formats — xlsx/xlsm/xlsb/ods): entry
  count cap (default 10,000), per-part decompressed cap (default 512 MiB),
  total decompressed cap (default 2 GiB), compression-ratio bomb check
  (ratio > 100:1 AND decompressed > 10 MiB → `bomb`). Caps configurable
  via `ReaderOptions` (additive fields with defaults; serve's `maxBytes`
  maps to input-file size cap, default 100 MiB).
- **XML guards** on wax's own quick-xml paths: depth cap (256), reject
  DOCTYPE/internal DTD subsets (`bomb`), token/buffer size caps.
  quick-xml does not expand custom entities — assert that with a test so a
  dependency upgrade can't silently regress it.
- **Wall-clock timeout** that actually fires: `wax dump --timeout-ms N`
  must produce an `ok:false` `timeout` document within ~2×N even when the
  parser is stuck inside calamine (watchdog thread/process; implementation
  latitude, observable behavior fixed). Expose the same mechanism as a
  library call so serve (W3A) gets it for free — coordinate the signature
  via buz early; a `wax_read::read_with_deadline(...)` wrapper is the
  expected shape.
- **cargo-fuzz** targets under `fuzz/` (workspace-excluded, own
  Cargo.toml, as cargo-fuzz sets up): at minimum container preflight
  (arbitrary bytes), xlsx path via `CalamineReader` (bytes → temp file →
  read with tight caps/timeout), and one legacy path (xls or xlsb). Seed
  corpora: a handful of *small* corpus files committed under
  `fuzz/corpus/<target>/` (public files only, < 50 KiB each).
- **scripts/check.sh**: add a fuzz stage — when nightly + cargo-fuzz are
  available run `cargo fuzz build` plus a short smoke of each target
  (`-max_total_time=30`); otherwise a loud SKIP line. CI (`ci.yml`) gains
  a job that installs nightly + cargo-fuzz and runs that stage for real.
  Findings that crash → fix in reader if it's ours, minimal repro + cap if
  it's calamine's, recorded in the seal either way.

## 4. Harness upgrades (W3D)

Keep every W1/W2 metric; JSON stays additive.

- **Protocol client**: a `serve`-mode driver inside `wax-harness` (Rust)
  that, per corpus file, spawns a fresh `wax serve`, then: `version` →
  `open` → `meta` → first window `(0,0,64,24)` → 4 more 64×24 windows at
  deterministic in-extent offsets (corners + center; fewer when the sheet
  is small) → `stats` → `close` → EOF. Records per-request wall time,
  open success, and `stats.peakRssBytes`. This is the W3 **gate
  mechanism**: it must run end-to-end over the corpus. Export smoke:
  for a deterministic ~50-file xlsx subset, `export` to CSV in a temp dir
  and assert `ok:true` + non-empty file.
- **Scoreboard JSON shape** (amended 2026-07-28 after W3D's buz, binding):
  new serve measurements land in an additive `serveMetrics` object —
  `{ windowLatencyMs: {p50,p95}, peakRssBytes: {p50,max},
  openViaServePct, … }`. The legacy `metrics.windowLatencyMs` keeps its
  nullable-scalar shape; its `wax` field is filled with the measured p50
  scalar once real numbers exist (`sheetjs` stays `null` — no protocol to
  measure). The SCOREBOARD.md row renders p50/p95 from `serveMetrics`.
- **Scoreboard rows** (replace `n/a`, never fabricate):
  - `window latency` — p50/p95 over all window requests corpus-wide.
  - `serve peak RSS (p50 / max)` — from `stats`.
  - `open-via-serve %` — files opened over the protocol (should track
    `files opened %`; a gap is a bug worth surfacing).
- **Per-extension table**: add `formula-text fidelity %` and
  `cached-result fidelity %` columns (same definitions as the corpus-wide
  rows, split by manifest `ext`). This makes xlsx's ~99.96% formula
  fidelity visible.
- Until W3A's `serve` merges, build against the spec in §1 and test the
  client against a tiny mock server (a fixture script speaking v0); the
  real end-to-end numbers land at integration. Degrade gracefully: if the
  wax binary lacks `serve`, print `n/a (serve unavailable)` — do not fail
  the whole run.

## 5. Reader carry-overs (W3E — capacity lane)

Sanctioned by the operator **only as capacity allows**; must never block
or displace W3A–D. Three work items, in priority order:

1. **xls/xlsb per-cell number-format codes** — the biggest whole-corpus
   display-match lever (xls display match 74.17%, xlsb 57.50%, both from
   missing `fmt` codes; W2 adjudication showed the formatter is right when
   given the code). For xls: BIFF `FORMAT`/`XF` records; calamine exposes
   xls format info partially — investigate its API first, else parse the
   OLE2/BIFF stream directly (new module; a small dep like `cfb` is
   acceptable, note it in the seal). For xlsb: parse `xl/styles.bin`
   (`BrtFmt`/`BrtXF`) from the zip directly. Wire into
   `calamine_reader.rs` exactly like the xlsx styles path; `d` still comes
   only from `wax_fmt::render`.
   *Scope addition (2026-07-28, after W3B's outlier profile):* an
   **extent-bomb guard** rides along with the BIFF scanner — corpus
   POI `51535.xls` (41,984 bytes) declares a 65536×256 BIFF extent and
   calamine's dense `Range` allocation transiently costs ~1.0 GiB RSS for
   zero cells. Before materializing an xls range, pre-scan `DIMENSIONS`;
   declared extent > 8,000,000 cells → structured `bomb` error naming the
   extent and cap. Dense-extent bombs on other containers are a noted W5
   hazard (W3C's timeout is the v0 backstop there).
2. **xlsb value-match investigation** (73.89% vs xls 98.11%) — triage a
   sample of mismatching files (`harness/triage.md` has buckets), find the
   systematic cause(s), fix what's ours, adjudicate what's SheetJS's
   (append to `harness/adjudications.md` with evidence).
3. **xlsx open-failure triage** (37 files) — classify: password-protected
   → proper error code; nonstandard part names / strict OOXML → cheap
   fixes if they're cheap; genuinely corrupt → adjudicate.

Success = measured scoreboard movement with honest numbers, not a target
hit at any cost. Expected order of magnitude: xls display match → ≥90%,
xlsb → ≥85%, whole-corpus display match → ≥85%.

## 6. The W3 gate

- The harness protocol client drives `open`/`window`/`export` end-to-end
  over stdio against the corpus (fresh serve per file), and the scoreboard
  carries **real** window-latency and serve-RSS numbers.
- Documented store memory bound; the 1 GiB legacy-xls outlier explained
  and reduced/capped.
- Fuzz targets build and smoke clean in `scripts/check.sh` + CI.
- `scripts/check.sh` fully green at every seal; commit messages end with
  your bee name; scoreboard delta committed at every shard merge (operator
  amendment, carried over from W2).
- No gate is relaxed silently — deviations go in the wave seal.

## 7. Ground rules (unchanged)

Stay in your lane (§0). Tests for logic. Disagreements with the oracle
where wax is right → `harness/adjudications.md`, never auto-lose. Blockers
→ buz the coordinator immediately; never a silent stall. Nothing touches
the apiary repo.
