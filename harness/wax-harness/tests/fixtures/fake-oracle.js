#!/usr/bin/env node

const path = require("node:path");

const file = process.argv[2];
const name = path.basename(file);
const fixture = name === "diff.xlsx"
  ? { sha256: "diff-sha", value: 3, formula: " SUM ( A1 ) " }
  : name === "crash.xlsx"
    ? { sha256: "crash-sha", value: null, formula: null }
    : { sha256: "match-sha", value: 1, formula: null };
const cells = fixture.value === null
  ? []
  : [{ r: 0, c: 0, t: "n", v: fixture.value, d: String(fixture.value), f: fixture.formula, fmt: null }];

process.stdout.write(JSON.stringify({
  schema: 1,
  tool: "sheetjs",
  toolVersion: "0.20.3",
  file,
  sha256: fixture.sha256,
  ok: true,
  error: null,
  wallMs: 20,
  peakRssBytes: 200,
  truncated: false,
  sheets: [{
    name: "Sheet1",
    index: 0,
    rows: cells.length,
    cols: cells.length,
    truncated: false,
    merges: [],
    cells,
  }],
  warnings: [],
}));
