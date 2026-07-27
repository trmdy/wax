# Shard W1A — cargo workspace scaffold, `wax` CLI, stub reader

You are shard **W1A** of the wax v1 mission. Coordinator: bee **CL.661**.

**Required reading before any code:** `MISSION.md`, `docs/w1-contracts.md`
(the frozen contracts — §0 layout, §1 normalized dump, §2 CLI, §3 stub
reader are your spec). You work in this git worktree only, on branch
`agent/wax-w1a-scaffold`. Never touch `main`. There is no remote; commit
locally, the coordinator merges.

## Deliverables

1. **Workspace root** `Cargo.toml` with members `crates/wax-core`,
   `crates/wax-read`, `crates/wax-write`, `crates/wax-proto`,
   `crates/wax-cli`. Do **not** list `harness/wax-harness` as a member —
   another shard builds it standalone; the coordinator wires it in at
   integration. Edition 2021+, resolver 2. Pin a `rust-toolchain.toml`
   (stable, the version installed here — check `rustc --version`).
2. **`wax-core`**: the typed cell model matching contracts §1 exactly —
   cell `{r, c, t, v, d, f, fmt}` with type tags `n|s|b|e|d`, sheet
   (name, index, rows, cols, truncated, merges, cells), document
   (schema=1, tool, toolVersion, file, sha256, ok, error, wallMs,
   peakRssBytes, truncated, sheets, warnings). Serde serialization must
   produce the contract JSON byte-for-byte in field naming (camelCase
   where the contract says so; absent info = JSON `null`, never omitted).
3. **`wax-read`**: a `Reader` trait (file path + options `{max_cells,
   timeout_ms}` → normalized document) and `StubReader` implementing
   contracts §3: xlsx/zip only; sheet names + order from
   `xl/workbook.xml`; extents from `<dimension>` else computed; numeric
   cells and inline strings (`<is>`) only; cell cap with loud
   `truncated: true`; non-xlsx → `ok:false, code:"unsupported"`; the
   fixed stub warning string from §3 on every dump. Use the `zip` and
   `quick-xml` crates; do not hand-roll zip/xml parsing.
4. **`wax-proto`**: minimal — `pub const PROTO_VERSION: u32 = 0;` and an
   error-code enum covering at least `unsupported | bad_zip | too_large |
   timeout | internal` (string forms match the contract).
5. **`wax-write`**: placeholder lib crate that compiles (W4 fills it).
6. **`wax-cli`**: binary `wax`. `wax --version` → `wax <semver> (proto 0)`
   exit 0. `wax dump --json <file> [--max-cells N] [--timeout-ms N]` →
   one normalized-dump document on stdout, diagnostics to stderr, exit
   codes per contracts §2 (exit 0 even for `ok:false` documents; 2 for
   usage errors; 1 for aborts with nothing on stdout). Fill `wallMs`
   (parse wall time) and `peakRssBytes` via `getrusage` ru_maxrss
   (bytes on macOS, KiB on Linux — normalize to bytes; `null` if
   unavailable). Compute the input file's sha256.
7. **Tests**: unit tests for the model serde shape (round-trip + exact
   JSON field presence/nulls), stub-reader tests against small xlsx
   fixtures you generate/commit under `crates/wax-read/tests/fixtures/`
   (hand-built tiny zips are fine), cell-cap truncation test,
   unsupported-input test, CLI integration test (spawn the binary,
   parse stdout).

## Boundaries (do not touch)

`harness/`, `corpus/`, `scripts/`, `.github/`, `docs/`, `ASSIGNMENTS.json`,
`MISSION.md`, `.gitignore`. If you believe a contract is wrong, do not
work around it — buz the coordinator:
`hive buz send CL.661 --sender <your-bee-name> --tier queue -p "<msg>"`.

## Definition of done

- `cargo build --release` and `cargo test` green from the workspace root.
- `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` clean.
- `wax dump --json <some xlsx>` emits schema-valid JSON per contracts §1.
- Do not invest in making the stub good — W2 replaces it with calamine.
- All work committed on your branch; commit messages end with your bee name.

## Sealing

When done (or blocked), seal: `hive seal <your-bee-name> --from seal.json`
with status, summary, deliverables list, test counts (exact numbers from
`cargo test`), and any deviations from this brief or the contracts. Then
buz CL.661 that you've sealed.
