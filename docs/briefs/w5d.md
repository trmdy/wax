# Shard W5D — oversized-string writer policy + harness classification

You are shard **W5D** of the wax v1 mission, wave 5 (hardening + release).
Coordinator: bee **CL.7c63**.

**Required reading before any code:** `docs/w4-contracts.md` §2 (writer
contract + its loud-degradation amendments), `docs/review/w4b.md`
(findings 1–2 and the nits — finding 2 is half your job),
`crates/wax-write/src/` (dropped-reporting conventions),
`harness/wax-harness/src/aggregate.rs` (round-trip availability logic).
You work in your git worktree only (`.worktrees/w5d`), on branch
`agent/wax-w5d-strings`. Never touch `main`; never `git push`. Commit
locally, the coordinator merges.

## The job

### 1. Writer: truncate-with-loud-drop for >32,767-char strings

rust_xlsxwriter rejects cell strings over Excel's 32,767-**character**
limit, which currently fails the whole export for 2 corpus files (POI
`46368.xls` twice). New policy (append it to `docs/w4-contracts.md` §2 as
a W5 amendment):

- Text cells (and cached string results) longer than 32,767 chars are
  truncated to exactly 32,767 characters (character count, not bytes — no
  splitting inside a char; explicitly handle the formula-cached-string
  case too if it can exceed the limit).
- Every truncation produces a `dropped` entry following the existing
  deduped/bounded conventions (cell ref + original length; collapse to a
  count past the existing cap).
- The export **succeeds**. CSV export keeps full fidelity — no CSV
  truncation.

### 2. Harness: stop conflating `internal` with export-unavailable

W4B finding 2: today any `code:"internal"` export response counts as
"xlsx export unavailable" and can flip round-trip sections to n/a. Now
that the writer is real, `internal` is a live-writer regression and must
be *loud*, not n/a:

- Availability evidence stays what finding 1's fix made it (files whose
  export stage actually succeeded — verify that fix is on main; re-fix if
  not).
- An `internal` export failure counts as a failed (not-clean) round-trip
  file, visible in triage with its error, never as unavailability.
- While you are in there, the W4B nits are in scope if cheap: surface the
  export `dropped` list in round-trip triage output (it is your
  truncation signal), add the omitted private-file count to the oracle
  read-back triage section, `deny_unknown_fields` on `ColInfo`.

### 3. Prove it end-to-end

Run the harness over a subset that includes the two POI 46368 files
(`harness/run.sh` full run without `--soffice` is fine, or a targeted
run if the harness supports file filters): both files must now export
clean with loud truncation drops, be counted in round-trip denominators,
and the "export unavailable" classification must be gone. Before/after
numbers go in the seal.

## Tests

Writer: truncation at exactly the boundary (32,767 ok / 32,768 truncated),
multi-byte char at the cut point, dropped-entry content + dedup cap,
cached-string formula case, CSV untouched. Harness: `internal` ⇒ failed
file not n/a, availability evidence unchanged for genuinely stub/absent
exports, plus tests for any nit you take.

## Boundaries (do not touch)

Your lane: `crates/wax-write/**`, `harness/wax-harness/**`,
`harness/run.sh` (only if needed), `docs/w4-contracts.md` (§2 amendment,
append-only). Not `crates/wax-read/**` (W5A/W5B), not `crates/wax-cli`
serve code, not `.github/**`/`scripts/**` (W5C), not
`harness/adjudications.md` (W5B). W5B may request triage-report tweaks
via the coordinator — take them only with an explicit buz from CL.7c63.

## Definition of done

- Both POI 46368 files export successfully with loud drops; harness
  classifies honestly; before/after evidence in the seal.
- Contract amendment written; tests green.
- `scripts/check.sh` fully green in your worktree.
- Commit messages end with your bee name.

## Sealing

`hive seal <your-bee-name>` (implementation seal) with status, summary,
deliverables, exact test counts, harness before/after numbers, and
deviations. Then
`hive buz send CL.7c63 --sender <your-bee-name> --tier queue -p "sealed"`.
Blockers → buz immediately; never a silent stall.
