# wax mismatch triage

Generated: `2026-07-28T18:45:19Z`

Files compared: 2044.

Counts include private corpus files; example paths deliberately omit them.

## Wax open failures by error code

| Category | Occurrences | Example files |
| --- | ---: | --- |
| <code>bad_zip</code> | 66 | <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/null_file.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/tests/data/reader/nonstandard_workbook_name.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/58616.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/60825.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/61045_govdocs1_626534.xls</code> |
| <code>bomb</code> | 16 | <code>corpus/files/poi/test-data/spreadsheet/35897-type4.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/50833.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/51535.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/51832.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/54764-2.xlsx</code> |
| <code>internal</code> | 3 | <code>corpus/files/poi/test-data/spreadsheet/64130.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/Simple.xlsb</code>, <code>corpus/files/poi/test-data/spreadsheet/clusterfuzz-testcase-minimized-POIHSSFFuzzer-5436547081830400.xls</code> |
| <code>unsupported</code> | 3 | <code>corpus/files/sheetjs/test_files/apachepoi_FormulaEvalTestData.xls.xlsb</code>, <code>corpus/files/sheetjs/test_files/apachepoi_FormulaEvalTestData_Copy.xlsx.xlsb</code>, <code>corpus/files/sheetjs/test_files/apachepoi_IntersectionPtg.xls.xlsb</code> |

## Value mismatches by type pair

| Category | Occurrences | Example files |
| --- | ---: | --- |
| <code>wax:d / SheetJS:d</code> | 112483 | <code>corpus/files/poi/test-data/spreadsheet/123233_charts.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/34775.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/45538_classic_Header.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/54071.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/58747.xlsx</code> |
| <code>wax:n / SheetJS:d</code> | 78005 | <code>corpus/files/openpyxl/3.1.5/openpyxl/worksheet/tests/data/test_datetime.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/12561-1.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/57181.xlsm</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2010.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2010.xlsx</code> |
| <code>wax:missing / SheetJS:s</code> | 23268 | <code>corpus/files/poi/test-data/spreadsheet/12843-1.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/15228.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/34775.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/44891.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/54206.xls</code> |
| <code>wax:missing / SheetJS:n</code> | 5235 | <code>corpus/files/openpyxl/3.1.5/openpyxl/packaging/tests/data/bug137.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/bug137.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/contains_chartsheets.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/12843-1.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/34775.xls</code> |
| <code>wax:s / SheetJS:s</code> | 2544 | <code>corpus/files/poi/test-data/spreadsheet/52575_main.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/56278.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/InlineString.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/StringContinueRecords.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/bug69769.xlsx</code> |
| <code>wax:n / SheetJS:missing</code> | 706 | <code>corpus/files/poi/test-data/spreadsheet/15573.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/25183.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/26100.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/44861.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/45538_classic_Header.xls</code> |
| <code>wax:d / SheetJS:n</code> | 532 | <code>corpus/files/sheetjs/test_files/LONumbers-2010.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2011.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers.xls</code>, <code>corpus/files/sheetjs/test_files/number_format.xls</code>, <code>corpus/files/sheetjs/test_files/number_format.xlsb</code> |
| <code>wax:n / SheetJS:n</code> | 234 | <code>corpus/files/poi/test-data/spreadsheet/25183.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/external_name.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/florida_data.ashx.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_25183.xls</code>, <code>corpus/files/sheetjs/test_files/pyExcelerator_chart1v8.xls</code> |
| <code>wax:n / SheetJS:s</code> | 97 | <code>corpus/files/poi/test-data/spreadsheet/34775.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/44861.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/45538_classic_Header.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/WithFormattedGraphTitle.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/external_name.xls</code> |
| <code>wax:s / SheetJS:d</code> | 53 | <code>corpus/files/sheetjs/test_files/formula_stress_test.ods</code>, <code>corpus/files/sheetjs/test_files/number_format.ods</code>, <code>corpus/files/sheetjs/test_files/roo_time-test.ods</code> |
| <code>wax:missing / SheetJS:b</code> | 15 | <code>corpus/files/poi/test-data/spreadsheet/60405.xls</code> |
| <code>wax:s / SheetJS:missing</code> | 4 | <code>corpus/files/poi/test-data/spreadsheet/bug60858.xlsx</code> |
| <code>wax:d / SheetJS:s</code> | 3 | <code>corpus/files/poi/test-data/spreadsheet/external_name.xls</code> |
| <code>wax:missing / SheetJS:d</code> | 2 | <code>corpus/files/poi/test-data/spreadsheet/WrongFormulaRecordType.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_WrongFormulaRecordType.xls</code> |
| <code>wax:d / SheetJS:missing</code> | 1 | <code>corpus/files/poi/test-data/spreadsheet/external_name.xls</code> |

