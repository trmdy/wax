# W5B review — corpus triage to green

- **Shard:** w5b (`agent/wax-w5b-triage`, bee CO.9f42 / wax-w5b)
- **Commits reviewed:** `b95d78f` (shard) + `1e893bd` (coordinator post-review fixes)
- **Reviewer:** coordinator CL.7c63 with an adversarial review agent, 2026-07-28
- **Verdict:** merge after fix-cycle (bee had exited post-seal; coordinator
  applied the fixes directly, W3C precedent).

## Scope reviewed

Full diff (+824/−70): `calamine_reader.rs`, `xls_styles.rs`,
`xlsb_styles.rs`, corpus-backed tests, `docs/w5-triage.md` ledger,
`harness/adjudications.md` (16 new evidence-grade entries). Shard results:
opens +0.35pp (zero regressions, 7 new opens), values +0.81pp, display
+0.41pp, cached-result fidelity +5.23pp; all 3 `internal` and all 3
`unsupported` open failures fixed; `###0.00;-###0.00` 14,592 → 0;
22,073 legacy missing strings restored; every triage bucket terminal
(fixed / adjudicated / signed-off limitation with pinned root cause).
Limitation sign-off was granted pre-seal with conditions (counts, upstream
pointers, no hidden fixable bugs) — all conditions met in the ledger.

## Review-agent findings and outcomes

1. **required-fix (fixed in `1e893bd`)** — the `Data::DateTime` arm routed
   through `normalize_number_value`, coupling cell *type* to format-
   supplement health: a degraded styles stream would flip every date cell
   in a file from `t:"d"` to `t:"n"`. Fixed: calamine's datetime typing is
   trusted directly; only negative serials (which Excel cannot render as
   dates) downgrade to numeric. Pinned by a unit test covering both sides.
2. **required-fix (fixed in `1e893bd`)** — `pending_string_formula` was
   cleared by any record other than STRING/Continue, but MS-XLS allows
   SHRFMLA/ARRAY/TABLE between a string FORMULA and its STRING record —
   such cells lost their empty-string caches. Fixed: 0x04BC/0x0221/0x0236
   exempted from the reset, with a synthetic-BIFF regression test.
3. **minor (fixed in `1e893bd`)** — the empty-string skip in
   `range_records` also applied to ODS, which has no supplement to restore
   the cells. Fixed: skip parameterized, ODS keeps empty cached strings.
4. **minor (accepted)** — package normalization (backslash zip names,
   Beta bundle records) materializes entries in memory, bounded only by
   the preflight rails (100 MB file / 2 GiB uncompressed) and only via
   `read_with_deadline`; direct `CalamineReader.read` callers are
   uncapped. Accepted for v1: all production entry points go through the
   deadline path; noted for any future embedding work.
5. **minor (accepted)** — `parse_bundle_sheet` now requires exact record
   consumption; spec-marginal padded BrtBundleSh records would degrade the
   whole style supplement to a warning. Accepted: degradation is loud, no
   corpus file exhibits padding.
6. **minor (accepted, documented)** — the four headline corpus regression
   tests are `#[ignore]`d (need `WAX_CORPUS_ROOT`), so CI cannot re-verify
   them, and `is_structural_xls_error` matches calamine error substrings
   that could drift on upgrade. Accepted: corpus is not available in CI by
   design; the risk is bounded to a calamine version bump, which already
   requires a full harness re-run per the mission rules.
7. **nit (fixed in `1e893bd`)** — `container_error`'s mixed `||`/`&&`
   expression parenthesized.
8. **nit (accepted)** — duplicate-position records in a hostile xlsb can
   let a later empty `BrtFmlaString` overwrite earlier cell metadata;
   malformed-input value substitution, same pattern as the pre-existing
   `cached_error` path. No crash surface.

**Checked clean by the agent:** all new BIFF/XLSB parsing is bounds-safe;
allocations proportional to actual bytes, not declared counts; varint
decoder capped; error paths degrade to the ordinary open; `safety.rs` /
`lib.rs` untouched (W5A boundary held).

## Verification after fixes

`cargo test -p wax-read` 44+11+5 green including the corpus-backed ignored
tests (4/4 with `WAX_CORPUS_ROOT`); clippy clean; `scripts/check.sh --fast`
fully green in the worktree. Corpus-wide numbers are re-validated by the
coordinator's final full scoreboard run before the release tag.
