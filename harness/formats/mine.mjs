#!/usr/bin/env node

import { spawn } from "node:child_process";
import { constants as fsConstants } from "node:fs";
import { access, mkdir, readFile, writeFile } from "node:fs/promises";
import { cpus } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "../..");
const DEFAULT_MANIFEST = path.join(REPO_ROOT, "corpus/manifest.jsonl");
const DEFAULT_OUTPUT = path.join(SCRIPT_DIR, "corpus-formats.json");
const DEFAULT_ORACLE = path.join(REPO_ROOT, "harness/oracle/run.js");
const DEFAULT_JOBS = Math.min(8, Math.max(1, cpus().length));
const DEFAULT_MAX_CELLS = 200_000;
const USAGE = `Usage: node harness/formats/mine.mjs [options]

Options:
  --manifest PATH       Manifest JSONL (default: corpus/manifest.jsonl)
  --corpus-root PATH    Root used to resolve relative manifest paths
                        (default: repository root)
  --oracle PATH         SheetJS oracle runner (default: harness/oracle/run.js)
  --output PATH         Generated JSON path
                        (default: harness/formats/corpus-formats.json)
  --jobs N              Concurrent oracle processes (default: ${DEFAULT_JOBS})
  --max-cells N         Per-file oracle cell cap (default: ${DEFAULT_MAX_CELLS})
  --timeout-ms N        Per-file oracle timeout (no default)
  --help                Show this help`;

function parseNonNegativeInteger(raw, option, { allowZero = true } = {}) {
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
  const options = {
    manifest: DEFAULT_MANIFEST,
    corpusRoot: REPO_ROOT,
    oracle: DEFAULT_ORACLE,
    output: DEFAULT_OUTPUT,
    jobs: DEFAULT_JOBS,
    maxCells: DEFAULT_MAX_CELLS,
    timeoutMs: null,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--help") {
      return { help: true };
    }

    const raw = argv[index + 1];
    if (raw === undefined) {
      throw new Error(`${argument} requires a value`);
    }
    index += 1;

    switch (argument) {
      case "--manifest":
        options.manifest = path.resolve(raw);
        break;
      case "--corpus-root":
        options.corpusRoot = path.resolve(raw);
        break;
      case "--oracle":
        options.oracle = path.resolve(raw);
        break;
      case "--output":
        options.output = path.resolve(raw);
        break;
      case "--jobs":
        options.jobs = parseNonNegativeInteger(raw, argument, {
          allowZero: false,
        });
        break;
      case "--max-cells":
        options.maxCells = parseNonNegativeInteger(raw, argument);
        break;
      case "--timeout-ms":
        options.timeoutMs = parseNonNegativeInteger(raw, argument, {
          allowZero: false,
        });
        break;
      default:
        throw new Error(`unknown option: ${argument}`);
    }
  }

  return options;
}

function parseManifest(contents) {
  const entries = [];
  for (const [index, line] of contents.split(/\r?\n/u).entries()) {
    if (line.trim().length === 0) {
      continue;
    }

    let entry;
    try {
      entry = JSON.parse(line);
    } catch (error) {
      throw new Error(
        `manifest line ${index + 1} is invalid JSON: ${error.message}`,
      );
    }
    if (
      entry === null ||
      typeof entry !== "object" ||
      typeof entry.path !== "string" ||
      entry.path.length === 0
    ) {
      throw new Error(`manifest line ${index + 1} has no usable path`);
    }
    entries.push({
      path: entry.path,
      private: entry.private === true,
    });
  }
  return entries;
}

function resolveEntryPath(entry, corpusRoot) {
  return path.isAbsolute(entry.path)
    ? entry.path
    : path.resolve(corpusRoot, entry.path);
}

async function exists(filePath) {
  try {
    await access(filePath, fsConstants.R_OK);
    return true;
  } catch {
    return false;
  }
}

function runOracle(oracle, filePath, maxCells, timeoutMs) {
  const arguments_ = [oracle, filePath, "--max-cells", String(maxCells)];
  if (timeoutMs !== null) {
    arguments_.push("--timeout-ms", String(timeoutMs));
  }

  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, arguments_, {
      cwd: REPO_ROOT,
      env: process.env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    const stdout = [];
    const stderr = [];

    child.stdout.on("data", (chunk) => stdout.push(chunk));
    child.stderr.on("data", (chunk) => stderr.push(chunk));
    child.once("error", reject);
    child.once("close", (status, signal) => {
      const output = Buffer.concat(stdout).toString("utf8");
      const diagnostic = Buffer.concat(stderr).toString("utf8").trim();
      if (status !== 0) {
        const conciseDiagnostic = diagnostic.split(/\r?\n/u)[0];
        reject(
          new Error(
            `oracle exited ${status ?? `on signal ${signal}`}${
              conciseDiagnostic.length > 0 ? `: ${conciseDiagnostic}` : ""
            }`,
          ),
        );
        return;
      }
      try {
        resolve(JSON.parse(output));
      } catch (error) {
        reject(new Error(`oracle returned invalid JSON: ${error.message}`));
      }
    });
  });
}

