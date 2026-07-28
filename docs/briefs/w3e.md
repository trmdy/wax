# Shard W3E — reader carry-overs (capacity lane)

You are shard **W3E** of the wax v1 mission, wave 3. Coordinator: bee
**CL.d73**.

**Required reading before any code:** `MISSION.md`, `docs/w1-contracts.md`
§1, `docs/w2-contracts.md` §1, `docs/w3-contracts.md` §5 (your spec) and
§0 (the wax-read file-level lane split with W3C). You work in your git
worktree only (`.worktrees/w3e`), on branch `agent/wax-w3e-reader`. Never
touch `main`. No remote; commit locally, the coordinator merges.

**Standing:** you are the sanctioned-as-capacity-allows lane. Core W3
(A–D) outranks you at merge time and for coordinator attention. Apiary v1
ships xlsx-only, so nothing here gates the wave — your job is honest
whole-corpus movement.

## The job — three items, strict priority order

1. **xls/xlsb per-cell number-format codes.** The single biggest
   whole-corpus display-match lever: xls display match is 74.17% and xlsb
   57.50%, and W2 adjudication proved the formatter is right when it gets
   the code — the reader just never extracts `fmt` for those containers,
   falling back to General.
   - xls: BIFF `FORMAT` (0x041E) + `XF` (0x00E0) records →
     numFmtId → code per cell. Check calamine's public API first (it
     parses XFs internally for dates); only parse the OLE2/BIFF stream
     yourself if the API can't give you cell→XF→fmt. A small extra dep
     (e.g. `cfb`) is acceptable — note it in the seal.
   - xlsb: parse `xl/styles.bin` (`BrtFmt`, `BrtXF`) from the zip
     directly, mirroring the existing xlsx `xl/styles.xml` path in
     `calamine_reader.rs`. Builtin numFmtId table (0–49) applies to both.
   - `d` still comes exclusively from `wax_fmt::render` on the raw value.
     Never hand-roll display strings.
2. **xlsb value-match investigation** (73.89% vs xls 98.11%): triage
   mismatching files via `harness/triage.md` buckets + `harness/run.sh
   --limit` slices; find the systematic causes; fix ours; where SheetJS
   is wrong, append evidence-backed verdicts to
   `harness/adjudications.md`.
3. **xlsx open-failure triage** (37 files): classify password-protected
   (proper structured error), nonstandard part names / strict OOXML
   (fix if cheap), corrupt (adjudicate). Don't sink days into single
   weird files — classification with evidence is itself the deliverable.

Expected movement (measure, don't force): xls display match ≥90%, xlsb
≥85%, whole-corpus display ≥85%. Honest numbers beat target-hitting; if
something is structurally out of reach, the seal says why with evidence.

## Lane discipline (shared crate with W3C)

You own `crates/wax-read/src/calamine_reader.rs` + any new modules you
add (e.g. `xls_styles.rs`, `xlsb_styles.rs`). W3C concurrently owns
`crates/wax-read/src/safety.rs` + small `lib.rs` hooks. Touch `lib.rs`
only for `mod` lines and additive plumbing; anything that could collide
with W3C, buz CL.d73 first. Merge conflicts in `lib.rs`/`Cargo.lock` are
the coordinator's to resolve — keep your edits surgical.

## Tests

Fixture-driven unit tests for BIFF FORMAT/XF parsing and BrtFmt/BrtXF
parsing (small crafted or corpus-derived public fixtures committed under
the crate's test data if tiny, else generated in-test); regression tests
for each xlsb value-match fix; per-fix before/after scoreboard slices
(`harness/run.sh --limit`) recorded in the seal. Existing wax-read tests
stay green.

## Boundaries (do not touch)

`crates/wax-read/src/safety.rs` (W3C), `crates/wax-store/**`,
`crates/wax-proto/**`, `crates/wax-cli/**`, `crates/wax-fmt/**` (consume
`render` only), `harness/wax-harness/**`, `harness/run.sh`,
`harness/oracle/**`, `corpus/**` payloads, `scripts/**`, `.github/**`,
`docs/**`, `ASSIGNMENTS.json`. `harness/adjudications.md` is append-only
for your verdicts.

## Definition of done

- `scripts/check.sh` fully green in your worktree.
- Item 1 landed with measured display-match movement; items 2–3 as far as
  honest progress reaches — a partial with evidence beats a stretched
  claim.
- Commit messages end with your bee name.

## Sealing

`hive seal <your-bee-name> --from seal.json` with status, summary,
deliverables, exact test counts, before/after per-extension numbers, dep
additions, adjudication entries, and deviations. Then
`hive buz send CL.d73 --sender <your-bee-name> --tier queue -p "sealed"`.
Blockers → buz immediately; never a silent stall.
