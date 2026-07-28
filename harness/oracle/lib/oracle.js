"use strict";

const crypto = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");
const { performance } = require("node:perf_hooks");
const SHEETJS_PACKAGE_PATH = path.join(
  path.dirname(require.resolve("xlsx")),
  "package.json",
);
const SHEETJS_VERSION = JSON.parse(
  fs.readFileSync(SHEETJS_PACKAGE_PATH, "utf8"),
).version;

const REPO_ROOT = path.resolve(__dirname, "../../..");
let sheetJsModule = null;

const ERROR_TEXT = new Map([
  [0x00, "#NULL!"],
  [0x07, "#DIV/0!"],
  [0x0f, "#VALUE!"],
  [0x17, "#REF!"],
  [0x1d, "#NAME?"],
  [0x24, "#NUM!"],
  [0x2a, "#N/A"],
  [0x2b, "#GETTING_DATA"],
]);

function elapsedMilliseconds(start) {
  return Math.max(0, performance.now() - start);
}

function sheetJs() {
  if (sheetJsModule === null) {
    sheetJsModule = require("xlsx");
  }
  return sheetJsModule;
}

function displayPath(inputPath) {
  const absolutePath = path.resolve(inputPath);
  const relativePath = path.relative(REPO_ROOT, absolutePath);

  if (
    relativePath.length > 0 &&
    relativePath !== ".." &&
    !relativePath.startsWith(`..${path.sep}`) &&
    !path.isAbsolute(relativePath)
  ) {
    return relativePath.split(path.sep).join("/");
  }

  return absolutePath;
}

function errorCode(error) {
  if (error && typeof error === "object") {
    switch (error.code) {
      case "ENOENT":
        return "not_found";
      case "EACCES":
      case "EPERM":
        return "permission_denied";
      case "EISDIR":
        return "invalid_file";
      default:
        break;
    }
  }
  return "unreadable";
}

function errorMessage(error) {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  return String(error || "SheetJS could not read the file");
}

function failureDump(inputPath, error, wallMs, sha256 = null, code = null) {
  return {
    schema: 1,
    tool: "sheetjs",
    toolVersion: SHEETJS_VERSION,
    file: displayPath(inputPath),
    sha256,
    ok: false,
    error: {
      code: code || errorCode(error),
      msg: errorMessage(error),
    },
    wallMs,
    peakRssBytes: null,
    truncated: false,
    sheets: [],
    warnings: [],
  };
}

function formatDate(value) {
  if (!(value instanceof Date) || !Number.isFinite(value.getTime())) {
    return null;
  }

  const iso = value.toISOString();
  if (iso.endsWith("T00:00:00.000Z")) {
    return iso.slice(0, 10);
  }
  return iso.slice(0, -1).replace(/\.000$/, "");
}

function formulaText(cell) {
  if (typeof cell.f !== "string" || cell.f.length === 0) {
    return null;
  }
  const formula = cell.f.startsWith("=") ? cell.f.slice(1) : cell.f;
  return formula.length === 0 ? null : formula;
}

function displayText(cell) {
  return Object.hasOwn(cell, "w") && cell.w !== undefined && cell.w !== null
    ? String(cell.w)
    : null;
}

function formatCode(cell) {
  if (typeof cell.z !== "string") {
    return null;
  }
  const format = cell.z.trim();
  return format.length === 0 || format.toLowerCase() === "general"
    ? null
    : format;
}

function errorValue(cell) {
  if (typeof cell.w === "string" && cell.w.length > 0) {
    return cell.w;
  }
  if (typeof cell.v === "number" && ERROR_TEXT.has(cell.v)) {
    return ERROR_TEXT.get(cell.v);
  }
  if (cell.v === undefined || cell.v === null) {
    return null;
  }
  return String(cell.v);
}

function normalizeCell(cell, row, column) {
  const formula = formulaText(cell);
  let type;
  let value;

  switch (cell.t) {
    case "n":
      type = "n";
      value =
        typeof cell.v === "number" && Number.isFinite(cell.v) ? cell.v : null;
      break;
    case "b":
      type = "b";
      value = typeof cell.v === "boolean" ? cell.v : null;
      break;
    case "e":
      type = "e";
      value = errorValue(cell);
      break;
    case "d": {
      const date = formatDate(cell.v);
      if (date === null) {
        type = "s";
        value =
          cell.v === undefined || cell.v === null ? null : String(cell.v);
      } else {
        type = "d";
        value = date;
      }
      break;
    }
    case "s":
    case "str":
      type = "s";
      value =
        cell.v === undefined || cell.v === null ? null : String(cell.v);
      break;
    default:
      type = "s";
      value =
        cell.v === undefined || cell.v === null ? null : String(cell.v);
      break;
  }

  return {
    r: row,
    c: column,
    t: type,
    v: value,
    d: displayText(cell),
    f: formula,
    fmt: formatCode(cell),
  };
}