## Display mismatches by format code

| Category | Occurrences | Example files |
| --- | ---: | --- |
| <code>###0.00;-###0.00</code> | 14592 | <code>corpus/files/poi/test-data/spreadsheet/49609.xlsx</code>, <code>corpus/files/sheetjs/test_files/apachepoi_49609.xlsx</code> |
| <code>General</code> | 8537 | <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/contains_chartsheets.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/123233_charts.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/15228.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/34775.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/43623.xls</code> |
| <code>m/d/yy</code> | 6504 | <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/complex-styles.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/empty_with_no_properties.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/tests/data/genuine/empty-with-styles.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/tests/data/genuine/sample.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/worksheet/tests/data/test_datetime.xlsx</code> |
| <code>hhmm</code> | 6110 | <code>corpus/files/sheetjs/test_files/LONumbers-2010.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2010.xlsx</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2011.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2011.xlsx</code>, <code>corpus/files/sheetjs/test_files/time_stress_test_1.xlsb</code> |
| <code>[m]</code> | 6106 | <code>corpus/files/sheetjs/test_files/time_stress_test_1.xlsb</code> |
| <code>[mm]</code> | 6106 | <code>corpus/files/sheetjs/test_files/time_stress_test_1.xlsb</code> |
| <code>hm</code> | 6106 | <code>corpus/files/sheetjs/test_files/time_stress_test_1.xlsb</code> |
| <code>[hh]</code> | 5698 | <code>corpus/files/sheetjs/test_files/LONumbers-2010.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2010.xlsx</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2011.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2011.xlsx</code>, <code>corpus/files/sheetjs/test_files/time_stress_test_1.xlsb</code> |
| <code>[h]</code> | 5690 | <code>corpus/files/sheetjs/test_files/LONumbers-2010.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2010.xlsx</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2011.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2011.xlsx</code>, <code>corpus/files/sheetjs/test_files/time_stress_test_1.xlsb</code> |
| <code>h</code> | 5690 | <code>corpus/files/sheetjs/test_files/LONumbers-2010.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2010.xlsx</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2011.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2011.xlsx</code>, <code>corpus/files/sheetjs/test_files/time_stress_test_1.xlsb</code> |
| <code>hh</code> | 5682 | <code>corpus/files/sheetjs/test_files/time_stress_test_1.xlsb</code> |
| <code># ?/?</code> | 1668 | <code>corpus/files/poi/test-data/spreadsheet/54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls.xlsb</code>, <code>corpus/files/sheetjs/test_files/fraction-test.xls</code>, <code>corpus/files/sheetjs/test_files/number_format.ods</code> |
| <code>#\ ??/?????????</code> | 1062 | <code>corpus/files/poi/test-data/spreadsheet/54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls.xlsb</code> |
| <code>_("$"* #,##0.00_);_("$"* \(#,##0.00\);_("$"* "-"??_);_(@_)</code> | 723 | <code>corpus/files/poi/test-data/spreadsheet/53446.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_53446.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_53446.xls.xlsb</code> |
| <code>0</code> | 313 | <code>corpus/files/sheetjs/test_files/formula_stress_test.ods</code>, <code>corpus/files/sheetjs/test_files/number_format.ods</code>, <code>corpus/files/sheetjs/test_files/roo_bbu.ods</code> |
| <code>#\ ?/2</code> | 252 | <code>corpus/files/poi/test-data/spreadsheet/54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls.xlsb</code> |
| <code>0.00</code> | 175 | <code>corpus/files/poi/test-data/spreadsheet/25183.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_25183.xls</code>, <code>corpus/files/sheetjs/test_files/number_format.ods</code> |
| <code># ??/??</code> | 163 | <code>corpus/files/poi/test-data/spreadsheet/54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls.xlsb</code>, <code>corpus/files/sheetjs/test_files/number_format.ods</code>, <code>corpus/files/sheetjs/test_files/number_format.xls</code> |
| <code>_(* #,##0_);_(* \(#,##0\);_(* "-"??_);_(@_)</code> | 156 | <code>corpus/files/poi/test-data/spreadsheet/15228.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_15228.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_15228.xls.xlsb</code> |
| <code>[$-1010409]0.000%</code> | 132 | <code>corpus/files/poi/test-data/spreadsheet/47251.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_47251.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_47251.xls.xlsb</code> |

## Round-trip merge defects

No disagreements observed.

## Oracle read-back failures on wax-clean exports

No disagreements observed.
