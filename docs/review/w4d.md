# Review — W4D: reader col widths + basic styles (`agent/wax-w4d-styles`, d0e2d57)

Reviewer: CL.6cbf (coordinator, agent-assisted deep review with a probe
fixture). Verdict: **merge after fixes** (findings 1–2, sent to the shard
2026-07-28; see the fix log).

## Scope check

Diff touches `crates/wax-read/{Cargo.toml,src/calamine_reader.rs,tests/**}`
(+890/−66) plus `Cargo.lock` (serde_json as a wax-read dev-dependency).
In-lane; zero changes to wax-core/wax-store — the shard populated the W4
scaffold fields without needing sign-off. `cargo test -p wax-read` green
in a detached review worktree (35 lib + 9 integration + 5 stub tests, 1
ignored corpus spot-check), clippy `--all-targets` and fmt clean.
`git merge-tree` against current main (post-W4A/B/C): clean, and the
cross-shard width contract holds — W4D emits 0-based `ColInfo.c` with
finite, non-negative widths (NaN/∞ rejected at parse, negatives
rejected); widths over 255 flow through by design and the W4A writer
clamps them loudly.

## Design

`OoxmlSupplement` grows from a per-cell numFmt map into a full XF table:
`parse_styles` now resolves fonts (b/i/u/strike/sz/name/color) and solid
patternFill fgColor into `CellStyle`, colors as `#RRGGBB` from `rgb`
(alpha stripped, case-normalized) or the ECMA-376 legacy indexed palette
(64 entries; 64/65 system colors unresolvable → dropped); `theme`/`tint`
are dropped without guessing, per contract. Styles are interned during
the sheet walk and re-compacted after the max_cells cap so truncated
cells can't pin unreferenced table entries — table order is deterministic
(first-use). Column widths: `<col>` declarations with `customWidth`
truthy expand min..=max into per-column `ColInfo`s,
last-declaration-wins via BTreeMap overwrite, capped at used-extent +
256 (≤16384) so a whole-sheet declaration stays bounded (unit-tested).
Degradation is properly scoped: a malformed styles part → warning + no
styles/fmt, open survives; a malformed cols run → warning + no widths
for that sheet, styles retained; both integration-tested through
`read_with_deadline`. The per-cell format lookup moved from cloned
Strings to references, which plausibly explains the measured −2.2% parse
time.

One deliberate semantic change, correctly commented at the source: a
cell with no `s` attribute now resolves cell XF 0 (per spec) instead of
nothing. Because real Excel base fonts always carry explicit `sz`/`name`,
essentially every cell of every real xlsx gets `s: Some(_)` — the
"fully-default ⇒ None" clause only fires for packages with a bare
font 0. The deduped style table stays tiny; the cost is ~8 bytes/cell of
dump JSON (the seal's +2.526%). **Coordinator sign-off: approved** — the
base font is real export-a-copy fidelity data; the additive-invisibility
contract holds in the letter (pre-W4 dumps of packages without a styles
part stay byte-identical, pinned by test).

## Findings

1. **major (required fix) — hostile `<cols>` part burns unbounded CPU
   inside the rails** (`calamine_reader.rs`, `col_infos`). The expansion
   is O(declarations × cap): probed with 20,000
   `<col min="1" max="16384" customWidth="1"/>` + `dimension A1:XFD1`
   (~1MB part, 20k tokens vs the 5M token guard) → **104.9s** of CPU in
   a debug build; the guard budget admits 250× more. Effect: dump/serve
   return Timeout at 30s (an openable file becomes a failure) and the
   abandoned worker thread burns a core for the duration — the W3
   outlier class. Fix: last-wins ⇒ iterate declarations in reverse with
   a covered-columns set, or merge intervals; O(N + cap).
2. **medium (required fix) — styles parsing is stricter than the
   pre-W4D path it extends**: one malformed attribute anywhere in
   styles.xml (`rgb`, `sz`, `numFmtId`, `fontId`…) hard-fails the whole
   part, nulling every `fmt` in the workbook — where the old code
   skipped the bad entry. Contract-compliant (warning, open survives)
   and corpus-clean (624/666 unchanged, zero mismatches), but a
   display-match risk on off-corpus sloppy generators. Fix: per-entry
   lenience for attribute junk; hard-fail only on XML-level
   malformation.
3. low (deferred to W5) — `intern_style` is a linear scan per styled
   cell (run twice); hostile many-distinct-XF files reach the same
   watchdog-burn family as 1. Real-world style counts make it moot;
   hash-keyed interning fixes it.
4. nits — wholesale sheet-metadata failure emits two warnings per
   sheet; the byte-identical dump test pins `toolVersion` (breaks on
   version bump); no explicit tests for gray125 fills, indexed 64/65,
   or `<u val="none"/>` (all handled correctly by inspection).

## Seal cross-check

624/666 is the xlsx/xlsm subset of the 2044-file corpus (scoreboard
1956/2044 total) — denominator coherent. Opens-unchanged matches the
degrade-to-warning design; +2.526% dump is consistent with ubiquitous
`"s":0` on xlsx diluted by non-xlsx files; −2.220% parse and +1.297%
styled-heavy store are plausible from the code. Corpus not rerun for
this review.

## Fix log

- 2026-07-28: findings 1–2 sent to the shard as required pre-merge
  fixes; XF-0 design approved; finding 3 deferred to W5.
- 2026-07-28 (fixes sealed, commit f90bd0b): width expansion rewritten
  to reverse last-declaration-wins traversal with a disjoint-successor
  set — O(declarations + cap); the 20,000-declaration whole-sheet probe
  now completes in 0.01s (was 104.9s) with exactly 16,384 entries
  pinned by regression test. Style-attribute junk is now local to its
  entry (index-preserving placeholders for fonts/fills/XFs, skipped bad
  numFmts); XML-level malformation alone still fails the part. Unit +
  real-OOXML integration tests prove one bad font/color no longer
  discards valid formats. Full check green; merged to main.
