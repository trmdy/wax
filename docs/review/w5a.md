# W5A review — fuzz burn-in + hostile-input containment

- **Shard:** w5a (`agent/wax-w5a-fuzz-coord`)
- **Executed by:** coordinator CL.7c63 after the assigned codex bee
  (CO.eda, gpt-5.6-sol) was retired — see Deviations.
- **Reviewer:** coordinator (self-review; the fixes are pinned by tests,
  measured before/after numbers, and two full corpus-fit runs).

## Deviation: coordinator takeover

The assigned shard bee acknowledged a sound plan and ran the corpus
baseline, then its model's cybersecurity filter refused the task twice —
analyzing a deliberately hostile spreadsheet reads as a security request.
The second refusal ended a turn mid-analysis; after ~2 h it had produced
no commits. Retired it and took the shard over directly (W3C precedent,
where two bees wedged on the same fuzz step). Recorded for successors:
**hostile-input hardening is a poor fit for codex shards** — the work
looks like offensive security to the model's filter even when it is
plainly defensive QA on our own parser. Either brief it as ordinary
robustness engineering from the first message, or keep it with the
coordinator.

## 1. The quarantined finding — diagnosis was wrong, now closed

`fuzz/known-findings` carried one open finding since W3C, diagnosed as
"calamine unbounded FAT-driven growth" in the CFB reader, with three
proposed fixes (mirror the FAT walk, upstream a patch, or an
address-space rlimit).

I first implemented the FAT/DIFAT-chain mirror per that diagnosis. It did
not contain the artifact — the file passed every chain check legitimately.
Instrumenting with a global allocator that aborts and backtraces on any
single allocation over 1 GiB located the real site:

```
137438953472 bytes
  calamine::Range::from_sparse
  calamine::xls::Xls<RS>::new_with_options
```

`Range::from_sparse` densifies the span **observed** in the collected cell
records — min/max row and column over the cells — and allocates
`rows * cols` defaults. No DIMENSIONS record participates, which is why
wax's existing declared-extent rail never saw it. The artifact holds cell
records at opposite corners of the BIFF grid: 65,536 × 65,536 = 2³² cells.

**Fix:** the BIFF preflight now accumulates the row/column span of every
cell-bearing record it already walks (Blank, Number, Label, BoolErr,
RString, RK, LabelSst, Formula, MulRk, MulBlank) and applies the same
`max_declared_cells` cap the DIMENSIONS rail uses — `ObservedExtent` in
`crates/wax-read/src/safety.rs`.

**Measured:** 24.4 GiB peak RSS / 33 s (timeout ride) → **1.2 MB / 0.26 s**
with a structured `bomb` naming the exact span. The input is now a
committed corpus seed
(`fuzz/corpus/legacy_xls_reader/calamine-observed-extent-bomb.xls`), so
`scripts/check.sh`'s deterministic replay gates it, plus a unit test
pinning the message. A byte-identical duplicate
(`oom-bc197d861c-original-artifact.xls`) was the same finding and is gone
with it. **`fuzz/known-findings/` is now empty.**

The CFB directory/mini-FAT chain-cycle guards written for the wrong
diagnosis were kept deliberately: calamine reads the directory chain with
`usize::MAX` as its length bound, so a cyclic FAT there is a real
(if unexercised) growth path; the guards are cheap and cost no opens.

## 2. Burn-in finding — calamine's column accumulator

Round 1: `container_preflight` clean over 1800 s (35,481,433 executions,
19,700 exec/s, 16,404 new units, 1,093 MB peak). `xlsx_reader` then
crashed:

```
calamine-0.36.1/src/xlsx/mod.rs:2838 — attempt to multiply with overflow
  get_row_and_optional_column
```

`col = col * 26 + (c - b'A') + 1` has no overflow guard. Seven or more
letters overflow `u32`: a panic under the overflow checks the fuzz targets
build with, and a **silent wrap** in release, yielding a column index
unrelated to the stored reference. Release-mode impact was checked
directly and is benign (the crash input is rejected `bad_zip` in 2 ms at
3 MB; a crafted overflowing dimension opens harmlessly at 1.8 MB because
calamine reads xlsx cells rather than trusting the dimension), so this is
not a shipped-binary memory hazard — but the fuzz gate must be clean and
a wrapped index must never reach the reader.

**Fix:** `check_cell_reference_attributes` in the XML preflight rejects
references with more than three column letters, scoped to the attributes
calamine feeds that parser — `@r` on `<c>`/`<row>`, `@ref` on
`<dimension>`/`<mergeCell>`/`<autoFilter>`/`<hyperlink>` — matched by
**local** name. Excel's last column is `XFD`, so >3 letters cannot name a
real column. Both scoping decisions were forced by evidence:

- Matching *qualified* names (the first attempt) let a namespace-prefixed
  `<x:dimension>` walk straight past the rail while calamine, which
  matches local names, still parsed it. Round 2 of the burn found exactly
  that.
- Matching *any* `r`/`ref` attribute regardless of element (the second
  attempt) cost **14 corpus opens**: it also matches XSD
  `<xs:element ref="EG_ExtensionList">` and `dc:creator` in the
  custom-XML and schema parts real workbooks carry. Reverted.
- `xmlns:r="…"` has local name `r` and had to be excluded explicitly.

