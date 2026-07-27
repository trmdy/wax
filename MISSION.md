# Mission — wax v1: the out-of-process sheet engine

- **Status:** mission live (2026-07-27). Coordinator-run multi-agent build.
- **Parent design:** apiary `docs/epics/sheets-and-docs.md` (+ §Spike results,
  2026-07-27) and Linear APIA-161. Apiary-side integration is explicitly
  **not** part of this mission.
- **Repo:** this repo (`~/Projects/trmd/wax/repos/wax`) is standalone. Apiary
  must never grow a Rust toolchain: wax ships as **prebuilt per-platform
  binaries** (macOS arm64/x64, Linux x64) from this repo's CI, consumed by
  Apiary like `nectar`/`rg` — PATH in dev, vendored `extraResources` in
  packaged builds, pinned by version + checksum. **No napi / node bindings,
  ever** — pure subprocess over stdio.

## Why (context for every bee)

Apiary is building a Sheet pane. The phase-1 spike (2026-07-27, results in
the apiary epic) found: SheetJS CE reads everything but ships from a vendor
CDN outside npm; ExcelJS failed 2/14 real files and drops cached formula
results; parsing a 40 MB xlsx costs 1.8 GB RSS — unacceptable inside
Electron main. wax fixes this properly: parsing happens in a short-lived,
sandbox-postured Rust subprocess that streams **windows** back over stdio
and dies. The interim JS baseline (SheetJS) stays until wax beats it on the
scoreboard; then Apiary swaps backends behind its `sheet:*` IPC seam.

## What wax is

A Rust workspace producing one CLI binary, `wax`, that wraps hardened
ecosystem crates and owns everything they don't do:

- **Read:** `calamine` (MIT — xlsx, legacy xls/BIFF, xlsb, ods) behind our
  normalization layer. **Write:** `rust_xlsxwriter` (MIT) behind our model
  mapping. Do not reimplement what these crates already harden.
- **Own code (the actual product):**
  1. **Protocol** — NDJSON over stdio: `open / meta / window / export /
     close`, plus caps, cancellation, timeouts, structured errors, protocol
     version in the `open` response.
  2. **Normalization** — one typed cell model: number/text/bool/error/date
     (1900/1904 epochs, serial→ISO), formula text + cached result, merges.
  3. **Number-format rendering** — an ECMA-376 format-code interpreter
     producing the display string Excel would show (`#,##0.00 kr` →
     `12 410,50 kr`): sections, conditionals, date/time codes, fractions,
     percentages, text sections, locale-aware separators. Biggest single
     compatibility surface; gets its own shard and its own test corpus.
  4. **Windowed store** — parse once into a compact columnar representation
     (typed columns + string table) so a 5M-cell sheet holds ~100–200 MB and
     `window` is O(window). Loud truncation metadata, never silent.
  5. **Safety rails** — zip-ratio bomb checks, per-part size caps, XML
     entity/depth limits, wall-clock parse timeout, fuzz targets
     (cargo-fuzz) on the container + XML paths.
  6. **Writer mapping** — model → styled xlsx (export-a-copy semantics) and
     CSV. No in-place editing of arbitrary files, ever.
  7. **The harness** — corpus tooling, differential oracle, round-trip
     checks, scoreboard. The harness outlives any parser choice.

## Non-goals (v1)

- No formula evaluation. Cached results only.
- No chart/pivot/drawing fidelity — presence noted in `meta`, content
  ignored; the writer never pretends to preserve them.
- No in-place mutation of existing xlsx.
- No napi/wasm builds. No Windows target yet (revisit post-v1).

## Protocol v0 (wave 3 finalizes; sketch is binding in spirit)

One NDJSON object per line, `id`-correlated, big payloads chunked:

