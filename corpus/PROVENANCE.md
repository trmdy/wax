# Corpus provenance

The payload tree under `corpus/files/` is intentionally gitignored. Every
payload is represented by one sorted record in `manifest.jsonl`, including its
exact source URL, SHA-256 digest, byte size, acquisition time, licence label,
and privacy status. `manifest-summary.json` is derived from those records.

The fetcher identifies itself as
`wax-corpus-fetch/1.0 (tormod.haugland@gmail.com)`. HTTP downloads are
sequential and rate-limited to at most two requests per second per host. Curl
uses resumable `.part` files and bounded exponential retry/backoff. Git sources
use one shallow, blob-filtered, sparse checkout at a pinned commit. Existing
payloads with matching SHA-256 are reused.

Files over 30 MiB and extensions outside `xlsx`, `xlsm`, `xlsb`, `xls`,
`ods`, `csv`, and `tsv` are excluded. Runtime skips are recorded in the
gitignored `corpus/fetch.log`. The public payload budget is capped at 4 GiB.

## SheetJS test files

- Collection id: `sheetjs-test-files`
- Upstream: <https://github.com/SheetJS/test_files>
- Fetch source: the `test_files/` directory in the official
  <https://github.com/SheetJS/SheetJS.github.io> site repository, pinned at
  commit `7d4614945c6a652421b66aa536fd0140a3ff3e4f`
- Licence: Apache-2.0 (the SheetJS repositories' declared licence)
- Method: one shallow, blob-filtered, sparse Git checkout; eligible files are
  copied locally and attributed to commit-pinned raw URLs
- Snapshot date: 2026-07-28
- Included: 1,189 files, 62,921,236 bytes
- Deviation: GitHub currently disables anonymous smart-HTTP and archive/API
  access for `SheetJS/test_files` itself (`401`, “Repository access blocked”).
  The official SheetJS website mirror is used instead. The coordinator was
  notified by buz before implementation.
- Exclusion: `large_strings.xls` is 58,245,120 bytes and exceeds the 30 MiB
  per-file cap.

## Apache POI spreadsheet test data

- Collection id: `poi-test-data`
- Upstream: <https://github.com/apache/poi/tree/trunk/test-data/spreadsheet>
- Licence: Apache-2.0
- Method: one shallow, blob-filtered, sparse Git checkout pinned at commit
  `0c5d8675e124cdfb4c147963135c9ba35fcfb009`; manifest sources are exact
  commit-pinned raw URLs
- Snapshot date: 2026-07-28
- Included: 793 files, 65,595,513 bytes
- Exclusions: non-spreadsheet test-data directories, unsupported extensions,
  and files over 30 MiB. No spreadsheet file hit the size cap in this
  snapshot.

## OpenPyXL test data

- Collection id: `openpyxl-test-data`
- Upstream: <https://foss.heptapod.net/openpyxl/openpyxl>
- Archive: OpenPyXL 3.1.5 tag archive, SHA-256
  `64a599aeed98b74925dcc09a18c7b3e19dafb3754eb8bad2b6887b63a91f7a37`
- Licence: MIT
- Method: one checksum-pinned archive download; only spreadsheet files from
  the extracted source/test tree are copied
- Snapshot date: 2026-07-28
- Included: 38 files, 1,260,164 bytes
- Exclusions: source code, non-spreadsheet fixtures, unsupported extensions,
  and files over 30 MiB. No spreadsheet file hit the size cap in this
  snapshot.

## SEC EDGAR generated financial reports

- Collection id: `sec-edgar`
- Upstream: <https://www.sec.gov/Archives/edgar/>
- Discovery index:
  <https://www.sec.gov/Archives/edgar/full-index/2025/QTR2/master.idx>
- Licence/terms: `US-PD`; these `Financial_Report.xlsx` artifacts are
  SEC-generated public dissemination files. EDGAR fair-access policy applies.
- Method: deterministically inspect 10-K/10-Q accessions from the immutable
  quarterly index and retain the first 24 available generated reports. Each
  report is downloaded from and attributed to its exact accession URL.
- Snapshot date: 2026-07-28
- Included: 24 files, 2,663,412 bytes
- Politeness: declared contact-bearing user agent, sequential requests, at
  most two requests per second, resumable transfers, bounded retry/backoff
- Exclusions: unavailable accessions, non-XLSX filing artifacts, and files
  over 30 MiB. Five deterministic candidate URLs returned 404 before the
  target of 24 available reports was reached.

## Local spike files

- Collection id: `spike-local`
- Source list:
  a local, uncommitted spike overlay on the operator machine (`corpus/manifest.local.jsonl`, gitignored; regenerated via `WAX_SPIKE_SCRIPT`)
- Licence/status: private, local-only accounting and synthetic spike inputs
- Method: parse the static `FILES` list without executing it; include only
  paths that exist and are readable within a five-second hash deadline,
  reference each by absolute path, and mark `private: true`
- Distribution: never copied into `corpus/files/`, committed, uploaded, or
  included in a published artifact. Missing, offline/unreadable, and over-30
  MiB private files are skipped and recorded in the local fetch log.
- Included: 4 readable references, 195,397 bytes. Eleven metadata-visible
  files in `~/Downloads` did not become readable within five seconds and one
  40,457,382-byte synthetic workbook exceeded the cap.

## Excluded collections

`govdocs1` and `data.gov` are not fetched in this snapshot because the pinned
public test collections already exceed the 1,000-distinct-file target.
Avoiding a top-up prevents unnecessary crawling and download volume.

The final public manifest contains 2,044 entries, 1,587 distinct SHA-256
payloads, and 132,440,325 bytes.
