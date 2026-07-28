# W5 corpus triage ledger

Final W5B harness snapshot: `2026-07-28T20:53:49Z`, 2,044 attempted and
zero skipped. The run used `harness/run.sh` without `--soffice` and wrote
isolated artifacts under `/private/tmp/wax-w5b-harness.hhd2dq`.

## Outcome summary

| Metric | Before W5B | After W5B | Delta |
| --- | ---: | ---: | ---: |
| Files opened | 95.69% (1,956/2,044) | 96.04% (1,963/2,044) | +7 files / +0.35 pp |
| Cell-value match | 92.31% (2,679,994/2,903,176) | 93.12% (2,713,261/2,913,841) | +0.81 pp |
| Display-string match | 97.09% (2,789,517/2,873,176) | 97.50% (2,833,244/2,905,914) | +0.41 pp |
| Display coverage | 99.97% | 99.97% (2,906,772/2,907,549) | unchanged |
| Cached-result fidelity | 53.53% | 58.76% (267,324/454,931) | +5.23 pp |

There are no open regressions: all 1,956 files that opened before W5B still
open. Seven files newly open: `nonstandard_workbook_name.xlsx`, `Simple.xlsb`,
the three formerly unsupported formula XLSB fixtures, `apachepoi_testRVA.xls.xlsb`,
and `formula_stress_test.xlsb`.

Historical buckets eliminated by fixes:

- `internal` 3 → 0. `Simple.xlsb` now opens after its Excel 2007 Beta bundle
  record is normalized in memory. `64130.xls` and the HSSF ClusterFuzz sector
  testcase now return structured `bad_zip` instead of `internal`.
- `unsupported` 3 → 0. Unsupported XLSB formula tokens now produce partial
  formula warnings while preserving cached values.
- `wax:d / SheetJS:n` 532 → 0. Negative date-formatted serials remain numeric
  rather than being clamped to `1899-12-31`.
- `###0.00;-###0.00` display mismatches 14,592 → 0; the format is 100% exact
  over 14,768 cells. Both `49609.xlsx` copies store ZIP entry names with
  backslashes; normalized part lookup now recovers their format metadata.
- The former `wax:missing / SheetJS:s` total fell from 23,268 to 1,195.
  Exact BIFF and XLSB cached-empty-string records restore 22,073 missing cells,
  including an exact 9,959-coordinate match for each `StringContinueRecords`
  XLS/XLSB copy.

The current generated triage has 38 rows: 2 clean round-trip rows, 20 fully
adjudicated rows, 15 signed-off limitation rows, and one split `bad_zip` row.
The split accounts for 2 fixed files, 48 adjudicated files, and 15 signed-off
limitations. Coordinator sign-off was received from `CL.7c63` on 2026-07-28,
including the `excel-reader-xlsx_error02.xlsx` amendment.

## Open-result buckets

| Bucket | Count | Terminal outcome | Evidence |
| --- | ---: | --- | --- |
| `bad_zip` | 65 | Split: 2 fixed, 48 adjudicated, 15 known limitations | The two mandatory former internals are regression-tested. The 48 corrupt/password/container cases are enumerated in `harness/adjudications.md` (empty files, invalid relationships, truncated XML, bad checksums, impossible ZIP/CFB structures, and amplification fixtures). The 15 limitations are listed below. |
| `bomb` | 16 | Adjudicated | All are entity/DTD bombs or BIFF extents above 8,000,000 cells. Per-file evidence and declared sizes are in `harness/adjudications.md`; SheetJS either fails or unsafely accepts the hostile fixture. |

The 15 `bad_zip` known limitations are:

- Four valid macro-sheet packages: `64420.xlsm`, `xlmmacro.xlsm`, and
  `BlankSheetTypes.xlsm`/`.xlsb`. Calamine 0.36.1 recognizes only worksheet,
  chartsheet, and dialogsheet OOXML relationship types at
  `calamine/src/xlsx/mod.rs:497-507` and only worksheet relationships at
  `calamine/src/xlsb/mod.rs:318-328`; macro-sheet content is not exposed.
- Two Excel 2007 Beta OOXML files: `sample-beta.xlsx` and
  `apachepoi_sample-beta.xlsx`. Their shared-string table uses Beta `sstItem`
  elements. Calamine 0.36.1 only collects `si` elements at
  `calamine/src/xlsx/mod.rs:366-375`, then fails the unresolved index at
  `calamine/src/xlsx/cells_reader.rs:648`.
- Two raw pre-CFB BIFF streams: `testEXCEL_3.xls` and `testEXCEL_4.xls` begin
  with BOF records `0x0209`/`0x0409`, not an OLE header. Calamine 0.36.1's XLS
  path requires the CFB signature at `calamine/src/cfb.rs:216-223`.
