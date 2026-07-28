"use strict";

const fs = require("node:fs");
const path = require("node:path");
const XLSX = require("xlsx");

const fixturesDirectory = path.join(__dirname, "fixtures");
fs.mkdirSync(fixturesDirectory, { recursive: true });

const sheet = XLSX.utils.aoa_to_sheet([
  [1, 2],
  ["merged"],
  [new Date(2024, 1, 29)],
]);

sheet.C1 = { t: "n", v: 3, f: "A1+B1" };
sheet.A3.z = "yyyy-mm-dd";
sheet["!ref"] = "A1:C3";
sheet["!merges"] = [XLSX.utils.decode_range("A2:B2")];

const workbook = XLSX.utils.book_new();
XLSX.utils.book_append_sheet(workbook, sheet, "Oracle");
workbook.Workbook = { WBProps: { date1904: false } };

XLSX.writeFile(workbook, path.join(fixturesDirectory, "oracle.xlsx"), {
  bookType: "xlsx",
  compression: true,
});

const date1904Sheet = {
  A1: { t: "n", v: 0, z: "yyyy-mm-dd" },
  "!ref": "A1",
};
const date1904Workbook = XLSX.utils.book_new();
XLSX.utils.book_append_sheet(date1904Workbook, date1904Sheet, "Epoch 1904");
date1904Workbook.Workbook = { WBProps: { date1904: true } };
XLSX.writeFile(
  date1904Workbook,
  path.join(fixturesDirectory, "date-1904.xlsx"),
  {
    bookType: "xlsx",
    compression: true,
  },
);

fs.writeFileSync(
  path.join(fixturesDirectory, "garbage.xlsx"),
  Buffer.from("PK\u0003\u0004this is not a valid workbook", "binary"),
);
