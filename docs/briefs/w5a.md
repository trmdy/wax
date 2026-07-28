# Shard W5A — fuzz burn-in + calamine FAT containment

You are shard **W5A** of the wax v1 mission, wave 5 (hardening + release).
Coordinator: bee **CL.7c63**.

**Required reading before any code:** `MISSION.md` (§Safety rails),
`fuzz/known-findings/README.md` (your primary target), `scripts/check.sh`
(the fuzz gate + burn plumbing), `crates/wax-read/src/safety.rs`,
`docs/briefs/w3c.md` (how the rails were corpus-fitted). You work in your
git worktree only (`.worktrees/w5a`), on branch `agent/wax-w5a-fuzz`.
Never touch `main`; never `git push`. Commit locally, the coordinator
merges.

## The job

### 1. Resolve the quarantined calamine unbounded-FAT finding

`fuzz/known-findings/legacy_xls_reader/calamine-unbounded-fat-growth.xls`
drives calamine's own CFB reader into unbounded Vec growth (measured
87 GiB peak RSS before the wall-clock timeout fires). The README lists
three fix options. Constraints on your choice:

- **macOS caveat:** `RLIMIT_AS` is effectively not enforced on macOS, and
  Apiary's primary platform is macOS. If you pick option (c)
  (child-process address-space limit) you must *prove* containment on this
  Mac with a measured peak-RSS number, not assume it. If you cannot prove
  it, option (a) — mirror the FAT/DIFAT chain walk in preflight with a
  cycle + total-length bound — is the deterministic fix and is expected to
  be the winner. (b) upstreaming is welcome as a bonus, never a blocker.
- Definition of resolved: `wax dump` (and serve `open`) on the quarantined
  input returns a structured error (`bomb` or `bad_zip`, whichever the
  evidence supports) **fast** (no 30 s timeout ride) with peak RSS in the
  tens of MiB; the input moves into `fuzz/artifacts/legacy_xls_reader/` so
  the deterministic replay in `check.sh` becomes its regression gate; the
  `fuzz/known-findings/` directory is emptied and its README updated to
  say so (keep the file as the ledger format for future findings).
- **Corpus-fit is mandatory** (W3C lesson: an early rail draft falsely
  rejected 126 real files). Any new preflight bound must be validated
  against the full corpus: zero newly-failing opens vs current `main`.
  `harness/run.sh` (no `--soffice`) before/after, compare opened counts,
  put the numbers in your seal.

### 2. Extended burn-in on all three targets

`container_preflight`, `xlsx_reader`, `legacy_xls_reader`:

- `WAX_FUZZ_BURN_SECONDS=1800 scripts/check.sh --fuzz-burn` (30 min per
  target) as the baseline pass. Stagger with your other work — it is
  CPU-bound but unattended.
- Every finding: minimize (`cargo fuzz tmin`), fix or guard in wax (or
  containment-bound if it is upstream), add the minimized input to
  `fuzz/artifacts/<target>/`, re-burn. A finding you cannot fully contain
  goes into `fuzz/known-findings/` with a complete ledger entry — but the
  bar for leaving one there at wave end is high; expect the coordinator to
  push back.
- Done when a full 3×1800 s pass completes with zero new findings.
  Report per-target burn stats (total execs, corpus growth, findings
  found/fixed) in the seal.

## Tests

Unit tests for every new preflight bound (hit the bound → structured
error; legitimate chain just under the bound → passes). The quarantined
input's containment gets an explicit integration test if runtime allows
(fast structured error), otherwise the artifact replay covers it.

## Boundaries (do not touch)

`fuzz/**` and `crates/wax-read/**` are yours, but inside wax-read your
lane is `safety.rs` + its hooks in `lib.rs`. Value/format semantics in
`calamine_reader.rs` belong to W5B (running in parallel) — if your fix
needs to reach in there, buz the coordinator first so we sequence it.
Nothing in `harness/**` (W5D owns harness code), `.github/**`/`scripts/**`
(W5C) beyond what `check.sh` already gives you, `crates/wax-write/**`
(W5D).

## Definition of done

- Known-findings directory empty; quarantined input contained + gated.
- Clean 3×1800 s burn pass; all findings fixed/guarded with artifacts.
- Zero corpus open regressions, numbers in the seal.
- `scripts/check.sh` fully green in your worktree.
- Commit messages end with your bee name.

## Sealing

`hive seal <your-bee-name>` (implementation seal) with status, summary,
deliverables, exact test counts, burn stats, the containment fix chosen +
measured peak RSS on the quarantined input, corpus before/after open
counts, and deviations. Then
`hive buz send CL.7c63 --sender <your-bee-name> --tier queue -p "sealed"`.
Blockers → buz immediately; never a silent stall.
