# Shard W3C — safety rails + fuzz targets

You are shard **W3C** of the wax v1 mission, wave 3. Coordinator: bee
**CL.d73**.

**Required reading before any code:** `MISSION.md`, `docs/w1-contracts.md`
§1–2, `docs/w3-contracts.md` §3 (your spec) and §0 (the wax-read
file-level lane split with W3E). You work in your git worktree only
(`.worktrees/w3c`), on branch `agent/wax-w3c-safety`. Never touch `main`.
No remote; commit locally, the coordinator merges.

## The job

wax parses hostile files inside a user's desktop app. Build the rails,
per `docs/w3-contracts.md` §3:

1. **Container preflight** in `crates/wax-read/src/safety.rs` (new file —
   that's your lane; `lib.rs` gets only small hooks; W3E owns
   `calamine_reader.rs` concurrently, coordinate via buz for anything
   beyond a `mod safety;` line). Zip formats (xlsx/xlsm/xlsb/ods): entry
   count cap 10,000; per-part decompressed cap 512 MiB; total decompressed
   cap 2 GiB; ratio bomb check (>100:1 AND >10 MiB → `ErrorCode::Bomb`);
   input-size cap via a new additive `ReaderOptions::max_bytes` (default
   100 MiB, CLI flag `--max-bytes` on dump). Preflight runs before
   calamine sees a byte. Structured `ok:false` errors, never panics.
2. **XML guards** on wax's own quick-xml paths (the styles/xlsx parsing in
   wax-read): depth cap 256, DOCTYPE/internal-DTD rejection → `bomb`,
   token/buffer caps. Add a test pinning that quick-xml does not expand
   custom entities, so a dependency upgrade can't silently regress it.
3. **Wall-clock timeout that actually fires**: `wax dump --timeout-ms N`
   yields an `ok:false` `timeout` document within ~2×N even when calamine
   is stuck. Implementation latitude (watchdog thread; abandoned worker
   acceptable v0). Shape it as a reusable
   `wax_read::read_with_deadline(...)`-style wrapper — W3A's serve wants
   the same mechanism; announce the signature via buz to CL.d73 early so
   W3A can align. (The CLI `--timeout-ms` flag exists; today it's only
   checked between sheets.)
4. **cargo-fuzz** under `fuzz/` (own manifest, excluded from the
   workspace, standard cargo-fuzz layout): targets for (a) container
   preflight on arbitrary bytes, (b) xlsx path through `CalamineReader`
   (bytes → temp file → read with tight caps + short timeout), (c) one
   legacy path (xls or xlsb). Seed corpora: small public corpus files
   (<50 KiB) committed under `fuzz/corpus/<target>/`. Run each target
   locally for a real burn (≥15 min each); crashes: fix if ours, minimal
   repro + protective cap if calamine's — either way in the seal.
5. **Wiring**: `scripts/check.sh` gains a fuzz stage — nightly +
   cargo-fuzz available → `cargo fuzz build` + `-max_total_time=30` smoke
   per target; else a loud SKIP. `.github/workflows/ci.yml` gains a job
   installing nightly + cargo-fuzz and running that stage. Keep existing
   check.sh stages byte-identical for existing users.

Also: the corpus has a legacy xls that drives wax peak RSS to ~1.0 GiB
(see `harness/results.jsonl`). W3B is profiling it store-side; if the
memory is calamine-transient, the fix lands here as a cap/limit. Watch
for their buz via the coordinator.

## Tests

Unit tests per rail: crafted zip bombs (high-ratio deflate), overlong
entry counts, oversized parts, DOCTYPE payloads, deep XML nesting, a
timeout fixture (a large/pathological file or generated one), plus
regression tests for every fuzz finding you fix. `ReaderOptions` default
changes must not alter existing green-path corpus behavior — run a quick
sanity slice (`harness/run.sh --limit 200`) before sealing and put the
delta in the seal (expect zero regressions; a bomb-check catching a real
corpus file is a *finding*, not a regression — adjudicate it).

## Boundaries (do not touch)

`crates/wax-read/src/calamine_reader.rs` (W3E's lane — buz for hooks),
`crates/wax-store/**`, `crates/wax-proto/**`, `crates/wax-cli/**` beyond
the `--max-bytes` flag plumbing, `crates/wax-fmt/**`,
`harness/wax-harness/**`, `harness/run.sh`, `corpus/**`, `docs/**`,
`ASSIGNMENTS.json`. You own `crates/wax-read/src/safety.rs`, small
`lib.rs` hooks, `fuzz/**`, `scripts/check.sh`, `.github/workflows/**`.

## Definition of done

- `scripts/check.sh` fully green (including your new stage's skip path on
  stable-only machines).
- Rails implemented with tests; fuzz targets built, seeded, burned ≥15 min
  each locally, wired into check.sh + CI.
- Commit messages end with your bee name.

## Sealing

`hive seal <your-bee-name> --from seal.json` with status, summary,
deliverables, exact test counts, fuzz burn results (execs, findings,
fixes), the harness sanity-slice delta, and deviations. Then
`hive buz send CL.d73 --sender <your-bee-name> --tier queue -p "sealed"`.
Blockers → buz immediately; never a silent stall.
