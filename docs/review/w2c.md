# Review — W2C harness/scoring upgrades (agent/wax-w2c-harness, 0eca314)

Reviewer: CL.988e (coordinator). Verdict: **merge**.

## Scope check

Single commit, 15 files, all inside `harness/wax-harness/**` — exactly the
assigned lane (`harness/run.sh` untouched; it needed no change since the
binary emits the new reports itself).

## What it adds

- `display_string_match` (both-non-null, exact equality) at file and
  aggregate level; new scoreboard row. Distinct from coverage, as specced.
- `per_extension` breakdown (attempted / wax opened / SheetJS opened /
  value match) keyed by manifest `ext` (lowercased; empty → `unknown`);
  the xlsx row is labelled as the binding W2 gate in SCOREBOARD.md.
- `formats.rs`: per-format-code display coverage + match, joined against
  `harness/formats/corpus-formats.json` when present (ranking switches to
  corpus cell count and the report says which ranking it used —
  `joined_corpus_formats` flag). Missing file degrades to no-join;
  malformed file errors loudly. Duplicate codes rejected.
- `triage.rs`: `harness/triage.md` with three sections (open failures by
  error code, value mismatches by type-pair, display mismatches by format
  code), top-20 categories, ≤5 example files each.
- Reports: `harness/format-coverage.json` + `harness/triage.md`, written
  atomically like the existing outputs.

## Antagonist findings

1. **Privacy** — checked hard, holds: `private` comes from the manifest
   entry (not path heuristics), example paths are excluded for private
   files while counts still aggregate, and there's a regression test
   asserting a private path does not leak into triage.md. Markdown
   injection via hostile format codes is neutralized (`inline_code`
   escapes `&<>|` and strips newlines).
2. **Additivity** — all new serde fields are `#[serde(default)]` and W1
   fields are untouched; old scoreboard.json consumers keep working. The
   shard's full-corpus validation run reproduced every W1 count exactly.
3. **Stub-safety** — display match honestly `n/a` when wax emits no
   display strings; format coverage counts oracle cells with wax coverage
   0. Both tested with stub-shaped fixtures per the brief.
4. Nits (not blocking): `compare()` derives `ext` from the path only for
   the runner to overwrite it from the manifest — harmless duplication.
   `format_display` per-file lists could bloat results.jsonl on
   format-heavy files; acceptable at current corpus scale.

## Verification

- Shard ran `scripts/check.sh` (green: fmt, clippy -D warnings, 46
  workspace + 30 standalone harness + 9 oracle tests) and full 2,048-file
  `harness/run.sh` twice (200-file smoke + full).
- Coordinator re-ran `scripts/check.sh --fast` and full `harness/run.sh`
  on merged main; scoreboard delta committed per operator amendment 2
  (see the merge commits).
- Useful data point surfaced by the per-extension work: the corpus holds
  638 xlsx files; the W1 stub already opens 597 of them (93.57%) — the
  binding W2 xlsx-opens gate (≥90%) is therefore about *not regressing*
  while W2A swaps in calamine and lifts value fidelity to ≥95%.
