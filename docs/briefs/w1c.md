# Shard W1C — SheetJS oracle runner

You are shard **W1C** of the wax v1 mission. Coordinator: bee **CL.661**.

**Required reading:** `MISSION.md` (§The corpus and the oracle),
`docs/w1-contracts.md` §1 (normalized dump — binding, byte-for-byte field
discipline) and §5 (runner contract). Branch `agent/wax-w1c-oracle`, this
worktree only. No remote; commit locally.

## Deliverables

1. **`harness/oracle/package.json` + lockfile**: SheetJS pinned as the
   CDN tarball `https://cdn.sheetjs.com/xlsx-0.20.3/xlsx-0.20.3.tgz`
   with its integrity hash recorded in the lockfile. Never the npm
   `xlsx` name. (Same pattern as `~/Projects/_tmp/sheets-spike/` —
   consult it.) Node ≥20, plain CommonJS or ESM, no build step, minimal
   deps (ideally SheetJS only + a test runner).
2. **`harness/oracle/run.js`**:
   `node harness/oracle/run.js <file> [--max-cells N] [--timeout-ms N]`
   → exactly one normalized-dump document (contracts §1) on stdout with
   `tool:"sheetjs"`, `toolVersion` = the real SheetJS version. Designed
   for **one fresh node process per file** (no server mode). Exit-code
   discipline per contracts §2/§5: unreadable file = `ok:false` document
   with a sensible `code`, exit 0; usage error = exit 2; genuine abort =
   exit 1, nothing on stdout.
3. **Mapping fidelity** (the hard part — this defines ground truth):
   - `v`/`t` from SheetJS cell `t`/`v`; `d` from SheetJS `w` (null when
     absent); `f` from cell `.f` (no leading `=`); `fmt` from the cell's
     number format (`.z`), null for General/unknown.
   - Dates: emit `t:"d"` with ISO-8601 `v` (resolve 1900/1904 epoch via
     workbook props; use `cellDates` deliberately and document the
     choice). Serials you can't confidently resolve stay `t:"n"`.
   - Merges → A1-style ascending; rows/cols extents from `!ref`
     (1-based counts, 0 for empty); sparse cells ascending row-major,
     0-based `r`/`c`; `--max-cells` (default 200000) with symmetric
     `truncated` flags per contracts §1.
   - `wallMs` = parse wall time; `peakRssBytes` from
     `process.resourceUsage().maxRSS` normalized to bytes; `sha256` of
     the input file.
   - Absent info is `null`, never omitted, never `""`.
4. **Tests** (`harness/oracle/test/`): runnable offline against small
   committed fixture files (generate xlsx fixtures with SheetJS itself
   in a setup script or commit tiny ones): schema shape (every field
   present, nulls not omitted), a formula cell carrying both `f` and
   cached `v`, a merge, a date cell, truncation behaviour, ok:false on
   garbage input. Plus a schema validator module the tests share —
   export it; the harness shard may reuse it.
5. **`harness/oracle/README.md`**: install (`npm ci`), usage, the
   pinning rationale, mapping decisions (esp. dates).

## Boundaries (do not touch)

Everything outside `harness/oracle/`. Coordinator contact:
`hive buz send CL.661 --sender <your-bee-name> --tier queue -p "<msg>"`.

## Definition of done

- `npm ci && npm test` green inside `harness/oracle/`.
- Running it on a real xlsx (there are some in
  `~/Projects/_tmp/sheets-spike/corpus-syn/`) produces valid schema-1
  JSON; verify with your validator.
- Lint-clean (`node --check` at minimum; eslint optional), commits end
  with your bee name.

## Sealing

Seal (`hive seal <your-bee-name> --from seal.json`) with deliverables,
test counts, SheetJS version + integrity hash, mapping decisions/
deviations. Then buz CL.661.
