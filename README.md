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
version=0.1.0
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

The final command prints `wax 0.1.0 (proto 0)` for the v0.1.0 release. Move
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
- `export` writes an XLSX or CSV copy and reports every dropped feature.
- `close` releases a handle.

This is a representative v0 session (one object per line):

```json
{"id":1,"op":"open","path":"/absolute/path/book.xlsx","maxCells":5000000,"maxBytes":104857600,"timeoutMs":30000}
{"id":1,"ok":true,"proto":0,"handle":"h1","truncated":false,"sheets":[{"name":"Sheet1","rows":2,"cols":2,"truncated":false}],"warnings":[]}
{"id":2,"op":"window","handle":"h1","sheet":0,"r0":0,"c0":0,"nr":2,"nc":2}
{"id":2,"ok":true,"sheet":0,"r0":0,"c0":0,"nr":2,"nc":2,"rows":[[{"t":"s","v":"Item","d":"Item"},{"t":"s","v":"Cost","d":"Cost"}],[{"t":"s","v":"Tea","d":"Tea"},{"t":"n","v":12.5,"d":"12.50"}]],"merges":[]}
{"id":3,"op":"close","handle":"h1"}
{"id":3,"ok":true}
```

An `open` defaults to 5,000,000 cells, 100 MiB input, and a 30-second
wall-clock timeout. A `window` is capped at 262,144 requested cells; the
server defaults to 16 handles with a five-minute idle timeout. Limits and
reader safety rails fail loudly or mark a result as truncated.

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
