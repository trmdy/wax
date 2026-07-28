# SheetJS oracle

This directory contains wax's independent SheetJS baseline. Each invocation
starts a fresh Node process, reads one spreadsheet, and emits exactly one
schema-1 normalized dump on stdout.

## Install and run

Node 20 or newer is required.

```sh
cd harness/oracle
npm ci
npm test
node run.js ../../corpus/files/example.xlsx
node run.js ../../corpus/files/example.xlsx --max-cells 200000 --timeout-ms 30000
```

`--max-cells` defaults to 200,000 across all sheets. `--timeout-ms` has no
implicit default; when supplied, parsing runs in a worker thread so the main
process can terminate a stuck parser and return a structured `timeout` result.
Usage errors exit 2. Read/parse failures are corpus data points: they emit an
`ok:false` document and exit 0. An internal abort exits 1 without stdout.

`schema.js` exports `validateNormalizedDump` and `assertNormalizedDump` for
the differential harness and tests.

## Why the dependency uses a URL

SheetJS Community Edition 0.20.3 is pinned directly to the vendor CDN tarball:

```text
https://cdn.sheetjs.com/xlsx-0.20.3/xlsx-0.20.3.tgz
```

The npm registry package named `xlsx` is stale and is intentionally not used.
The exact resolved URL and SHA-512 integrity digest are recorded in
`package-lock.json`; the 0.20.3 digest is
`sha512-oLDq3jw7AcLqKWH2AhCpVTZl8mf6X2YReP+Neh0SJUzV/BdZYjth94tG5toiMB1PPrYtxOCfaoUCkvtuH+3AJA==`.
Use `npm ci`, not an unpinned install.

## Mapping decisions

- The reader uses `cellDates: true`, `cellNF: true`, and `cellText: true`.
  SheetJS therefore applies the workbook's 1900/1904 epoch when it recognizes
  a date-formatted serial. Only SheetJS cells returned as `t:"d"` become wax
  dates; ambiguous numeric serials remain `t:"n"`. JavaScript dates are emitted
  as timezone-free ISO calendar values (`YYYY-MM-DD` at midnight, otherwise
  `YYYY-MM-DDTHH:mm:ss[.sss]`) because spreadsheet dates have no timezone.
- Cached formula results stay in `v`/`t`; formula text comes from `.f` with a
  defensive leading `=` removal. A missing cached result is `null`.
- Display strings come from `.w`. Number formats come from `.z`, with missing,
  empty, and `General` formats mapped to `null`.
- Error cells use SheetJS's formatted error text, falling back to the standard
  error-code mapping. Text formula results (`t:"str"`) normalize to `t:"s"`.
- Sheet extents use the bottom-right coordinate of `!ref`, yielding 1-based
  row and column counts. Cells are sparse and sorted row-major. Merges are
  encoded as A1 ranges and sorted lexically.
- `process.resourceUsage().maxRSS` is reported in KiB by Node and multiplied
  by 1024 for `peakRssBytes`.

The test fixture is generated locally by `test/generate-fixtures.js` with the
pinned SheetJS build. This keeps binary artifacts out of Git while making the
test suite deterministic and network-free after `npm ci`.
