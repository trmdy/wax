"use strict";

const assert = require("node:assert/strict");
const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const { spawnSync } = require("node:child_process");
const test = require("node:test");
const {
  CELL_FIELDS,
  SHEET_FIELDS,
  TOP_LEVEL_FIELDS,
  validateNormalizedDump,
} = require("../schema");

const oracleDirectory = path.resolve(__dirname, "..");
const repoRoot = path.resolve(oracleDirectory, "../..");
const runner = path.join(oracleDirectory, "run.js");
const fixture = path.join(__dirname, "fixtures", "oracle.xlsx");
const date1904Fixture = path.join(
  __dirname,
  "fixtures",
  "date-1904.xlsx",
);
const garbage = path.join(__dirname, "fixtures", "garbage.xlsx");

function invoke(...arguments_) {
  return spawnSync(process.execPath, [runner, ...arguments_], {
    cwd: repoRoot,
    encoding: "utf8",
    timeout: 10_000,
  });
}

function invokeDocument(...arguments_) {
  const result = invoke(...arguments_);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stderr, "");
  assert.notEqual(result.stdout, "");
  assert.equal(result.stdout.trim(), result.stdout);
  return JSON.parse(result.stdout);
}

test("emits a complete schema-1 document", () => {
  const document = invokeDocument(fixture);
  const validation = validateNormalizedDump(document);

  assert.deepEqual(validation.errors, []);
  assert.equal(validation.valid, true);
  assert.deepEqual(Object.keys(document), TOP_LEVEL_FIELDS);
  assert.equal(document.schema, 1);
  assert.equal(document.tool, "sheetjs");
  assert.equal(document.toolVersion, "0.20.3");
  assert.equal(document.file, "harness/oracle/test/fixtures/oracle.xlsx");
  assert.equal(document.ok, true);
  assert.equal(document.error, null);
  assert.equal(document.truncated, false);
  assert.equal(document.sheets.length, 1);
  assert.deepEqual(Object.keys(document.sheets[0]), SHEET_FIELDS);

  for (const cell of document.sheets[0].cells) {
    assert.deepEqual(Object.keys(cell), CELL_FIELDS);
  }

  const expectedHash = crypto
    .createHash("sha256")
    .update(fs.readFileSync(fixture))
    .digest("hex");
  assert.equal(document.sha256, expectedHash);
  assert.ok(document.wallMs >= 0);
  assert.ok(document.peakRssBytes > 0);
});

test("preserves cached formula values, merges, dates, and formats", () => {
  const document = invokeDocument(fixture);
  const sheet = document.sheets[0];

  assert.equal(sheet.name, "Oracle");
  assert.equal(sheet.index, 0);
  assert.equal(sheet.rows, 3);
  assert.equal(sheet.cols, 3);
  assert.deepEqual(sheet.merges, ["A2:B2"]);

  const formula = sheet.cells.find((cell) => cell.r === 0 && cell.c === 2);
  assert.deepEqual(formula, {
    r: 0,
    c: 2,
    t: "n",
    v: 3,
    d: "3",
    f: "A1+B1",
    fmt: null,
  });

  const date = sheet.cells.find((cell) => cell.r === 2 && cell.c === 0);
  assert.equal(date.t, "d");
  assert.equal(date.v, "2024-02-29");
  assert.equal(date.f, null);
  assert.equal(date.fmt, "yyyy-mm-dd");
});

test("applies the cell cap across the whole document", () => {
  const document = invokeDocument(fixture, "--max-cells", "2");
  const validation = validateNormalizedDump(document);

  assert.equal(validation.valid, true, validation.errors.join("\n"));
  assert.equal(document.truncated, true);
  assert.equal(document.sheets[0].truncated, true);
  assert.equal(document.sheets[0].cells.length, 2);
  assert.deepEqual(
    document.sheets[0].cells.map(({ r, c }) => [r, c]),
    [
      [0, 0],
      [0, 1],
    ],
  );
});

test("resolves the workbook 1904 date epoch", () => {
  const document = invokeDocument(date1904Fixture);

  assert.equal(document.ok, true);
  assert.deepEqual(document.sheets[0].cells[0], {
    r: 0,
    c: 0,
    t: "d",
    v: "1904-01-01",
    d: "1904-01-01",
    f: null,
    fmt: "yyyy-mm-dd",
  });
});

test("returns a structured ok:false document for unreadable input", () => {
  const document = invokeDocument(garbage);
  const validation = validateNormalizedDump(document);

  assert.equal(validation.valid, true, validation.errors.join("\n"));
  assert.equal(document.ok, false);
  assert.equal(document.error.code, "unreadable");
  assert.equal(typeof document.error.msg, "string");
  assert.notEqual(document.error.msg, "");
  assert.deepEqual(document.sheets, []);
});

test("uses exit 0 for a missing file data point", () => {
  const document = invokeDocument(
    path.join(__dirname, "fixtures", "does-not-exist.xlsx"),
  );

  assert.equal(document.ok, false);
  assert.equal(document.error.code, "not_found");
  assert.equal(document.sha256, null);
  assert.equal(validateNormalizedDump(document).valid, true);
});

test("enforces a requested parse timeout", () => {
  const document = invokeDocument(fixture, "--timeout-ms", "1");

  assert.equal(document.ok, false);
  assert.equal(document.error.code, "timeout");
  assert.equal(document.sha256, null);
  assert.equal(validateNormalizedDump(document).valid, true);
});

test("uses exit 2 and no stdout for usage errors", () => {
  const result = invoke(fixture, "--max-cells", "many");

  assert.equal(result.status, 2);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /--max-cells must be an integer/);
  assert.match(result.stderr, /Usage:/);
});

test("validator reports missing fields instead of throwing", () => {
  const validation = validateNormalizedDump({
    schema: 1,
    tool: "sheetjs",
  });

  assert.equal(validation.valid, false);
  assert.ok(validation.errors.includes("document.toolVersion is missing"));
  assert.ok(validation.errors.includes("document.sheets is missing"));
});