Verified: a crafted overflow file is rejected structurally and a
legitimate `A1:XFD1048576` dimension still opens.

### Residual — declared open, not silently passed

Round 3's artifact is **not** closed by this rail, and the reason is worth
more than the crash: its sheet part is invisible to preflight. The `zip`
crate cannot enumerate that member (Python's `zipfile` also refuses it:
`Bad magic number for file header`) while calamine's own zip reader parses
it happily — so preflight validated a different set of parts than the
reader consumed. **This applies to every XML rail wax has**, not just this
one. No XML-layer fix can close it; it needs either an upstream
`checked_mul` guard in calamine or a container-level check that two zip
readers agree on the entry set.

Release-mode impact was measured, not assumed: the artifact returns a
structured `bad_zip` in ~2 ms at ~3 MB, and a wrapped column index that
does reach the reader is inert because calamine reads xlsx cells rather
than trusting the dimension (opens at 1.8 MB, no growth). The finding is
quarantined in `fuzz/known-findings/xlsx_reader/` with full evidence and
ranked fix options, and it is declared in the W5 seal as a gate deviation.

## 3. Corpus fit (mandatory, twice)

Both rails were validated against the full 2,044-file corpus, since
either could over-reject:

| Run | Files opened | Regressions |
| --- | ---: | ---: |
| Baseline (post-W5B merge) | 96.04% (1963/2044) | — |
| After observed-extent rail | 96.04% (1963/2044) | 0 |
| After cell-reference rail (element-scoped) | 96.04% (1963/2044) | 0 |
| After broadened any-`ref` rail — **rejected** | 95.35% (1949/2044) | **14** |
| After re-scoped rail (shipped) | 96.04% (1963/2044) | 0 |
| After `Lbl` minimum-length rail | 96.04% (1963/2044) | 0 |

Five full 2,044-file runs. The rejected row is why the broad rail is not
in the tree: corpus fit caught it, not review.

## 4. Burn-in: five rounds, four distinct outcomes

Extended burns were run repeatedly because each round changed the tree.
What they produced, in order:

1. **`container_preflight` — clean, four times over** (1800 s each;
   representative round: 35,481,433 executions, 19,700 exec/s, 16,404 new
   units, 1,093 MB peak). No findings at any point.
2. **`xlsx_reader` — calamine reference-arithmetic family (open).** Two
   distinct sites in the same upstream code: the column accumulator
   overflow (§2) and `get_dimension`'s unordered
   `parts[1].0 - parts[0].0`, which underflows on a reversed range
   (`B9:A1`). Both panic only under overflow-checked builds and are inert
   in release — measured across two artifacts and three crafted
   workbooks. Consolidated into one declared finding in
   `fuzz/known-findings/`; chasing further individual operations at the
   XML layer was rejected as whack-a-mole with real corpus cost.
3. **A false OOM that would have become a fabricated finding.** Round 3
   reported `libFuzzer: out-of-memory (used: 2051Mb; limit: 2048Mb)` on
   `xlsx_reader` — but the artifact was **zero bytes** (`da39a3ee…` is the
   SHA-1 of the empty input) and the stack held only `fuzzer::Fuzzer::Loop`
   and libFuzzer's own `SizedFile` vector, with no wax or calamine frames.
   `-rss_limit_mb` caps *total process* RSS, which after ~10k discovered
   units includes the fuzzer's in-memory corpus. Fixed in `check.sh`:
   raise the process ceiling and set `-malloc_limit_mb` explicitly so
   single hostile allocations — the class that caught the 137 GB extent
   bomb — still report. **This was invisible at the 300 s CI burn length
   and only appeared at 1800 s.**
4. **`legacy_xls_reader` — calamine `parse_lbl` out-of-bounds (fixed).**
   `index out of bounds: the len is 0 but the index is 3` at
   `xls.rs:791`: `parse_lbl` reads `data[3]` and `read_u16(&data[4..])`
   with no length check. **This class panics in release too**, unlike the
   arithmetic family — wax contained it as a structured `internal` error
   in 1.5 ms, but `internal` means "wax bug" by our own contract and W5B
   had just driven that category to zero. Fixed properly: `Lbl` (0x0018)
   joins the preflight's minimum-record-length table (6 bytes), which
   exists for exactly this class. The input now returns a deterministic
   `bad_zip` naming the record, and is a committed regression seed.

Final per-target statistics are in the seal.

## Findings against my own work

1. The `ObservedExtent` cap reuses `max_declared_cells`, so a file whose
   *real* sparse span exceeds 8M cells is now rejected as `bomb` even
   though calamine would have allocated it successfully (8M cells ≈ 256 MB).
   Accepted: it is the same bound the declared-extent rail has enforced
   since W3C, well-formed files declare a matching DIMENSIONS extent
   anyway, and the corpus shows zero such files. A consumer needing more
   raises `max_declared_cells`.
2. The three-letter reference cap is format-true for xlsx/xlsm but is
   applied to every XML part in the package, including ones calamine may
   not parse. Accepted: rejecting a structurally impossible reference
   anywhere in the package is the conservative direction, and corpus fit
   is clean.
3. Self-review is weaker than the agent-assisted reviews the other three
   shards received. Mitigated by measured before/after containment
   numbers, unit tests pinning both errors, deterministic artifact replay,
   and two full corpus runs — but stated plainly rather than papered over.
