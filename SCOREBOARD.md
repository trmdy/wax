# wax compatibility scoreboard

Generated: `2026-07-28T02:48:49Z`

Corpus: 2048 attempted, 0 skipped.

| Metric | wax | SheetJS baseline |
| --- | ---: | ---: |
| files opened % | 29.15% (597/2048) | 97.17% (1990/2048) |
| cell-value match % | 64.63% (448091/693364) | reference |
| display-string coverage % | 0.00% (0/470104) | 99.99% (2922256/2922657) |
| display-string match % | n/a | n/a |
| formula fidelity % | 0.00% (0/72609) | reference |
| cached-result fidelity % | 59.01% (42844/72609) | reference |
| p50 parse time | 0 ms | 8 ms |
| p95 parse time | 0 ms | 88 ms |
| peak RSS (p50 / max) | 1.69 MiB / 63.09 MiB | 88.98 MiB / 1.16 GiB |
| window latency | n/a | n/a |

## Per-extension compatibility

The `xlsx` row is the binding W2 reader gate.

| Extension | Files attempted | wax opened | SheetJS opened | Cell-value match |
| --- | ---: | ---: | ---: | ---: |
| <code>ods</code> | 37 | 0.00% (0/37) | 91.89% (34/37) | n/a |
| <code>xls</code> | 885 | 0.00% (0/885) | 96.95% (858/885) | n/a |
| <code>xlsb</code> | 456 | 0.00% (0/456) | 99.34% (453/456) | n/a |
| <code>xlsm</code> | 32 | 0.00% (0/32) | 100.00% (32/32) | n/a |
| <code>xlsx</code> (W2 gate) | 638 | 93.57% (597/638) | 96.08% (613/638) | 64.63% (448091/693364) |

## Top format-code display compatibility

Top 20 ranked by corpus-wide cell count from `harness/formats/corpus-formats.json`.

| Format code | Oracle cells (run / corpus) | wax display coverage | Display match |
| --- | ---: | ---: | ---: |
| <code>#,##0 ;[Red](#,##0)</code> | 106179 / 106179 | 0.00% (0/106179) | n/a |
| <code>_("$"* #,##0.00_);_("$"* \(#,##0.00\);_("$"* "-"??_);_(@_)</code> | 67502 / 67502 | 0.00% (0/67502) | n/a |
| <code>#,##0.00</code> | 55547 / 55547 | 0.00% (0/55547) | n/a |
| <code>@</code> | 52278 / 52278 | 0.00% (0/52278) | n/a |
| <code>_(* #,##0.00_);_(* \(#,##0.00\);_(* "-"??_);_(@_)</code> | 41735 / 41735 | 0.00% (0/41735) | n/a |
| <code>#,##0</code> | 32964 / 32964 | 0.00% (0/32964) | n/a |
| <code>_(* #,##0_);_(* \(#,##0\);_(* "-"??_);_(@_)</code> | 26484 / 26484 | 0.00% (0/26484) | n/a |
| <code>#,##0 ;(#,##0)</code> | 24287 / 24287 | 0.00% (0/24287) | n/a |
| <code>"$"#,##0_);[Red]\("$"#,##0\)</code> | 21850 / 21850 | 0.00% (0/21850) | n/a |
| <code>dd\.mm\.yyyy</code> | 19455 / 19455 | 0.00% (0/19455) | n/a |
| <code>0.00</code> | 18188 / 18189 | 0.00% (0/18188) | n/a |
| <code>_-* #,##0.00" TL"_-;\-* #,##0.00" TL"_-;_-* \-??" TL"_-;_-@_-</code> | 18018 / 18018 | 0.00% (0/18018) | n/a |
| <code>"$"#,##0.00</code> | 16301 / 16301 | 0.00% (0/16301) | n/a |
| <code>0</code> | 15255 / 15275 | 0.00% (0/15255) | n/a |
| <code>###0.00;-###0.00</code> | 14768 / 14768 | 0.00% (0/14768) | n/a |
| <code>[hh]</code> | 12572 / 12572 | 0.00% (0/12572) | n/a |
| <code>[h]</code> | 12536 / 12536 | 0.00% (0/12536) | n/a |
| <code>[s]</code> | 12536 / 12536 | 0.00% (0/12536) | n/a |
| <code>[ss]</code> | 12536 / 12536 | 0.00% (0/12536) | n/a |
| <code>h</code> | 12536 / 12536 | 0.00% (0/12536) | n/a |