function countDump(dump, counts) {
  const formatsInFile = new Set();
  for (const sheet of dump.sheets) {
    for (const cell of sheet.cells) {
      counts.cellsSeen += 1;
      if (typeof cell.fmt !== "string" || cell.fmt.length === 0) {
        continue;
      }
      counts.formattedCells += 1;
      const current = counts.formats.get(cell.fmt) ?? {
        code: cell.fmt,
        cellCount: 0,
        fileCount: 0,
      };
      current.cellCount += 1;
      counts.formats.set(cell.fmt, current);
      formatsInFile.add(cell.fmt);
    }
  }
  for (const code of formatsInFile) {
    counts.formats.get(code).fileCount += 1;
  }
}

async function runPool(items, jobs, worker) {
  let nextIndex = 0;
  async function takeNext() {
    while (nextIndex < items.length) {
      const index = nextIndex;
      nextIndex += 1;
      await worker(items[index], index);
    }
  }
  await Promise.all(
    Array.from({ length: Math.min(jobs, items.length) }, () => takeNext()),
  );
}

async function mine(options) {
  const manifest = parseManifest(await readFile(options.manifest, "utf8"));
  const available = [];
  let skippedMissing = 0;

  for (const entry of manifest) {
    const filePath = resolveEntryPath(entry, options.corpusRoot);
    if (await exists(filePath)) {
      available.push({ ...entry, filePath });
    } else {
      skippedMissing += 1;
    }
  }

  const counts = {
    cellsSeen: 0,
    formattedCells: 0,
    filesOpened: 0,
    filesFailed: 0,
    oracleAborts: 0,
    truncatedFiles: 0,
    privateFilesScanned: 0,
    formats: new Map(),
  };
  let completed = 0;

  process.stderr.write(
    `Mining ${available.length} present files with ${options.jobs} jobs` +
      ` (${skippedMissing} missing)\n`,
  );

  function reportProgress() {
    completed += 1;
    if (completed % 100 === 0 || completed === available.length) {
      process.stderr.write(`Scanned ${completed}/${available.length}\n`);
    }
  }

  await runPool(available, options.jobs, async (entry) => {
    if (entry.private) {
      counts.privateFilesScanned += 1;
    }

    let dump;
    try {
      dump = await runOracle(
        options.oracle,
        entry.filePath,
        options.maxCells,
        options.timeoutMs,
      );
    } catch (error) {
      counts.filesFailed += 1;
      counts.oracleAborts += 1;
      process.stderr.write(
        `Oracle abort (${counts.oracleAborts}): ${error.message}\n`,
      );
      reportProgress();
      return;
    }

    if (dump.ok === true && Array.isArray(dump.sheets)) {
      counts.filesOpened += 1;
      countDump(dump, counts);
      if (dump.truncated === true) {
        counts.truncatedFiles += 1;
      }
    } else {
      counts.filesFailed += 1;
    }

    reportProgress();
  });

  const formats = [...counts.formats.values()].sort(
    (left, right) =>
      right.cellCount - left.cellCount ||
      right.fileCount - left.fileCount ||
      left.code.localeCompare(right.code),
  );
  return {
    schema: 1,
    generatedAt: new Date().toISOString(),
    oracle: "SheetJS 0.20.3 normalized dumps",
    maxCellsPerFile: options.maxCells,
    totals: {
      manifestFiles: manifest.length,
      filesScanned: available.length,
      filesOpened: counts.filesOpened,
      filesFailed: counts.filesFailed,
      oracleAborts: counts.oracleAborts,
      filesMissing: skippedMissing,
      privateFilesScanned: counts.privateFilesScanned,
      truncatedFiles: counts.truncatedFiles,
      cellsSeen: counts.cellsSeen,
      formattedCells: counts.formattedCells,
      distinctCodes: formats.length,
    },
    formats,
  };
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

  if (options.help) {
    process.stdout.write(`${USAGE}\n`);
    return;
  }

  try {
    const result = await mine(options);
    await mkdir(path.dirname(options.output), { recursive: true });
    await writeFile(options.output, `${JSON.stringify(result, null, 2)}\n`);
    process.stderr.write(
      `Wrote ${result.formats.length} formats from ` +
        `${result.totals.formattedCells} formatted cells to ${options.output}\n`,
    );
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main(process.argv.slice(2));
}

export {
  countDump,
  mine,
  parseArguments,
  parseManifest,
  resolveEntryPath,
  runPool,
};
