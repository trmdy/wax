# wax mismatch triage

Generated: `2026-07-28T02:48:49Z`

Files compared: 2048.

Counts include private corpus files; example paths deliberately omit them.

## Wax open failures by error code

| Category | Occurrences | Example files |
| --- | ---: | --- |
| <code>unsupported</code> | 1410 | <code>corpus/files/openpyxl/3.1.5/openpyxl/packaging/tests/data/sample.xlsm</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/legacy_drawing.xlsm</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/tests/data/reader/example_vba_and_custom_doc_props.xlsm</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/tests/data/reader/example_vba_and_no_custom_doc_props.xlsm</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/tests/data/reader/vba+comments.xlsm</code> |
| <code>bad_zip</code> | 32 | <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/null_file.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/tests/data/reader/nonstandard_workbook_name.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/49609.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/58616.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/60825.xlsx</code> |
| <code>schema_violation</code> | 9 | <code>corpus/files/poi/test-data/spreadsheet/123233_charts.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/52348.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/56278.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/59746_NoRowNums.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/64508.xlsx</code> |

## Value mismatches by type pair

| Category | Occurrences | Example files |
| --- | ---: | --- |
| <code>wax:missing / SheetJS:s</code> | 218897 | <code>corpus/files/openpyxl/3.1.5/openpyxl/comments/tests/data/comments.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/packaging/tests/data/bug137.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/packaging/tests/data/hyperlink.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/bug137.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/complex-styles.xlsx</code> |
| <code>wax:n / SheetJS:d</code> | 22011 | <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/complex-styles.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/empty_with_no_properties.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/sample.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/tests/data/genuine/empty-with-styles.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/tests/data/genuine/mac_date.xlsx</code> |
| <code>wax:missing / SheetJS:e</code> | 2323 | <code>corpus/files/openpyxl/3.1.5/openpyxl/worksheet/tests/data/copy_test.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/46535.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/49783.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/56688_2.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/56688_3.xlsx</code> |
| <code>wax:missing / SheetJS:b</code> | 1961 | <code>corpus/files/openpyxl/3.1.5/openpyxl/tests/data/genuine/sample.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/worksheet/tests/data/copy_test.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/writer/tests/data/empty.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/46535.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/48962.xlsx</code> |
| <code>wax:missing / SheetJS:n</code> | 75 | <code>corpus/files/openpyxl/3.1.5/openpyxl/packaging/tests/data/bug137.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/bug137.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/contains_chartsheets.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/47813.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/60255_extra_drawingparts.xlsx</code> |
| <code>wax:missing / SheetJS:d</code> | 4 | <code>corpus/files/poi/test-data/spreadsheet/SimpleStrict.xlsx</code>, <code>corpus/files/sheetjs/test_files/roo_Pfand_from_windows_phone.xlsx</code>, <code>corpus/files/sheetjs/test_files/xlsx-stream-d-date-cell.xlsx</code> |
| <code>wax:s / SheetJS:s</code> | 2 | <code>corpus/files/poi/test-data/spreadsheet/60289.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/InlineString.xlsx</code> |

## Display mismatches by format code

No disagreements observed.
