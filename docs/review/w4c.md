# Review — W4C: serve/CLI export wiring (`agent/wax-w4c-export`, 51c63f8)

Reviewer: CL.6cbf (coordinator, agent-assisted deep review with live
probes). Verdict: **merge-with-notes**.

## Scope check

Diff against merge base (15fcffe, the W4 scaffold) touches only
`Cargo.lock` + `crates/wax-cli/**` — exactly the assigned lane.
`crates/wax-proto`: **zero** diffs (verified; `ErrorCode::from_code`
pre-existed on main). `scripts/`/`.github/`: no changes — correctly none
needed, CI is green on origin/main and the CI duty is reactive.
`git merge-tree` reports a clean merge; only the docs-only §2 amendment
landed on main after the branch point.

## What's there

- **serve `export` xlsx live**: op shape unchanged; format parsed
  case-insensitively; format-before-sheet validation order preserved;
  `sheet` (default 0) range-validated for both formats before dispatch,
  ignored by `write_xlsx`; runs on the existing worker/cancel thread
  infrastructure; `WriteError.code` → `ErrorCode::from_code` with
  `Internal` fallback, `msg` passed through verbatim.
- **Warnings appended to `dropped`** for both formats via a per-dispatch
  `HandleSnapshot.warnings` clone (no accumulation across repeated
  exports); pinned by `export_appends_open_warnings_to_dropped` — the
  warnings land *after* the writer's drops, matching the contract's
  "appended".
- **`wax export` subcommand**: contract signature verbatim in `--help`;
  reader options field-for-field identical to `dump`
  (`read_with_deadline` + max-cells/max-bytes/timeout-ms); store built,
  sheet pre-validated (mirrors serve), writer called, one flat JSON
  line, exit 0 for both reader- and writer-failures (reader failures
  reuse the reader's code); usage errors exit 2 with empty stdout;
  `--version` untouched.
- **serve CSV hardened**: sibling `NamedTempFile` + `persist` (atomic
  rename, same filesystem by construction), entry + pre-persist cancel
  checkpoints — cancelled/failed exports leave no file (new unit test).
  Wire behavior unchanged.
- **Stub sanity**: xlsx over serve returns the structured stub
  `internal`; end-to-end success tests staged behind `#[ignore]` for the
  W4A flip.

## W4B interop (checked against `agent/wax-w4b-roundtrip`)

Both availability probes in `harness/wax-harness/src/roundtrip.rs`
match: stub failure `code == "internal"` → `xlsxExportUnavailable`;
pre-merge binary stderr `` wax: unknown command `export` `` satisfies
`export_cli_is_unavailable`. Success shape (one line, `bytes`, `dropped`
array, exit 0) satisfies W4B's schema checks.

## Tests

9 unit + 10 CLI + 11 serve passing, 2 ignored staged success-path tests
(32 defined). `scripts/check.sh` fully green in a clean worktree. The
ignored tests tolerate the amended §2: neither pins `dropped` contents
(extra entries for unrepresentable merges/names/widths won't break
them), and the fixture's merges (`A3:B3`, `D3:F4`) are both
xlsx-representable.

## Findings (non-blocking, tracked)

1. **CSV exports now land mode 0600** (tempfile default) instead of the
   umask-derived mode of the old `File::create`, and `persist` over an
   existing file replaces its permissions. Fine for Apiary (same user);
   → W4A picks one deliberate permissions convention for the shared
   temp+rename pattern in `wax-write` (requested as part of its
   fast-follow).
2. **`wax export --format csv` is dead until W4A** — it delegates to the
   stubbed `wax_write::write_csv` (structured `internal`, exit 0), while
   serve keeps the working local CSV. Contract-conformant sequencing;
   the one-commit swap plan stands.
3. `wax export` JSON key order is alphabetical (`serde_json::json!`
   BTreeMap) vs `dump`'s `ok`-first struct order. Not semantic; nit.
4. Zero-sheet workbook + xlsx via CLI says `sheet index 0 is out of
   range` rather than §2's "empty workbook" — same `bad_request` code,
   writer owns the canonical wording post-W4A.
5. xlsx unwritable-out is untestable against the stub (never touches the
   fs); covered for CSV now, owed by W4A's writer tests for xlsx.

## Deviations from seal

None found; test counts, check.sh, and contract adherence verified
independently in a detached worktree at 51c63f8.

## Fix log

- 2026-07-28 (swap sealed, commit 0353baa): CSV delegation swap landed —
  serve's local CSV encoder/temp-persist deleted (−160 lines), export
  delegates to `wax_write::write_csv` through the writer error adapter,
  dropped assertions updated to the canonical five entries, and both
  staged xlsx success-path tests un-ignored and passing against the real
  writer (32 wax-cli tests, full workspace 191). Coordinator had earlier
  flipped the two stub-pinned tests at the W4A merge (cdedc89).
