# Shard W4D — reader extraction: xlsx column widths + basic styles

You are shard **W4D** of the wax v1 mission, wave 4. Coordinator: bee
**CL.6cbf**.

**Required reading before any code:** `MISSION.md`, `docs/w4-contracts.md`
(§5 is your spec; §1 describes the model/store scaffold you populate),
the existing xlsx styles path in `crates/wax-read/` (the `numFmtId`-per-
cell XF mapping W2A/W3E built — you are extending it, not replacing it).
You work in your git worktree only (`.worktrees/w4d`), on branch
`agent/wax-w4d-styles`. Never touch `main`; never `git push`. Commit
locally, the coordinator merges.

## The job

For xlsx/xlsm only (contract §5 is normative):

1. **Column widths**: worksheet `<cols>` → `Sheet.col_infos`
   (min..=max expansion honoring `customWidth`, capped at used extent + a
   sane bound so whole-sheet default declarations don't balloon the
   model).
2. **Basic styles**: extend the `xl/styles.xml` XF resolution to fonts
   (bold/italic/underline/strike/size/name/color) and solid-fill
   colors; `rgb` + `indexed` (legacy palette) only, `theme`/`tint`
   dropped without guessing. Dedup into `Document.styles`; per-cell via
   `Cell.s`; fully-default style ⇒ `s: None` (keep the table small).
3. Everything flows through the existing safety rails; a malformed styles
   or cols part degrades to no-styles/no-widths + a warning — it must
   never turn an openable file into a failure. Open rates on the corpus
   must not regress.

xls/xlsb/ods extraction is explicitly out of scope (W5+ candidate) — do
not fake it.

## Tests

Unit fixtures: cols ranges (single, min..max span, customWidth false,
whole-sheet declaration capped), fonts (each flag, size, name, rgb color,
indexed color), solid fills, theme-color dropped, style dedup + `s: None`
for default, malformed styles part degrades with warning. The
additive-invisibility contract: a fixture without explicit widths/styles
dumps byte-identically to pre-W4 (there is a wax-core test pinning the
serialization; add a reader-level one). A corpus spot-check test on 2–3
known styled files.

## Measure (for the seal)

Corpus impact numbers: open rate unchanged, dump-size delta, parse-time
delta, store `approx_bytes` delta on a styled-heavy file. Real numbers,
not estimates.

## Boundaries (do not touch)

`crates/wax-write/**` (W4A), `crates/wax-cli/**` (W4C), `harness/**`
(W4B), `corpus/**`, `scripts/**`, `.github/**`, `docs/**`,
`ASSIGNMENTS.json`. You own `crates/wax-read/**`; `crates/wax-core/**`
and `crates/wax-store/**` are **additive only and with coordinator
sign-off** — the W4 fields already exist, so you should rarely need
either; buz first if you do.

## Definition of done

- `scripts/check.sh` fully green in your worktree.
- Widths + basic styles populated on xlsx corpus files; measured impact
  numbers in the seal; open rates not regressed.
- Commit messages end with your bee name.

## Sealing

`hive seal <your-bee-name>` with status, summary, deliverables, exact test
counts, the measured corpus numbers, and deviations. Then
`hive buz send CL.6cbf --sender <your-bee-name> --tier queue -p "sealed"`.
Blockers → buz immediately; never a silent stall.
