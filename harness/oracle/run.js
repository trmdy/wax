#!/usr/bin/env node
"use strict";

const { performance } = require("node:perf_hooks");
const {
  Worker,
  isMainThread,
  parentPort,
  workerData,
} = require("node:worker_threads");
const { assertNormalizedDump } = require("./schema");
const { createNormalizedDump, failureDump } = require("./lib/oracle");

const DEFAULT_MAX_CELLS = 200_000;
const USAGE =
  "Usage: node harness/oracle/run.js <file> [--max-cells N] [--timeout-ms N]";

function positiveOrZeroInteger(raw, option, allowZero) {
  if (!/^\d+$/.test(raw)) {
    throw new Error(`${option} must be an integer`);
  }
  const value = Number(raw);
  if (
    !Number.isSafeInteger(value) ||
    value < 0 ||
    (!allowZero && value === 0)
  ) {
    throw new Error(
      `${option} must be a ${allowZero ? "non-negative" : "positive"} integer`,
    );
  }
  return value;
}

function parseArguments(argv) {
  let file = null;
  let maxCells = DEFAULT_MAX_CELLS;
  let timeoutMs = null;
  const seen = new Set();

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];

    if (argument === "--max-cells" || argument === "--timeout-ms") {
      if (seen.has(argument)) {
        throw new Error(`${argument} may only be supplied once`);
      }
      seen.add(argument);
      const raw = argv[index + 1];
      if (raw === undefined) {
        throw new Error(`${argument} requires a value`);
      }
      index += 1;
      if (argument === "--max-cells") {
        maxCells = positiveOrZeroInteger(raw, argument, true);
      } else {
        timeoutMs = positiveOrZeroInteger(raw, argument, false);
      }
      continue;
    }

    if (argument.startsWith("-")) {
      throw new Error(`unknown option: ${argument}`);
    }
    if (file !== null) {
      throw new Error("exactly one input file is required");
    }
    file = argument;
  }

  if (file === null) {
    throw new Error("an input file is required");
  }

  return { file, maxCells, timeoutMs };
}

function peakRssBytes() {
  const maxRssKiB = process.resourceUsage().maxRSS;
  return Number.isFinite(maxRssKiB)
    ? Math.max(0, Math.round(maxRssKiB * 1024))
    : null;
}

function runWorker(options) {
  return new Promise((resolve, reject) => {
    const worker = new Worker(__filename, { workerData: options });
    let settled = false;
    let timer = null;

    function finish(callback, value) {
      if (settled) {
        return;
      }
      settled = true;
      if (timer !== null) {
        clearTimeout(timer);
      }
      callback(value);
    }

    worker.once("message", (message) => {
      if (message && message.dump) {
        finish(resolve, message.dump);
      } else {
        finish(reject, new Error("oracle worker returned an invalid response"));
      }
    });

    worker.once("error", (error) => {
      finish(reject, error);
    });

    worker.once("exit", (code) => {
      if (!settled) {
        finish(
          reject,
          new Error(`oracle worker exited before producing output (exit ${code})`),
        );
      }
    });

    if (options.timeoutMs !== null) {
      timer = setTimeout(async () => {
        if (settled) {
          return;
        }
        settled = true;
        await worker.terminate();
        resolve({
          timedOut: true,
          wallMs: Math.max(0, performance.now() - options.startedAt),
        });
      }, options.timeoutMs);
    }
  });
}

async function main(argv) {
  let options;
  try {
    options = parseArguments(argv);
  } catch (error) {
    process.stderr.write(`${error.message}\n${USAGE}\n`);
    process.exitCode = 2;
    return;
  }

  const startedAt = performance.now();

  try {
    const result = await runWorker({ ...options, startedAt });
    const dump =
      result && result.timedOut
        ? failureDump(
            options.file,
            new Error(`parsing exceeded ${options.timeoutMs} ms`),
            result.wallMs,
            null,
            "timeout",
          )
        : result;
    dump.peakRssBytes = peakRssBytes();
    assertNormalizedDump(dump);
    process.stdout.write(JSON.stringify(dump));
  } catch (error) {
    process.stderr.write(
      `oracle aborted without a result: ${
        error instanceof Error ? error.message : String(error)
      }\n`,
    );
    process.exitCode = 1;
  }
}

if (isMainThread) {
  if (require.main === module) {
    void main(process.argv.slice(2));
  }
} else {
  const dump = createNormalizedDump(workerData.file, workerData.maxCells);
  parentPort.postMessage({ dump });
}

module.exports = {
  DEFAULT_MAX_CELLS,
  main,
  parseArguments,
  peakRssBytes,
};
