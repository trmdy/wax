# W5D review — oversized-string writer policy + harness classification

- **Shard:** w5d (`agent/wax-w5d-strings`, bee CO.64ad / wax-w5d)
- **Commit reviewed:** `3425147`
- **Reviewer:** coordinator CL.7c63, 2026-07-28
- **Verdict:** merge. No required fixes.

## What was reviewed

Full diff (8 files, +533/−44): writer truncation policy in
`crates/wax-write/src/lib.rs`, harness classification in
`aggregate.rs`/`roundtrip.rs`/`triage.rs`/`model.rs`, contract amendment in
`docs/w4-contracts.md` §2, and the test additions. `scripts/check.sh --fast`
re-run green by the coordinator in the shard worktree.

## Findings

1. **Truncation core is correct.** `truncate_xlsx_string` uses
   `char_indices().nth(32_767)` — cut lands on a char boundary, keeps exactly
   32,767 characters (not bytes), and computes the original length for the
   drop message without a second scan. Covered by tests at the exact
   boundary (32,767 kept verbatim), 32,768 (truncated), a multibyte char at
   the cut point, and a 🙂 (4-byte) cached-formula string.
2. **Cached-formula strings share one truncation.** The refactor computes
   `formula_result` once and feeds the same truncated value to both the
   in-sheet `Formula::set_result` and the post-save formula patch — the two
   paths cannot diverge.
3. **Drop reporting stays loud and bounded.** New `MAX_DROPPED_DETAILS`
   (100) cap with an explicit "N additional dropped entries omitted"
   summary line; per-cell messages are sheet-qualified
   (`'Sheet'!B1 string truncated from X to 32767 characters`).
4. **`internal` no longer masquerades as unavailability** (W4B finding 2):
   `export_xlsx` maps every `ok:false` response to a failed file; the
   unavailable classification is reserved for process-level evidence (stub
   `process_exit`), and `aggregate_round_trip` keeps the section `available`
   whenever a loud export failure exists. Unit test pins the exact mixed
   case. The W4B finding-1 fix (availability evidence = export stage
   succeeded) is preserved.
5. **W4B nits taken:** export `dropped` is parsed, stored per file
   (`export_dropped`), and rendered in a new triage section with a
   private-file count; `ColInfo` now has `deny_unknown_fields` + test.
6. **End-to-end proof:** both POI 46368.xls corpus files flip from
   `xlsxExportUnavailable` (before) to successful exports with one loud
   truncation drop each ('Sheet1'!A1, 32,770 → 32,767); they now
   participate in round-trip denominators as honest cell-level defects
   (6/8 values match; the truncated A1 mismatches by design).

## Notes / accepted deviations

- The two POI files are intentionally not round-trip-clean — the contract
  amendment (w4-contracts §2, W5 addendum) states export succeeds and the
  truncation is a visible defect, not a hidden one.
- Soffice was disabled for the targeted two-file proof; the full corpus
  numbers land with the coordinator's final scoreboard run.