function isNonEmptyCell(cell) {
  if (!cell || typeof cell !== "object") {
    return false;
  }
  if (formulaText(cell) !== null) {
    return true;
  }
  return cell.v !== undefined && cell.v !== null && cell.v !== "";
}

function sheetCells(sheet) {
  const cells = [];

  for (const address of Object.keys(sheet)) {
    if (address.startsWith("!") || !/^[A-Z]+[1-9]\d*$/.test(address)) {
      continue;
    }

    let coordinate;
    try {
      coordinate = sheetJs().utils.decode_cell(address);
    } catch {
      continue;
    }

    const cell = sheet[address];
    if (!isNonEmptyCell(cell)) {
      continue;
    }

    cells.push({
      row: coordinate.r,
      column: coordinate.c,
      cell,
    });
  }

  cells.sort((left, right) => {
    return left.row - right.row || left.column - right.column;
  });
  return cells;
}

function sheetExtent(sheet) {
  if (typeof sheet["!ref"] !== "string" || sheet["!ref"].length === 0) {
    return { rows: 0, cols: 0 };
  }

  try {
    const range = sheetJs().utils.decode_range(sheet["!ref"]);
    return {
      rows: range.e.r + 1,
      cols: range.e.c + 1,
    };
  } catch {
    return { rows: 0, cols: 0 };
  }
}

function sheetMerges(sheet) {
  if (!Array.isArray(sheet["!merges"])) {
    return [];
  }
  return sheet["!merges"]
    .map((merge) => sheetJs().utils.encode_range(merge))
    .sort();
}

function normalizeWorkbook(workbook, maxCells) {
  const sheets = [];
  let emittedCells = 0;
  let documentTruncated = false;

  for (const [index, name] of workbook.SheetNames.entries()) {
    const worksheet = workbook.Sheets[name] || {};
    const candidates = sheetCells(worksheet);
    const remaining = Math.max(0, maxCells - emittedCells);
    const selected = candidates.slice(0, remaining);
    const truncated = selected.length < candidates.length;
    const extent = sheetExtent(worksheet);

    if (truncated) {
      documentTruncated = true;
    }
    emittedCells += selected.length;

    sheets.push({
      name,
      index,
      rows: extent.rows,
      cols: extent.cols,
      truncated,
      merges: sheetMerges(worksheet),
      cells: selected.map(({ row, column, cell }) =>
        normalizeCell(cell, row, column),
      ),
    });
  }

  return { sheets, truncated: documentTruncated };
}

function createNormalizedDump(inputPath, maxCells) {
  const XLSX = sheetJs();
  const start = performance.now();
  let bytes;

  try {
    bytes = fs.readFileSync(inputPath);
  } catch (error) {
    return failureDump(inputPath, error, elapsedMilliseconds(start));
  }

  const sha256 = crypto.createHash("sha256").update(bytes).digest("hex");
  let workbook;

  try {
    workbook = XLSX.read(bytes, {
      type: "buffer",
      cellDates: true,
      cellFormula: true,
      cellNF: true,
      cellText: true,
      dense: false,
    });
    const normalized = normalizeWorkbook(workbook, maxCells);

    return {
      schema: 1,
      tool: "sheetjs",
      toolVersion: SHEETJS_VERSION,
      file: displayPath(inputPath),
      sha256,
      ok: true,
      error: null,
      wallMs: elapsedMilliseconds(start),
      peakRssBytes: null,
      truncated: normalized.truncated,
      sheets: normalized.sheets,
      warnings: [],
    };
  } catch (error) {
    return failureDump(
      inputPath,
      error,
      elapsedMilliseconds(start),
      sha256,
    );
  }
}

module.exports = {
  createNormalizedDump,
  displayPath,
  failureDump,
  formatDate,
  normalizeCell,
  normalizeWorkbook,
};