- Seven suffix/content mismatches: the six `roo_type_{excel,excelx,openoffice}`
  fixtures and `excel-reader-xlsx_error02.xlsx`. `file(1)` identifies the last
  as valid CFB/XLS despite its `.xlsx` suffix. wax selects solely by suffix in
  `crates/wax-read/src/calamine_reader.rs::WorkbookKind::from_path`; content
  sniffing is not yet implemented.

## Value buckets

| Bucket | Count | Terminal outcome | Evidence / pinned root cause |
| --- | ---: | --- | --- |
| `wax:d / SheetJS:d` | 112,091 | Adjudicated | SheetJS drops cached milliseconds; `time_stress_test_1.xlsb`, the Austrian government XLS, and the `number_format`/`LONumbers` suites have stored `.184`, `.083`, `.960`, etc. Per-cell evidence is in `harness/adjudications.md`. |
| `wax:n / SheetJS:d` | 78,398 | Adjudicated | 78,005 cells use elapsed/duration formats and must retain numeric serials; 393 are negative serials that Excel cannot display as 1900-system dates. Both subsets and representative cells are adjudicated separately. |
| `wax:missing / SheetJS:n` | 5,235 | Known limitation | Mostly cached chart-series cells: `12843-1.xls` and its duplicate contribute 4,068 numeric cells from chart sheets, with `34775.xls`, `ex42570-20305.xls`, and their copies supplying most of the rest. Chart content is explicitly excluded by `MISSION.md` §Non-goals; Calamine exposes `SheetType::ChartSheet` (`calamine/src/xls.rs:769-773`) but wax intentionally emits non-worksheets empty in `read_xls`. |
| `wax:s / SheetJS:s` | 2,546 | Known limitation | `no_drawing_patriarch.xlsx` contributes 2,496. At zero-based `sheet 0, r9, c1`, stored `<t>1602` plus 21 spaces `</t>` becomes wax `"1602"`, while SheetJS retains all 25 characters. Calamine—not SheetJS or wax—trims ASCII whitespace when `xml:space="preserve"` is absent at `calamine/src/xlsx/mod.rs:2898-2911`; this is upstream fidelity loss. The remaining 50 are rich-text/legacy encoding cases through the same reader boundary. |
| `wax:missing / SheetJS:s` | 1,195 | Known limitation | Chart-sheet cache strings dominate (`12843-1.xls`, `ex42570-20305.xls`, and `34775.xls` plus duplicates). This is the same `MISSION.md` chart-content non-goal and Calamine chart-sheet boundary above; five `60405.xls` cells and two duplicate-filename cells are unsupported legacy/ambiguous-package residue. |
| `wax:n / SheetJS:missing` | 706 | Known limitation | `external_name.xls` contributes 396 and chart workbooks contribute most of the remainder. Calamine 0.36.1's sparse XLS extraction in `calamine/src/xls.rs:600-650` exposes stored external/chart source cells that SheetJS's normalized range omits; wax has no external-name model to reconcile them. |
| `wax:n / SheetJS:n` | 234 | Known limitation | `25183.xls` plus its duplicate contribute 164 and `external_name.xls` contributes 59. Calamine's XLS cached-value extraction (`calamine/src/xls.rs:600-650`) disagrees with SheetJS on these malformed/shared formula records; representative `25183.xls!B1` is wax `1.0` versus SheetJS `0.25`. |
| `wax:n / SheetJS:s` | 97 | Known limitation | 62 are `external_name.xls`; the remainder are legacy chart/title records in `34775.xls`, `44861.xls`, `45538_classic_Header.xls`, and copies. Root cause is the Calamine XLS record-to-`Data` boundary at `calamine/src/xls.rs:600-650`, with no external-name/chart value type in the frozen model. |
| `wax:s / SheetJS:d` | 53 | Known limitation | 49 are `number_format.ods`, with four more in `roo_time-test.ods` and `formula_stress_test.ods`. Calamine supplies ISO/text values without full ODS number-format metadata; wax pins this boundary in `crates/wax-read/src/calamine_reader.rs::read_ods` with an explicit best-effort warning. |
| `wax:missing / SheetJS:b` | 15 | Known limitation | All are `60405.xls` malformed/unsupported legacy formula records omitted by Calamine's XLS range construction (`calamine/src/xls.rs:600-650`). |
| `wax:s / SheetJS:missing` | 4 | Adjudicated | `bug60858.xlsx!F2,G2,I2,L2` each stores `<f>` but no cached `<v>`. wax preserves formula text with null cache; SheetJS drops the cells. |
| `wax:d / SheetJS:s` | 3 | Known limitation | All are `external_name.xls`; cached external-name values cross Calamine's XLS type conversion without an external-name representation in the normalized model (`calamine/src/xls.rs:600-650`). |
| `wax:missing / SheetJS:d` | 2 | Known limitation | `WrongFormulaRecordType.xls` and its copy deliberately store a formula under the wrong BIFF record type. SheetJS recovers one date per file; Calamine's strict record dispatch does not (`calamine/src/xls.rs:600-650`). |
| `wax:d / SheetJS:missing` | 1 | Known limitation | One `external_name.xls` date is exposed by Calamine outside SheetJS's normalized range; external-name reconciliation is not represented in v1. |

