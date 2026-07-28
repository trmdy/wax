"use strict";

const TOP_LEVEL_FIELDS = [
  "schema",
  "tool",
  "toolVersion",
  "file",
  "sha256",
  "ok",
  "error",
  "wallMs",
  "peakRssBytes",
  "truncated",
  "sheets",
  "warnings",
];

const SHEET_FIELDS = [
  "name",
  "index",
  "rows",
  "cols",
  "truncated",
  "merges",
  "cells",
];

const CELL_FIELDS = ["r", "c", "t", "v", "d", "f", "fmt"];
const CELL_TYPES = new Set(["n", "s", "b", "e", "d"]);

function isObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isNonNegativeInteger(value) {
  return Number.isSafeInteger(value) && value >= 0;
}

function isFiniteNonNegative(value) {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

function requireFields(value, fields, location, errors) {
  for (const field of fields) {
    if (!Object.hasOwn(value, field)) {
      errors.push(`${location}.${field} is missing`);
    }
  }
}

function validateCell(cell, location, errors) {
  if (!isObject(cell)) {
    errors.push(`${location} must be an object`);
    return;
  }

  requireFields(cell, CELL_FIELDS, location, errors);

  if (!isNonNegativeInteger(cell.r)) {
    errors.push(`${location}.r must be a non-negative integer`);
  }
  if (!isNonNegativeInteger(cell.c)) {
    errors.push(`${location}.c must be a non-negative integer`);
  }
  if (!CELL_TYPES.has(cell.t)) {
    errors.push(`${location}.t must be one of n, s, b, e, d`);
  }
  if (cell.d !== null && typeof cell.d !== "string") {
    errors.push(`${location}.d must be a string or null`);
  }
  if (cell.f !== null && typeof cell.f !== "string") {
    errors.push(`${location}.f must be a string or null`);
  }
  if (cell.f === "") {
    errors.push(`${location}.f must be null instead of an empty string`);
  } else if (typeof cell.f === "string" && cell.f.startsWith("=")) {
    errors.push(`${location}.f must not include a leading equals sign`);
  }
  if (cell.fmt !== null && typeof cell.fmt !== "string") {
    errors.push(`${location}.fmt must be a string or null`);
  }
  if (cell.fmt === "") {
    errors.push(`${location}.fmt must be null instead of an empty string`);
  } else if (
    typeof cell.fmt === "string" &&
    cell.fmt.toLowerCase() === "general"
  ) {
    errors.push(`${location}.fmt must be null for the General format`);
  }

  if (cell.v === null) {
    return;
  }

  const valueMatchesType =
    (cell.t === "n" &&
      typeof cell.v === "number" &&
      Number.isFinite(cell.v)) ||
    (cell.t === "s" && typeof cell.v === "string") ||
    (cell.t === "b" && typeof cell.v === "boolean") ||
    (cell.t === "e" && typeof cell.v === "string") ||
    (cell.t === "d" &&
      typeof cell.v === "string" &&
      /^\d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2}:\d{2}(?:\.\d{3})?)?$/.test(
        cell.v,
      ));

  if (!valueMatchesType) {
    errors.push(`${location}.v does not match cell type ${String(cell.t)}`);
  }
}

