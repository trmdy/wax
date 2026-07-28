# Shard W4B — round-trip validation + scoreboard

You are shard **W4B** of the wax v1 mission, wave 4. Coordinator: bee
**CL.6cbf**.

**Required reading before any code:** `MISSION.md`, `docs/w4-contracts.md`
(§4 is your spec; §2–3 are what you measure), the existing harness code
(`harness/wax-harness/`), `harness/run.sh`. You work in your git worktree
only (`.worktrees/w4b`), on branch `agent/wax-w4b-roundtrip`. Never touch
`main`; never `git push`. Commit locally, the coordinator merges.

## The job

Extend the harness with the W4 writer validation per contract §4:

1. **Round-trip differential** over every corpus file wax opens: export
   xlsx via `wax export --json` (fresh process per file, mirroring the
   dump path), re-dump the exported file, compare model→model:
   value fidelity (t, v), display fidelity (d), merges set-wise,
   round-trip files clean %, loud truncated-skip count.
2. **Oracle read-back**: SheetJS oracle reads each exported file →
   `oracle read-back open %`.
3. **soffice does-it-open** on a deterministic ~200-file validation subset
   (seeded, spread across source extensions, size-capped): headless
   convert with a fresh `-env:UserInstallation` profile, per-file timeout,
   clean = exit 0 + non-empty output. soffice is installed on this machine
   (PATH + `/Applications/LibreOffice.app/Contents/MacOS/soffice`);
   absent ⇒ loud `n/a`, never a fake rate.
4. **Scoreboard**: "Writer round-trip" section rows + additive camelCase
   `metrics.roundTrip {filesClean, valueMatch, displayMatch,
   oracleOpenRate, sofficeOpenRate, skippedTruncated, status}` with
   numerators/denominators. Keep every existing metric and JSON field
   working — additive only.

**Degrade gracefully**: until W4A/W4C merge, `wax export` may not exist or
xlsx may return a structured `internal` error. Detect that and print
`n/a (xlsx export unavailable)` for the whole section; the run must still
complete and every pre-W4 number must be unchanged. Build against the
contract, test against fixtures/mocks now; real corpus numbers land at
integration (the coordinator will tell you when to produce them).

Runtime budget: the full round-trip pass adds an export + a dump + an
oracle read per file — keep it parallel like the existing runner, and make
the soffice subset a flag so the quick loop stays quick.

Where wax's round-trip is right and the oracle's read-back disagrees,
adjudicate with evidence in `harness/adjudications.md` (append-only),
never auto-lose.

## Tests

Harness unit tests for: comparator correctness (value/display/merge
rules, numeric epsilon reuse), round-trip bookkeeping (failed export
counted as unclean, truncated counted as skipped), scoreboard JSON shape
(additive; old fields byte-stable), soffice detection + `n/a` path,
deterministic subset selection (same seed → same files).

## Boundaries (do not touch)

`crates/**`, `corpus/**`, `scripts/**`, `.github/**`, `docs/**` (except
nothing), `ASSIGNMENTS.json`. You own `harness/wax-harness/**`,
`harness/run.sh`, and append-only `harness/adjudications.md`.

## Definition of done

- `scripts/check.sh` fully green in your worktree.
- Full harness run works both with and without a real xlsx export
  available (degradation path exercised by test).
- Scoreboard renders the new section with honest numbers or honest `n/a`.
- Commit messages end with your bee name.

## Sealing

`hive seal <your-bee-name>` with status, summary, deliverables, exact test
counts, and deviations. Then
`hive buz send CL.6cbf --sender <your-bee-name> --tier queue -p "sealed"`.
Blockers → buz immediately; never a silent stall.
