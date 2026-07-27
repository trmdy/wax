# Shard W1B — corpus fetch + manifest (≥1,000 files) + provenance

You are shard **W1B** of the wax v1 mission. Coordinator: bee **CL.661**.

**Required reading:** `MISSION.md` (§The corpus and the oracle), 
`docs/w1-contracts.md` §4 (manifest schema — binding). Branch
`agent/wax-w1b-corpus`, this worktree only. No remote; commit locally.

## Deliverables

1. **`corpus/fetch.sh`** (may delegate to helper scripts/node under
   `corpus/`): idempotent + resumable fetcher that populates
   `corpus/files/<collection>/…` (gitignored) and writes the manifest.
   Politeness is non-negotiable: identify yourself with UA
   `wax-corpus-fetch/1.0 (tormod.haugland@gmail.com)`, rate-limit
   (≤2 req/s per host; SEC EDGAR hard-requires a declared UA — respect
   their fair-access rules), retry with backoff, resume partial runs
   without refetching (skip files already present with matching sha256).
2. **Collections** (target **≥1,000 distinct real files** total):
   - `sheetjs-test-files`: shallow-clone github.com/SheetJS/test_files
     (one clone = politest fetch; several hundred files, many formats).
   - `poi-test-data`: Apache POI spreadsheet test data
     (github.com/apache/poi, `test-data/spreadsheet/` — sparse/shallow
     clone).
   - `openpyxl-test-data`: openpyxl's bundled test workbooks (e.g. from
     the PyPI sdist tarball or heptapod archive download).
   - `sec-edgar`: Financial_Report.xlsx exhibits from EDGAR filings
     (public domain) — enough to help clear 1,000 total.
   - `data-gov` and/or `govdocs1` office subset: optional top-up if
     needed to reach the target.
   - `spike-local`: the 16 spike files listed in
     `~/Projects/_tmp/sheets-spike/bench-all.sh` — **`private: true`**,
     referenced by absolute local path, NOT copied into `corpus/files/`,
     never committed/uploaded (real personal/business data). Manifest
     entries only, and only for files that exist on disk.
   Skip files >30 MB (log the skip); keep total corpus ≤ ~4 GB. Only
   spreadsheet-ish extensions: xlsx, xlsm, xlsb, xls, ods, csv, tsv.
3. **`corpus/manifest.jsonl`** — exactly the contracts §4 schema, one
   object per line, sorted by `id`, sha256 + bytes + source URL +
   licence + fetchedAt for every entry. Committed.
4. **`corpus/manifest-summary.json`** — total count, per-collection
   count + bytes, per-extension histogram, generation timestamp.
   Committed.
5. **`corpus/PROVENANCE.md`** — per collection: what it is, exact
   source, licence/terms notes, fetch method, date, any exclusions.
6. **Validation tooling**: `corpus/verify.sh` — re-hash present files
   against the manifest, report missing/mismatched, exit non-zero on
   mismatch. Manifest-generation logic that is non-trivial gets tests
   (a small node/shell test is fine; keep it runnable offline).

## Boundaries (do not touch)

`crates/`, `harness/`, `scripts/`, `.github/`, `docs/`, root files.
Public/test files only — when in doubt about licence/terms, leave it out
and note why in PROVENANCE.md. Questions → buz coordinator:
`hive buz send CL.661 --sender <your-bee-name> --tier queue -p "<msg>"`.

## Definition of done

- `bash corpus/fetch.sh` from a clean checkout reaches ≥1,000 manifest
  entries with real downloaded payloads (spike entries excluded from the
  1,000 count), then `bash corpus/verify.sh` passes.
- Manifest lines all validate against contracts §4 (write a tiny
  validator; run it in verify.sh).
- Committed manifest contains no private payload copies and no
  `corpus/files/` binaries; commits end with your bee name.

## Sealing

Seal (`hive seal <your-bee-name> --from seal.json`) with: per-collection
file counts + bytes, total corpus size, how politeness was implemented,
skips/exclusions, deviations. Then buz CL.661.
