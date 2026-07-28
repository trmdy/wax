# wax compatibility scoreboard

Generated: `2026-07-28T00:00:00Z`

Corpus: 3 attempted, 1 skipped.

| Metric | wax | SheetJS baseline |
| --- | ---: | ---: |
| files opened % | 66.67% (2/3) | 100.00% (3/3) |
| cell-value match % | 50.00% (1/2) | reference |
| display-string coverage % | 0.00% (0/2) | 100.00% (2/2) |
| display-string match % | 50.00% (1/2) | reference |
| formula fidelity % | 100.00% (1/1) | reference |
| cached-result fidelity % | 0.00% (0/1) | reference |
| p50 parse time | 10 ms | 20 ms |
| p95 parse time | 12 ms | 22 ms |
| peak RSS (p50 / max) | 100 B / 120 B | 200 B / 220 B |
| window latency | n/a | n/a |

## Per-extension compatibility

The `xlsx` row is the binding W2 reader gate.

| Extension | Files attempted | wax opened | SheetJS opened | Cell-value match |
| --- | ---: | ---: | ---: | ---: |
| <code>ods</code> | 1 | 0.00% (0/1) | 100.00% (1/1) | n/a |
| <code>xlsx</code> (W2 gate) | 2 | 100.00% (2/2) | 100.00% (2/2) | 50.00% (1/2) |

## Top format-code display compatibility

Top 20 ranked by corpus-wide cell count from `harness/formats/corpus-formats.json`.

| Format code | Oracle cells (run / corpus) | wax display coverage | Display match |
| --- | ---: | ---: | ---: |
| <code>#,##0.00&#124;kr</code> | 2 / 50 | 50.00% (1/2) | 100.00% (1/1) |