```jsonc
→ {"id":1,"op":"open","path":"/abs/file.xlsx","maxBytes":52428800}
← {"id":1,"ok":true,"proto":0,"handle":"h1","sheets":[{"name":"Costs","rows":195114,"cols":12,"truncated":false}],"warnings":["charts ignored (2)"]}
→ {"id":2,"op":"window","handle":"h1","sheet":0,"r0":0,"c0":0,"nr":64,"nc":24}
← {"id":2,"ok":true,"cells":[[/* row-major: {v,t,f?,d?,fmt?} — d = display string */]],"merges":[...]}
→ {"id":3,"op":"export","handle":"h1","format":"xlsx","out":"/abs/copy.xlsx"}
← {"id":3,"ok":true,"bytes":183220,"dropped":["pivot caches"]}
```

Errors: `{"id":n,"ok":false,"code":"too_large|bad_zip|timeout|...","msg":"…"}`.
`wax --version` prints semver + proto number. One process may hold multiple
handles; idle handles expire; SIGTERM is clean.

## The corpus and the oracle

- **Corpus** (`corpus/` — gitignored payloads, committed manifest with
  source + sha256 per file): SheetJS test-file suite, openpyxl test data,
  Apache POI test data, govdocs1 office subset, SEC EDGAR xlsx exhibits,
  data.gov exports, plus the 16 spike files already on this machine
  (`~/Projects/_tmp/sheets-spike/bench-all.sh` lists them). Target ≥1,000
  distinct real files; record provenance; public/test files only.
- **Oracle:** a node runner using SheetJS 0.20.x (CDN tarball, pinned)
  emitting per-file JSON ground truth (values, types, formulas, cached
  results, merges, dims). Disagreements are adjudicated per-file: sometimes
  wax is right and SheetJS is wrong — record verdicts in
  `harness/adjudications.md`, never auto-lose.
- **Writer validation:** read-back differential (wax write → wax + SheetJS
  read → compare) and, when LibreOffice is installed, `soffice --headless`
  conversion as a does-it-open check.

## Scoreboard (the objective function)

`SCOREBOARD.md`, regenerated by `harness/run.sh`, committed with each wave
seal. Columns vs the SheetJS baseline: files opened %, cell-value match %,
display-string coverage %, formula/cached fidelity %, p50/p95 parse time,
peak RSS, window latency. **Swap gate for Apiary:** wax ≥ SheetJS on
opens/values, display-string coverage ≥95% on corpus formats, and better
p95 time + RSS. No gate is ever relaxed silently — deviations go in the
seal.

## Waves

- **W1 — foundations (gate for everything):** cargo workspace scaffold
  (`wax-core`, `wax-read`, `wax-write`, `wax-proto`, `wax-cli`, `harness/`),
  corpus fetch + manifest (≥1,000 files), SheetJS oracle runner, differential
  harness executing end-to-end with a stub reader, CI (fmt/clippy/test).
  *Gate: `harness/run.sh` produces a real SCOREBOARD.md against the stub.*
- **W2 — reader + normalization + number formats:** calamine integration,
  cell model, epochs/dates, the format interpreter (own shard + tests).
- **W3 — protocol + windowed store + safety rails:** stdio server, columnar
  store, caps/timeouts/bomb-checks, fuzz targets seeded.
- **W4 — writer:** model → styled xlsx + csv, round-trip + soffice checks.
- **W5 — hardening + release:** fuzz burn-in, corpus triage to green,
  release CI producing signed per-platform tarballs + checksums, README.
  *(Apiary integration happens after this mission, behind `sheet:*`.)*

## Swarm rules

- Coordinator owns `ASSIGNMENTS.json` (shard → bee → branch → state) and
  merges to `main`; shards work on `agent/wax-w<N><letter>-<slug>` branches
  and **seal** (`hive seal`) with: deliverables, test counts, scoreboard
  delta, deviations. ≤8 bees concurrent; spawn with `--account auto`.
- Every shard writes tests for its logic; clippy + fmt clean; no shard
  touches another shard's crate without coordinator sign-off.
- Corpus downloads are polite (rate-limited, resumable, provenance-logged).
- Nothing in this mission touches the apiary repo.
- Blockers surface as seals/buz to the coordinator, not silent stalls;
  the coordinator seals a wave summary (scoreboard attached) per wave.
