# wax mismatch triage

Generated: `2026-07-29T19:32:00Z`

Files compared: 2044.

Counts include private corpus files; example paths deliberately omit them.

## Wax open failures by error code

| Category | Occurrences | Example files |
| --- | ---: | --- |
| <code>bad_zip</code> | 65 | <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/null_file.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/58616.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/60825.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/61045_govdocs1_626534.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/61300.xls</code> |
| <code>bomb</code> | 16 | <code>corpus/files/poi/test-data/spreadsheet/35897-type4.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/50833.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/51535.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/51832.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/54764-2.xlsx</code> |

## Value mismatches by type pair

| Category | Occurrences | Example files |
| --- | ---: | --- |
| <code>wax:d / SheetJS:d</code> | 112091 | <code>corpus/files/poi/test-data/spreadsheet/123233_charts.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/34775.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/45538_classic_Header.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/54071.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/58747.xlsx</code> |
| <code>wax:n / SheetJS:d</code> | 78398 | <code>corpus/files/openpyxl/3.1.5/openpyxl/worksheet/tests/data/test_datetime.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/12561-1.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/57181.xlsm</code>, <code>corpus/files/poi/test-data/spreadsheet/bug60858.xlsx</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2010.xls</code> |
| <code>wax:missing / SheetJS:n</code> | 5235 | <code>corpus/files/openpyxl/3.1.5/openpyxl/packaging/tests/data/bug137.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/bug137.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/contains_chartsheets.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/12843-1.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/34775.xls</code> |
| <code>wax:s / SheetJS:s</code> | 2544 | <code>corpus/files/poi/test-data/spreadsheet/52575_main.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/56278.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/InlineString.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/StringContinueRecords.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/bug69769.xlsx</code> |
| <code>wax:missing / SheetJS:s</code> | 1195 | <code>corpus/files/poi/test-data/spreadsheet/12843-1.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/34775.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/60405.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/duplicate-filename-case-insensitive.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/duplicate-filename.xlsx</code> |
| <code>wax:n / SheetJS:missing</code> | 706 | <code>corpus/files/poi/test-data/spreadsheet/15573.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/25183.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/26100.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/44861.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/45538_classic_Header.xls</code> |
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
| <code>General</code> | 9275 | <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/contains_chartsheets.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/123233_charts.xlsx</code>, <code>corpus/files/poi/test-data/spreadsheet/15228.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/34775.xls</code>, <code>corpus/files/poi/test-data/spreadsheet/43623.xls</code> |
| <code>m/d/yy</code> | 6523 | <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/complex-styles.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/empty_with_no_properties.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/tests/data/genuine/empty-with-styles.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/tests/data/genuine/sample.xlsx</code>, <code>corpus/files/openpyxl/3.1.5/openpyxl/worksheet/tests/data/test_datetime.xlsx</code> |
| <code>hhmm</code> | 6110 | <code>corpus/files/sheetjs/test_files/LONumbers-2010.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2010.xlsx</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2011.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2011.xlsx</code>, <code>corpus/files/sheetjs/test_files/time_stress_test_1.xlsb</code> |
| <code>[m]</code> | 6106 | <code>corpus/files/sheetjs/test_files/time_stress_test_1.xlsb</code> |
| <code>[mm]</code> | 6106 | <code>corpus/files/sheetjs/test_files/time_stress_test_1.xlsb</code> |
| <code>hm</code> | 6106 | <code>corpus/files/sheetjs/test_files/time_stress_test_1.xlsb</code> |
| <code>[hh]</code> | 5698 | <code>corpus/files/sheetjs/test_files/LONumbers-2010.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2010.xlsx</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2011.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2011.xlsx</code>, <code>corpus/files/sheetjs/test_files/time_stress_test_1.xlsb</code> |
| <code>[h]</code> | 5690 | <code>corpus/files/sheetjs/test_files/LONumbers-2010.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2010.xlsx</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2011.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2011.xlsx</code>, <code>corpus/files/sheetjs/test_files/time_stress_test_1.xlsb</code> |
| <code>h</code> | 5690 | <code>corpus/files/sheetjs/test_files/LONumbers-2010.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2010.xlsx</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2011.xls</code>, <code>corpus/files/sheetjs/test_files/LONumbers-2011.xlsx</code>, <code>corpus/files/sheetjs/test_files/time_stress_test_1.xlsb</code> |
| <code>hh</code> | 5682 | <code>corpus/files/sheetjs/test_files/time_stress_test_1.xlsb</code> |
| <code>_(* #,##0_);_(* \(#,##0\);_(* "-"??_);_(@_)</code> | 2976 | <code>corpus/files/poi/test-data/spreadsheet/15228.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_15228.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_15228.xls.xlsb</code> |
| <code># ?/?</code> | 1668 | <code>corpus/files/poi/test-data/spreadsheet/54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls.xlsb</code>, <code>corpus/files/sheetjs/test_files/fraction-test.xls</code>, <code>corpus/files/sheetjs/test_files/number_format.ods</code> |
| <code>#\ ??/?????????</code> | 1062 | <code>corpus/files/poi/test-data/spreadsheet/54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls.xlsb</code> |
| <code>_("$"* #,##0.00_);_("$"* \(#,##0.00\);_("$"* "-"??_);_(@_)</code> | 723 | <code>corpus/files/poi/test-data/spreadsheet/53446.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_53446.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_53446.xls.xlsb</code> |
| <code>0</code> | 313 | <code>corpus/files/sheetjs/test_files/formula_stress_test.ods</code>, <code>corpus/files/sheetjs/test_files/number_format.ods</code>, <code>corpus/files/sheetjs/test_files/roo_bbu.ods</code> |
| <code>#\ ?/2</code> | 252 | <code>corpus/files/poi/test-data/spreadsheet/54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls.xlsb</code> |
| <code>0.00</code> | 175 | <code>corpus/files/poi/test-data/spreadsheet/25183.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_25183.xls</code>, <code>corpus/files/sheetjs/test_files/number_format.ods</code> |
| <code># ??/??</code> | 163 | <code>corpus/files/poi/test-data/spreadsheet/54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls.xlsb</code>, <code>corpus/files/sheetjs/test_files/number_format.ods</code>, <code>corpus/files/sheetjs/test_files/number_format.xls</code> |
| <code>[$-1010409]0.000%</code> | 132 | <code>corpus/files/poi/test-data/spreadsheet/47251.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_47251.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_47251.xls.xlsb</code> |
| <code>#\ ?/4</code> | 129 | <code>corpus/files/poi/test-data/spreadsheet/54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls</code>, <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls.xlsb</code> |

## Round-trip merge defects

No disagreements observed.

## Round-trip export drops

| File | Dropped during export |
| --- | --- |
| <code>corpus/files/openpyxl/3.1.5/openpyxl/packaging/tests/data/bug137.xlsx</code> | <code>sheet "Chart1" is not a worksheet and was emitted empty</code> |
| <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/bug137.xlsx</code> | <code>sheet "Chart1" is not a worksheet and was emitted empty</code> |
| <code>corpus/files/openpyxl/3.1.5/openpyxl/reader/tests/data/contains_chartsheets.xlsx</code> | <code>sheet "chart" is not a worksheet and was emitted empty</code> |
| <code>corpus/files/openpyxl/3.1.5/openpyxl/tests/data/reader/nonstandard_workbook_name.xlsx</code> | <code>nonstandard OOXML workbook part normalized in memory</code> |
| <code>corpus/files/poi/test-data/spreadsheet/31749.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/46368.xls</code> | <code>cell 'Sheet1'!A1 string truncated from 32770 to 32767 characters</code> |
| <code>corpus/files/poi/test-data/spreadsheet/47813.xlsx</code> | <code>sheet "Chart" is not a worksheet and was emitted empty</code> |
| <code>corpus/files/poi/test-data/spreadsheet/49219.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/51519.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/poi/test-data/spreadsheet/52527.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/53404.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/53446.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/56557.xlsx</code> | <code>sheet "Chart" is not a worksheet and was emitted empty</code> |
| <code>corpus/files/poi/test-data/spreadsheet/57003-FixedFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/57181.xlsm</code> | <code>sheet "Detailed_graph" is not a worksheet and was emitted empty</code> |
| <code>corpus/files/poi/test-data/spreadsheet/57181.xlsm</code> | <code>sheet "Summary_graph" is not a worksheet and was emitted empty</code> |
| <code>corpus/files/poi/test-data/spreadsheet/60255_extra_drawingparts.xlsx</code> | <code>sheet "Chart1" is not a worksheet and was emitted empty</code> |
| <code>corpus/files/poi/test-data/spreadsheet/62815.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/poi/test-data/spreadsheet/BOOK_in_capitals.xls</code> | <code>sheet name '301. exposures by cpty and agreement_301_NTTX_EXCEL.rpt' sanitized to '301. exposures by cpty and agre'</code> |
| <code>corpus/files/poi/test-data/spreadsheet/BooleanFunctionsTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/CodeFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/ComplexFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/DGet.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/DateTimeToNumberTestCases.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/DeltaFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/FactDoubleFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/FormulaEvalTestData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/FormulaEvalTestData_Copy.xlsx</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/IfFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/IfNaTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/ImRealFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/ImaginaryFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/IndexFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/IndirectFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/LookupFunctionsTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/MatrixFormulaEvalTestData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/MatrixFormulaEvalTestData.xlsx</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/QuotientFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/ReptFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/RomanFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/SampleSS.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/poi/test-data/spreadsheet/Simple.xlsb</code> | <code>xlsb legacy bundle-sheet layout normalized in memory</code> |
| <code>corpus/files/poi/test-data/spreadsheet/Simple.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/poi/test-data/spreadsheet/SimpleScatterChart.xlsx</code> | <code>sheet "Chart1" is not a worksheet and was emitted empty</code> |
| <code>corpus/files/poi/test-data/spreadsheet/StringContinueRecords.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/Trend.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/TwoOperandNumericFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/WeekNumFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/WeekNumFunctionTestCaseData2013.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/WithChartSheet.xlsx</code> | <code>sheet "Chart2" is not a worksheet and was emitted empty</code> |
| <code>corpus/files/poi/test-data/spreadsheet/WithTextBox.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/poi/test-data/spreadsheet/XSSFSheet.copyRows.xlsx</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/angelo.edu_content_files_19555-nsse-2011-multiyear-benchmark.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/bug66682.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/bug66682.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/poi/test-data/spreadsheet/chart_sheet.xlsx</code> | <code>sheet "Chart1" is not a worksheet and was emitted empty</code> |
| <code>corpus/files/poi/test-data/spreadsheet/comments.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/poi/test-data/spreadsheet/date.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/poi/test-data/spreadsheet/external_name.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/poi/test-data/spreadsheet/hyperlink.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/poi/test-data/spreadsheet/sample.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/poi/test-data/spreadsheet/testVarious.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/poi/test-data/spreadsheet/xssf-enum.xltx.xlsx</code> | <code>sheet name 'Контрольний список  "Школа"_x000a_' sanitized to 'Контрольний список  "Школа"_x00'</code> |
| <code>corpus/files/sheetjs/test_files/AutoFilter.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/AutoFilter.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/BlankSheetTypes.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/NumberFormatCondition.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/RkNumber.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_12561-1.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_12561-2.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_12843-1.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_12843-2.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_13224.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_13796.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_14330-1.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_14330-2.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_14460.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_15228.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_15375.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_15556.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_15573.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_1900DateWindowing.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_1904DateWindowing.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_19599-1.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_19599-2.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_22742.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_24207.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_24215.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_25183.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_25695.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_26100.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_27272_1.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_27272_2.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_27349-vlookupAcrossSheets.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_27364.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_27394.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_27852.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_27933.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_28772.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_28774.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_29675.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_29942.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_29982.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_30070.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_30540.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_30978-alt.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_30978-deleted.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_31661.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_31749.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_31749.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_31749.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_31979.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_32822.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_33082.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_34775.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_35564.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_35565.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_36947.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_37376.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_37630.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_37684-1.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_37684-2.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_37684.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_39234.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_39512.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_39634.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_3dFormulas.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_40285.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_41139.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_41546.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_42464-ExpPtg-bad.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_42464-ExpPtg-ok.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_42726.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_42844.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_43251.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_43623.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_43902.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_44010-SingleChart.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_44010-TwoCharts.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_44167.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_44200.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_44201.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_44235.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_44297.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_44593.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_44636.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_44643.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_44693.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_44840.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_44861.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_44891.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_44958.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45129.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45290.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45322.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45365-2.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45365.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45430.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45431.xlsm.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45492.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45538_classic_Footer.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45538_classic_Header.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45538_form_Footer.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45538_form_Header.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45540_classic_Footer.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45540_classic_Header.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45540_form_Footer.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45540_form_Header.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45544.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45672.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45720.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45761.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_45784.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_46136-NoWarnings.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_46136-WithWarnings.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_46137.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_46250.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_46368.xls</code> | <code>cell 'Sheet1'!A1 string truncated from 32770 to 32767 characters</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_46445.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_46535.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_46536.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47026.xlsm.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47034.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47089.xlsm.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47090.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47154.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47251.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47504.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47668.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47701.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47737.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47804.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47813.xlsx</code> | <code>sheet "Chart" is not a worksheet and was emitted empty</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47813.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47847.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47862.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47889.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47920.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_47924.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_48026.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_48180.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_48325.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_48495.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_48539.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_48703.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_48703.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_48779.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_48923.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_48968.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49096.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49156.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49185.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49219.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49219.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49219.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49237.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49273.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49325.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49524.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49529.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49581.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49609.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49612.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49751.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49761.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49761.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49783.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49872.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49896.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49928.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49928.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49931.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_49966.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_50020.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_50096.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_50298.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_50299.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_50426.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_50756.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_50779_1.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_50779_2.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_50784-font_theme_colours.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_50786-indexed_colours.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_50795.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_50833.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_50846-border_colours.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_50867_with_table.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_50939.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_51143.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_51222.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_51461.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_51470.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_51498.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_51535.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_51585.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_51670.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_51675.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_51710.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_51850.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_51963.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_52348.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_52527.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_52527.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_52527.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_52575_main.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_52575_source.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_52716.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_53101.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_53282.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_53282b.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_53404.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_53404.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_53404.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_53446.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_53446.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_53446.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_53568.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_53588.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_53734.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_53798.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_53798_shiftNegative_TMPL.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_53798_shiftNegative_TMPL.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_53972.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_54016.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_54071.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_54084 - Greek - beyond BMP.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_54206.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_54206.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_54288-ref.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_54288.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_54436.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_54500.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_54524.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_54607.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_54686_fraction_formats.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_55341_CellStyleBorder.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_55640.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_55745.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_55850.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_55923.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_55924.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_55926.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_55927.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_56011.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_56017.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_AbnormalSharedFormulaFlag.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_AreaErrPtg.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_AverageTaxRates.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_BOOK_in_capitals.xls</code> | <code>sheet name '301. exposures by cpty and agreement_301_NTTX_EXCEL.rpt' sanitized to '301. exposures by cpty and agre'</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_BOOK_in_capitals.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_Booleans.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_BrNotClosed.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_CodeFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_CodeFunctionTestCaseData.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_CodeFunctionTestCaseData.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ColumnStyle1dp.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ColumnStyle1dpColoured.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ColumnStyleNone.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ComplexFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ComplexFunctionTestCaseData.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ComplexFunctionTestCaseData.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ContinueRecordProblem.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_CustomXMLMapping-singleattributenamespace.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_CustomXMLMappings-complex-type.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_CustomXMLMappings.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_CustomXmlMappings-inverse-order.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_DBCSHeader.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_DBCSSheetName.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_DataValidations-49244.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_DateFormatTests.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_DateFormats.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_DeltaFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_DeltaFunctionTestCaseData.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_DeltaFunctionTestCaseData.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_DrawingAndComments.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_DrawingContinue.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ElapsedFormatTests.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_EmbeddedChartHeaderTest.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_Employee.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ErrPtg.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ExcelWithAttachments.xlsm.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_FactDoubleFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_FactDoubleFunctionTestCaseData.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_FactDoubleFunctionTestCaseData.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ForShifting.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ForShifting.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_FormatChoiceTests.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_FormatChoiceTests.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_FormatConditionTests.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_Formatting.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_Formatting.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_FormulaEvalTestData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_FormulaEvalTestData.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_FormulaEvalTestData.xls.xlsb</code> | <code>xlsb formulas for sheet "EverythingTests" are partial: Unsupported Ptf 46</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_FormulaEvalTestData.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_FormulaEvalTestData_Copy.xlsx</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_FormulaEvalTestData_Copy.xlsx.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_FormulaEvalTestData_Copy.xlsx.xlsb</code> | <code>xlsb formulas for sheet "EverythingTests" are partial: Unsupported Ptf 0</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_FormulaEvalTestData_Copy.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_FormulaRefs.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_GeneralFormatTests.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_GroupTest.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_HyperlinksOnManySheets.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_IfFormulaTest.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ImRealFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ImRealFunctionTestCaseData.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ImRealFunctionTestCaseData.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ImaginaryFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ImaginaryFunctionTestCaseData.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ImaginaryFunctionTestCaseData.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_IndexFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_IndexFunctionTestCaseData.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_IndexFunctionTestCaseData.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_IndirectFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_IndirectFunctionTestCaseData.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_IndirectFunctionTestCaseData.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_InlineStrings.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_IntersectionPtg.xls.xlsb</code> | <code>xlsb formulas for sheet "Sheet1" are partial: Unsupported Ptf 26</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_IntersectionPtg.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_IrrNpvTestCaseData.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_LookupFunctionsTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_LookupFunctionsTestCaseData.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_LookupFunctionsTestCaseData.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_MRExtraLines.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_MatchFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_MatchFunctionTestCaseData.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_MatchFunctionTestCaseData.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_MissingBits.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_NewlineInFormulas.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_NoGutsRecords.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_NumberFormatApproxTests.xlsx.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_NumberFormatApproxTests.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_NumberFormatTests.xlsx.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_NumberFormatTests.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_PercentPtg.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_QuotientFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_QuotientFunctionTestCaseData.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_QuotientFunctionTestCaseData.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_RangePtg.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ReadOnlyRecommended.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ReferencePtg.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_RepeatingRowsCols.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_RepeatingRowsCols.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ReptFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ReptFunctionTestCaseData.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ReptFunctionTestCaseData.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_RomanFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_RomanFunctionTestCaseData.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_RomanFunctionTestCaseData.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SampleSS.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SampleSS.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SampleSS.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SharedFormulaTest.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SheetWithDrawing.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ShrinkToFit.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ShrinkToFit.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_Simple.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SimpleChart.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SimpleMultiCell.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SimpleMultiCell.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SimpleWithAutofilter.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SimpleWithChoose.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SimpleWithColours.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SimpleWithComments.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SimpleWithComments.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SimpleWithDataFormat.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SimpleWithFormula.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SimpleWithImages-mac.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SimpleWithImages.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SimpleWithPageBreaks.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SimpleWithPrintArea.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SimpleWithSkip.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SimpleWithStyling.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SingleLetterRanges.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SolverContainerAfterSPGR.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SquareMacro.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_StringContinueRecords.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_StringContinueRecords.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_StringContinueRecords.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_StringFormulas.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_SubtotalsNested.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_Tables.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_TestRandBetween.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_TextFormatTests.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_TwoSheetsNoneHidden.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_TwoSheetsNoneHidden.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_TwoSheetsOneHidden.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_TwoSheetsOneHidden.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_UncalcedRecord.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_UnionPtg.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WORKBOOK_in_capitals.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WeekNumFunctionTestCaseData.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WeekNumFunctionTestCaseData.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WeekNumFunctionTestCaseData.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WeekNumFunctionTestCaseData2013.xls</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WeekNumFunctionTestCaseData2013.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WeekNumFunctionTestCaseData2013.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithChart.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithChart.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithChartSheet.xlsx</code> | <code>sheet "Chart2" is not a worksheet and was emitted empty</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithChartSheet.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithCheckBoxes.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithConditionalFormatting.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithConditionalFormatting.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithDrawing.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithEmbeddedObjects.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithEmbeded.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithExtendedStyles.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithFormattedGraphTitle.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithHyperlink.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithMoreVariousData.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithTable.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithTextBox.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithTextBox2.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithThreeCharts.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithThreeCharts.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithTwoCharts.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithTwoCharts.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithTwoHyperLinks.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WithVariousData.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_WrongFormulaRecordType.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_XRefCalc.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_XRefCalcData.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_atp.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_atp.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_blankworkbook.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_bug_42794.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_chart_sheet.xlsx</code> | <code>sheet "Chart1" is not a worksheet and was emitted empty</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_chart_sheet.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_colwidth.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_comments.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_comments.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_countblankExamples.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_countifExamples.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_dg-text.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_drawings.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_duprich1.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_duprich2.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_dvEmpty.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_empty.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ex41187-19267.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ex42564-21435.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ex42564-21503.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ex42564-elementOrder.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ex42570-20305.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ex44921-21902.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ex45046-21984.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ex45582-22397.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ex45672.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ex45978-extraLinkTableSheets.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ex46548-23133.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ex47747-sharedFormula.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_excel_with_embeded.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_excelant.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_externalFunctionExample.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_finance.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_headerFooterTest.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_intercept.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_mirrTest.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_missingFuncs44675.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_mortgage-calculation.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_multibookFormulaA.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_multibookFormulaB.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_namedinput.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_noHeaderFooter47244.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_ole2-embedding.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_overlapSharedFormula.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_picture.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_rank.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_reordered_sheets.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_rk.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_sample.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_shared_formulas.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_shared_formulas.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_sheetProtection_allLocked.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_sheetProtection_not_protected.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_styles.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_sumifformula.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_sumifs.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_templateExcelWithAutofilter.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_testArraysAndTables.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_testNames.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_testRRaC.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_testRVA.xls.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_testRVA.xls.xlsb</code> | <code>xlsb formulas for sheet "Sheet1" are partial: Invalid stack length</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_testRVA.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_text.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_unicodeNameRecord.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_workbookProtection_not_protected.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_workbookProtection_workbook_revision_protected.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_workbookProtection_workbook_structure_protected.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_workbookProtection_workbook_windows_protected.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_workbookProtection_worksheet_protected.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_xlsx-jdbc.xlsx.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/apachepoi_yearfracExamples.xls.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/cell_style_simple.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/comments_stress_test.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/custom_properties.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/formula_stress_test.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/formula_stress_test.xlsb</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/formula_stress_test.xlsb</code> | <code>xlsb formulas for sheet "Engineering" are partial: Invalid stack length</code> |
| <code>corpus/files/sheetjs/test_files/formula_stress_test.xlsb</code> | <code>xlsb formulas for sheet "Lookup" are partial: Unsupported Ptf 26</code> |
| <code>corpus/files/sheetjs/test_files/formula_stress_test.xlsb</code> | <code>xlsb formulas for sheet "Math" are partial: Invalid stack length</code> |
| <code>corpus/files/sheetjs/test_files/formula_stress_test.xlsb</code> | <code>xlsb formulas for sheet "Text" are partial: Invalid stack length</code> |
| <code>corpus/files/sheetjs/test_files/formula_stress_test.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/large_strings.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/merge_cells.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/merge_cells.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/named_ranges_2011.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/number_format.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/number_format.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/openpyxl_r_contains_chartsheets.xlsx</code> | <code>sheet "chart" is not a worksheet and was emitted empty</code> |
| <code>corpus/files/sheetjs/test_files/pivot_table_named_range.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/pivot_table_test.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/rich_text_stress.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/rich_text_stress.xlsb</code> | <code>xlsb merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_Bibelbund.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_Bibelbund1.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_advanced_header.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_bbu.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_boolean.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_borders.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_comments.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_datetime.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_dreimalvier.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_emptysheets.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_formula.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_hidden_sheets.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_html-escape.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_matrix.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_named_cells.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_numbers1.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_only_one_sheet.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_paragraph.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_ric.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_simple_spreadsheet.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_simple_spreadsheet_from_italo.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_style.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_time-test.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/roo_whitespace.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/sushi.ods</code> | <code>ods number-format codes and merged regions are best-effort</code> |
| <code>corpus/files/sheetjs/test_files/xlrd_reveng1.xlsx</code> | <code>error cells written as text</code> |
| <code>corpus/files/sheetjs/test_files/xlsx-stream-d-date-cell.xlsb</code> | <code>xlsb merged regions are best-effort</code> |

## Round-trip failures

| File | Failure |
| --- | --- |
| <code>corpus/files/sheetjs/test_files/excel-reader-xlsx_error08.xlsx</code> | <code>export: bad_request: sheet index 0 is out of range</code> |

## Oracle read-back failures on wax-clean exports

No disagreements observed.
