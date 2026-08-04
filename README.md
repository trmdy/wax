# wax

wax is an out-of-process spreadsheet engine. The prebuilt Rust sidecar reads
XLSX, XLS, XLSB, and ODS files, keeps parsed workbooks in a bounded windowed
store, and exports XLSX or CSV over an NDJSON stdio protocol. It gives desktop
applications spreadsheet support without loading a parser or a Rust toolchain
into the application process.

## Install

[GitHub releases](https://github.com/trmdy/wax/releases) contain one archive
for each supported platform:

| Host | Archive platform |
| --- | --- |
| Apple silicon macOS | `macos-arm64` |
| Intel macOS | `macos-x64` |
| x86-64 glibc Linux | `linux-x64` |

Download `SHA256SUMS.txt` and the archive
`wax-v<version>-<platform>.tar.gz`, then verify and extract it:

```bash
set -euo pipefail
version=0.5.0
platform=macos-arm64
archive="wax-v${version}-${platform}.tar.gz"
base="https://github.com/trmdy/wax/releases/download/v${version}"

curl -fLO "${base}/${archive}"
curl -fLO "${base}/SHA256SUMS.txt"
grep -F "  ${archive}" SHA256SUMS.txt | shasum -a 256 -c -
mkdir "wax-v${version}"
tar -xzf "$archive" -C "wax-v${version}"
"wax-v${version}/wax" --version
```

The final command prints `wax 0.5.0 (proto 0)` for the v0.5.0 release. Move
the binary to a directory on `PATH` if desired.

To build instead, install Rust and run:

```bash
cargo build --locked --release
target/release/wax --version
```

The repository pins its Rust version and required components in
[`rust-toolchain.toml`](rust-toolchain.toml); `rustup` selects it
automatically.

## Serve protocol

`wax serve` reads one JSON request per stdin line and writes one correlated
JSON response per stdout line, flushing each response. Every request has a
client-selected unsigned integer `id`; concurrent responses may arrive out of
order.

The main workbook lifecycle is:

- `open` parses a path and returns a `handle`, sheet metadata, warnings,
  truncation status, and the protocol number.
- `meta` refreshes metadata for a handle.
- `window` returns a clipped row/column window plus intersecting merges.
- `recalc` evaluates dirty downstream formulas against a hypothetical edit
  layer without mutating the workbook.
- `export` writes an XLSX or CSV copy and reports every dropped feature.
- `close` releases a handle.

Successful `version` and `open` responses advertise capabilities additively
in `caps` (absence means no capabilities); the `--version` line never
carries them. The current server advertises
`caps:["exportOverrides","sheetSizeInfos","exportSizeOverrides","formulaEval","sheetView","authoredFormulas"]`.

This is a representative v0 session (one object per line):

```json
{"id":1,"op":"open","path":"/absolute/path/book.xlsx","maxCells":5000000,"maxBytes":104857600,"timeoutMs":30000}
{"id":1,"ok":true,"proto":0,"caps":["exportOverrides","sheetSizeInfos","exportSizeOverrides","formulaEval","sheetView","authoredFormulas"],"handle":"h1","truncated":false,"sheets":[{"name":"Sheet1","rows":2,"cols":2,"truncated":false,"colInfos":[{"c":1,"width":22.5}],"rowInfos":[{"r":0,"height":27.75}],"defaultRowHeight":15.0,"defaultColWidth":8.43,"frozenRows":1,"frozenCols":0}],"warnings":[]}
{"id":2,"op":"window","handle":"h1","sheet":0,"r0":0,"c0":0,"nr":2,"nc":2}
{"id":2,"ok":true,"sheet":0,"r0":0,"c0":0,"nr":2,"nc":2,"rows":[[{"t":"s","v":"Item","d":"Item"},{"t":"s","v":"Cost","d":"Cost"}],[{"t":"s","v":"Tea","d":"Tea"},{"t":"n","v":12.5,"d":"12.50"}]],"merges":[]}
{"id":3,"op":"close","handle":"h1"}
{"id":3,"ok":true}
```

An `open` defaults to 5,000,000 cells, 100 MiB input, and a 30-second
wall-clock timeout. A `window` is capped at 262,144 requested cells; the
server defaults to 16 handles with a five-minute idle timeout. Limits and
reader safety rails fail loudly or mark a result as truncated.

An `export` optionally carries `overrides` — an array of up to 100,000
`{"sheet":0,"r":1,"c":2,"v":42.5}` edits (zero-based absolute indices)
layered over the read model before the writer runs; the store itself is
never mutated. `v` is a JSON number, string, boolean, or `null` (clear).
Strings beginning with `=` stay text, numbers are never coerced to dates
(the retained format code carries date semantics), an overridden cell keeps
its style and format code and gets its display string re-rendered, an
overridden formula cell drops the formula loudly in `dropped`, duplicates
are last-wins, and the response reports the post-collapse count in
`applied`. Overrides may extend the used extent, subject to the extent/bomb
caps. `format:"csv"` accepts the same field, applies only the exported
sheet's overrides, and ignores edits for other (valid) sheets. The same
edits work offline via `wax export --overrides <json-file>`.

Under `authoredFormulas`, an override may carry
`{"sheet":0,"r":4,"c":2,"f":"=SUM(A1:A3)","v":6}`. The optional `v`
is only the caller's advisory cache: wax parses and computes `f`, adds it to
the request-local dependency graph, and uses the engine result. Recalc and
export both support this shape; XLSX export preserves a real formula with a
fresh cached value. Without `f`, all v0.2 literal rules remain unchanged.
Unknown functions, invalid syntax, and cycles are cell errors (`#NAME?`,
`#VALUE!`, and `#CYCLE!`) rather than request failures.

Under `sheetSizeInfos`, every `open`/`meta` sheet entry always carries four
size fields: `colInfos` (`[{c,width}]`, Excel character units, explicit
custom widths only), `rowInfos` (`[{r,height}]`, points, any height the
container declares — user-set or autofit-persisted), and concrete
`defaultRowHeight`/`defaultColWidth` (container declarations when present,
otherwise the Excel fallbacks 15.0/8.43 — consumers never need their own
fallback). The same fields appear per sheet in `wax dump` output
additively. xlsx, xls, and xlsb are all covered; other containers report
empty arrays plus the fallback defaults. Exports carry sizes through
exactly (heights, widths, and declared defaults survive a read-write-read
loop byte-for-byte).

Under `exportSizeOverrides`, an `export` optionally carries
`sizeOverrides:{"cols":[{"sheet":0,"c":1,"width":24.5}],"rows":[{"sheet":0,
"r":0,"height":30.0}]}` (same zero-based indices and units): entries
collapse last-wins, layer over the source's declared sizes, clamp loudly to
0..=255 chars / 0..=409.5 points, and share the 100,000-entry cap with cell
overrides. csv exports drop size overrides loudly with a counted `dropped`
entry scoped to the exported sheet.

Under `formulaEval`, wax evaluates the v0.4 XLSX/XLSM formula subset at open:
arithmetic, comparisons, `&`, scalar A1 references (including cross-sheet
references), ranges passed to list-consuming functions, and `SUM`, `AVERAGE`,
`COUNT`, `COUNTA`, `MIN`, `MAX`, `IF`, `AND`, `OR`, `NOT`, `ROUND`, `ABS`, and
`CONCAT`. Evaluated window cells carry `e:true`; their computed value and
display string replace the file cache while their formula and format code
remain intact. Unknown functions remain file-cached and omit `e`. Legacy
implicit-intersection formulas, array/range arithmetic, and dynamic arrays are
outside this scalar MVP and likewise remain file-cached. Cycles return
`#CYCLE!`, and the one-second wall-clock evaluation budget degrades loudly
through `warnings` instead of hanging.

`recalc` accepts the same override entries as export and returns changed
override cells plus downstream evaluated cells, without mutating later windows
or exports. Literal overrides retain the cell's format and recompute `d`;
unformatted authored formulas render a General display:

```json
{"id":8,"op":"recalc","handle":"h1","overrides":[{"sheet":0,"r":1,"c":0,"v":10}]}
{"id":8,"ok":true,"changed":[{"sheet":0,"r":1,"c":0,"v":10.0,"d":null,"e":false},{"sheet":0,"r":1,"c":2,"v":13.0,"d":null,"e":true}],"evaluated":1,"skipped":0,"truncated":false,"warnings":[]}
```

The changed set is capped at 50,000 cells with `truncated:true` and a warning
when clipped. Export uses the same side-effect-free evaluation layer to write
fresh caches for covered downstream formulas; uncovered formulas are reported
in `dropped` and retain their file cache.

Under `sheetView`, every `open`/`meta` sheet entry carries `frozenRows` and
`frozenCols` as zero-based counts (`0` means no frozen pane). XLSX pane records
are guaranteed; XLS and XLSB frozen-pane records are read best-effort. Ordinary
split panes are not misreported as frozen panes.

Send `{"id":9,"op":"cancel","target":2}` to request cooperative cancellation
of in-flight request 2. Errors always use
`{"id":n,"ok":false,"code":"...","msg":"..."}`; v0 codes are `unsupported`,
`bad_zip`, `too_large`, `timeout`, `internal`, `bomb`, `bad_request`,
`bad_handle`, and `cancelled`. EOF and SIGTERM stop the server cleanly.

Consumers must check the `proto` in every successful `open` response. The
same protocol number appears in `wax --version`, so executable selection can
be rejected before incompatible data reaches the application.

## Compatibility harness

The harness compares wax with a pinned SheetJS oracle across a provenance-
tracked corpus, while recording timing, memory, window latency, and writer
read-back results. Disagreements are investigated rather than automatically
awarded to either implementation; accepted results live in
[`harness/adjudications.md`](harness/adjudications.md).

After the corpus has been fetched, install the oracle and run all or part of
the harness:

```bash
npm ci --prefix harness/oracle
harness/run.sh
harness/run.sh --limit 100 --jobs 4
```

The run rebuilds wax when `WAX_BIN` is unset and regenerates
[`SCOREBOARD.md`](SCOREBOARD.md) plus `harness/scoreboard.json`. See
[`MISSION.md`](MISSION.md) for the product rationale, fidelity contract, and
full build story.
