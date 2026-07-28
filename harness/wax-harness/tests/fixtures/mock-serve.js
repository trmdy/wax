#!/usr/bin/env node

const fs = require("node:fs");
const readline = require("node:readline");

if (process.argv[2] !== "serve") {
  process.stderr.write("mock wax only supports serve\n");
  process.exit(2);
}

let mode = process.env.MOCK_SERVE_MODE || "happy";
let pendingMeta = null;
let windowCount = 0;

function send(response) {
  process.stdout.write(`${JSON.stringify(response)}\n`);
}

function success(id, fields = {}) {
  send({ id, ok: true, ...fields });
}

function windowResponse(request) {
  const nr = 64;
  const nc = 24;
  return {
    id: request.id,
    ok: true,
    sheet: request.sheet,
    r0: request.r0,
    c0: request.c0,
    nr,
    nc,
    rows: Array.from({ length: nr }, () => Array(nc).fill(null)),
    merges: [],
  };
}

const input = readline.createInterface({
  input: process.stdin,
  crlfDelay: Infinity,
});

input.on("line", (line) => {
  const request = JSON.parse(line);
  switch (request.op) {
    case "version":
      success(request.id, { proto: 0, version: "0.1.0" });
      break;
    case "open":
      if (request.path.includes("open-error.xlsx")) {
        send({
          id: request.id,
          ok: false,
          code: "bad_zip",
          msg: "fixture open rejection",
        });
        break;
      }
      if (request.path.includes("error.xlsx")) mode = "error";
      if (request.path.includes("death.xlsx")) mode = "death";
      if (request.path.includes("hang.xlsx")) mode = "hang";
      success(request.id, {
        proto: 0,
        handle: "h1",
        truncated: false,
        sheets: [{ name: "Sheet1", rows: 128, cols: 48, truncated: false }],
        warnings: [],
      });
      break;
    case "meta":
      pendingMeta = {
        id: request.id,
        ok: true,
        truncated: false,
        sheets: [{ name: "Sheet1", rows: 128, cols: 48, truncated: false }],
        warnings: [],
      };
      break;
    case "window":
      windowCount += 1;
      if (mode === "death" && windowCount === 1) {
        process.stderr.write("fixture process death\n");
        process.exit(17);
      }
      if (mode === "error" && windowCount === 2) {
        send({
          id: request.id,
          ok: false,
          code: "internal",
          msg: "fixture window failure",
        });
      } else if (!(mode === "hang" && windowCount === 1)) {
        send(windowResponse(request));
      }
      // Meta deliberately arrives after every window response, proving the
      // client correlates ids rather than assuming response order.
      if (windowCount === 5 && pendingMeta) {
        send(pendingMeta);
        pendingMeta = null;
      }
      break;
    case "export": {
      const csv = "value\r\n";
      fs.writeFileSync(request.out, csv);
      success(request.id, { bytes: Buffer.byteLength(csv), dropped: ["formulas"] });
      break;
    }
    case "stats":
      success(request.id, {
        peakRssBytes: 52_428_800,
        handles: 1,
        storeBytes: 1_234_567,
      });
      break;
    case "close":
      success(request.id);
      break;
    default:
      send({
        id: request.id ?? null,
        ok: false,
        code: "bad_request",
        msg: `unsupported fixture op ${request.op}`,
      });
  }
});

input.on("close", () => {
  process.exit(0);
});