## Display buckets

| Bucket | Count | Terminal outcome | Evidence / pinned root cause |
| --- | ---: | --- | --- |
| `General` | 9,275 | Known limitation | Inherits value/model differences above. `no_drawing_patriarch.xlsx` supplies 2,496 trimmed-space displays; chart/formula fixtures such as `53446.xls`, `57231_MixedGasReport.xls`, and `FormulaEvalTestData.xls` account for most of the balance. Pointers are the Calamine string trim at `xlsx/mod.rs:2898-2911`, XLS cached extraction at `xls.rs:600-650`, and the `MISSION.md` chart non-goal. |
| `m/d/yy` | 6,523 | Known limitation | A mixed legacy-style/date bucket. For example `15228.xls!B3` is stored/read by wax as `mm-dd-yy` (`08-01-98`) while SheetJS substitutes builtin `m/d/yy` (`8/1/98`); other cells inherit missing chart/formula/date caches. wax's BIFF style source is `crates/wax-read/src/xls_styles.rs::parse_workbook_stream`; Calamine cached dates come through `calamine/src/xls.rs:600-650`. |
| `hhmm` | 6,110 | Adjudicated | SheetJS truncates the least-significant displayed minute; wax rounds. See the `time_stress_test_1.xlsb` display adjudication. |
| `[m]` | 6,106 | Adjudicated | Same elapsed-time rounding adjudication. |
| `[mm]` | 6,106 | Adjudicated | Same elapsed-time rounding adjudication. |
| `hm` | 6,106 | Adjudicated | Same time rounding adjudication. |
| `[hh]` | 5,698 | Adjudicated | Same elapsed-hour rounding adjudication; 5,682 are in `time_stress_test_1.xlsb`. |
| `[h]` | 5,690 | Adjudicated | Same elapsed-hour rounding adjudication. At serial `0.2`, wax correctly emits `5`; SheetJS truncates 4.8 hours to `4`. |
| `h` | 5,690 | Adjudicated | Same hour-only rounding adjudication. |
| `hh` | 5,682 | Adjudicated | Same hour-only rounding adjudication. |
| `_(* #,##0_);_(* \(#,##0\);_(* "-"??_);_(@_)` | 2,976 | Adjudicated | Empty cached strings select `_(@_)`; its two underscore-width placeholders produce two spaces. SheetJS drops them. |
| `# ?/?` | 1,668 | Adjudicated | Bounded-denominator evidence: `0.7` is nearer `5/7` (wax) than `2/3` (SheetJS), and `0.1` is nearest `1/9`, not zero. |
| `#\ ??/?????????` | 1,062 | Adjudicated | Same exact bounded-denominator fraction adjudication. |
| `_("$"* #,##0.00_);_("$"* \(#,##0.00\);_("$"* "-"??_);_(@_)` | 723 | Adjudicated | Same accounting text-section whitespace adjudication as the integer form. |
| `0` | 313 | Known limitation | 312 are `formula_stress_test.ods`/`number_format.ods`; Calamine's ODS cached values and formats are explicitly best-effort at `crates/wax-read/src/calamine_reader.rs::read_ods`. |
| `#\ ?/2` | 252 | Adjudicated | Same bounded-denominator fraction adjudication. |
| `0.00` | 175 | Known limitation | 164 are `25183.xls` plus its duplicate and reflect wrong cached values before formatting (`B1`: wax `1.0`, SheetJS `0.25`); 11 are `number_format.ods`. Root pointers are Calamine `xls.rs:600-650` and wax `read_ods`. |
| `# ??/??` | 163 | Adjudicated | Same bounded-denominator fraction adjudication. |
| `[$-1010409]0.000%` | 132 | Adjudicated | The bracket is a locale identifier, not a currency token. wax renders `0.420%`; SheetJS invents `$0.420%`. |
| `#\ ?/4` | 129 | Adjudicated | Same bounded-denominator fraction adjudication. |

## Round-trip buckets

| Bucket | Count | Terminal outcome | Evidence |
| --- | ---: | --- | --- |
| Round-trip merge defects | 0 | Clean | The generated triage reports no disagreements. |
| Oracle read-back failures on wax-clean exports | 0 | Clean | The generated triage reports no disagreements; oracle read-back opened 1,956/1,956 clean exports. |

## Fixed implementation and tests

The reader now normalizes backslash ZIP part names, discovers nonstandard
OOXML workbook parts, normalizes Excel 2007 Beta XLSB bundle records, degrades
unsupported XLSB formula tokens to partial warnings, restores explicit BIFF
and XLSB empty-string formula caches, classifies structural CFB/BIFF failures,
and preserves negative date-formatted serials as numbers. Regression coverage
includes synthetic package tests, record-level BIFF/XLSB tests, and four
machine-corpus tests covering every fixed open/value class.