function validateSheet(sheet, expectedIndex, location, errors) {
  if (!isObject(sheet)) {
    errors.push(`${location} must be an object`);
    return;
  }

  requireFields(sheet, SHEET_FIELDS, location, errors);

  if (typeof sheet.name !== "string") {
    errors.push(`${location}.name must be a string`);
  }
  if (!isNonNegativeInteger(sheet.index)) {
    errors.push(`${location}.index must be a non-negative integer`);
  } else if (sheet.index !== expectedIndex) {
    errors.push(`${location}.index must match its array position`);
  }
  if (!isNonNegativeInteger(sheet.rows)) {
    errors.push(`${location}.rows must be a non-negative integer`);
  }
  if (!isNonNegativeInteger(sheet.cols)) {
    errors.push(`${location}.cols must be a non-negative integer`);
  }
  if (typeof sheet.truncated !== "boolean") {
    errors.push(`${location}.truncated must be a boolean`);
  }

  if (!Array.isArray(sheet.merges)) {
    errors.push(`${location}.merges must be an array`);
  } else {
    for (const [mergeIndex, merge] of sheet.merges.entries()) {
      if (typeof merge !== "string" || merge.length === 0) {
        errors.push(`${location}.merges[${mergeIndex}] must be a string`);
      }
      if (mergeIndex > 0 && sheet.merges[mergeIndex - 1] > merge) {
        errors.push(`${location}.merges must be ascending`);
      }
    }
  }

  if (!Array.isArray(sheet.cells)) {
    errors.push(`${location}.cells must be an array`);
  } else {
    let previous = null;
    for (const [cellIndex, cell] of sheet.cells.entries()) {
      const cellLocation = `${location}.cells[${cellIndex}]`;
      validateCell(cell, cellLocation, errors);
      if (
        previous !== null &&
        isObject(cell) &&
        isNonNegativeInteger(cell.r) &&
        isNonNegativeInteger(cell.c) &&
        (cell.r < previous.r ||
          (cell.r === previous.r && cell.c <= previous.c))
      ) {
        errors.push(`${location}.cells must be unique and row-major ascending`);
      }
      if (
        isObject(cell) &&
        isNonNegativeInteger(cell.r) &&
        isNonNegativeInteger(cell.c)
      ) {
        previous = cell;
      }
    }
  }
}

function validateNormalizedDump(value) {
  const errors = [];

  if (!isObject(value)) {
    return { valid: false, errors: ["document must be an object"] };
  }

  requireFields(value, TOP_LEVEL_FIELDS, "document", errors);

  if (value.schema !== 1) {
    errors.push("document.schema must equal 1");
  }
  if (value.tool !== "wax" && value.tool !== "sheetjs") {
    errors.push('document.tool must be "wax" or "sheetjs"');
  }
  if (typeof value.toolVersion !== "string" || value.toolVersion.length === 0) {
    errors.push("document.toolVersion must be a non-empty string");
  }
  if (typeof value.file !== "string" || value.file.length === 0) {
    errors.push("document.file must be a non-empty string");
  }
  if (
    value.sha256 !== null &&
    (typeof value.sha256 !== "string" ||
      !/^[a-f0-9]{64}$/.test(value.sha256))
  ) {
    errors.push("document.sha256 must be a lowercase SHA-256 or null");
  }
  if (typeof value.ok !== "boolean") {
    errors.push("document.ok must be a boolean");
  }
  if (!isFiniteNonNegative(value.wallMs)) {
    errors.push("document.wallMs must be a finite non-negative number");
  }
  if (
    value.peakRssBytes !== null &&
    !isNonNegativeInteger(value.peakRssBytes)
  ) {
    errors.push("document.peakRssBytes must be a non-negative integer or null");
  }
  if (typeof value.truncated !== "boolean") {
    errors.push("document.truncated must be a boolean");
  }
  if (!Array.isArray(value.warnings)) {
    errors.push("document.warnings must be an array");
  } else if (value.warnings.some((warning) => typeof warning !== "string")) {
    errors.push("document.warnings must contain only strings");
  }

  if (value.ok === true && value.error !== null) {
    errors.push("document.error must be null when ok is true");
  }
  if (value.ok === true && value.sha256 === null) {
    errors.push("document.sha256 must be present when ok is true");
  }
  if (value.ok === false) {
    if (!isObject(value.error)) {
      errors.push("document.error must be an object when ok is false");
    } else if (
      typeof value.error.code !== "string" ||
      value.error.code.length === 0 ||
      typeof value.error.msg !== "string" ||
      value.error.msg.length === 0
    ) {
      errors.push("document.error must contain non-empty code and msg strings");
    }
  }

  if (!Array.isArray(value.sheets)) {
    errors.push("document.sheets must be an array");
  } else {
    for (const [sheetIndex, sheet] of value.sheets.entries()) {
      validateSheet(sheet, sheetIndex, `document.sheets[${sheetIndex}]`, errors);
    }
  }

  return { valid: errors.length === 0, errors };
}

function assertNormalizedDump(value) {
  const result = validateNormalizedDump(value);
  if (!result.valid) {
    throw new Error(`invalid normalized dump:\n${result.errors.join("\n")}`);
  }
  return value;
}

module.exports = {
  CELL_FIELDS,
  SHEET_FIELDS,
  TOP_LEVEL_FIELDS,
  assertNormalizedDump,
  validateNormalizedDump,
};
