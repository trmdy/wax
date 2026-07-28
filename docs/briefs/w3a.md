# Shard W3A — NDJSON stdio protocol server

You are shard **W3A** of the wax v1 mission, wave 3. Coordinator: bee
**CL.d73**.

**Required reading before any code:** `MISSION.md`, `docs/w1-contracts.md`
§1–2, `docs/w3-contracts.md` — §1 is your spec, word for word; §2 is the
store seam you consume. You work in your git worktree only
(`.worktrees/w3a`), on branch `agent/wax-w3a-proto`. Never touch `main`.
No remote; commit locally, the coordinator merges.

## The job

`wax serve`: the NDJSON-over-stdio server Apiary will drive. Implement
protocol v0 exactly as frozen in `docs/w3-contracts.md` §1:

- Ops: `version`, `open`, `meta`, `window`, `export` (csv only; xlsx →
  `unsupported`), `close`, `cancel`, `stats`.
- Wire types + serde live in `crates/wax-proto` (you own it — the
  `ErrorCode` enum on `main` already has the full v0 code list).
- The serve loop lives in `crates/wax-cli`. Reader = `wax_read`
  (`CalamineReader`), storage = `wax_store::WorkbookStore` — code against
  its five frozen public calls and nothing deeper; W3B is rewriting the
  internals concurrently, same API.
- Concurrency: requests may execute in parallel (worker threads per open;
  windows on an open handle can be served from the shared store);
  responses out of order, `id`-correlated, one flushed line each, never
  interleaved mid-line.
- Cancellation cooperative (checkpoints); per-op wall-clock timeout as the
  hard backstop — abandoned worker threads are acceptable v0, document
  it. W3C is building a `read_with_deadline`-shaped wrapper in wax-read;
  buz them (via the coordinator if the seam is unclear) rather than
  duplicating a watchdog — but do not block on them: a local watchdog you
  swap later is fine.
- Lifecycle: `--idle-timeout-ms` (default 300000), `--max-handles`
  (default 16), clean EOF/SIGTERM exit.
- Open defaults in serve mode: `maxCells` 5,000,000, `maxBytes` 100 MiB,
  `timeoutMs` 30000. (`wax dump` keeps its 200,000 default — do not
  change dump's behavior; the harness depends on it.)

## Tests

- Unit tests for request parsing/validation (caps, bad ids, unknown ops,
  malformed lines → `bad_request` with `id:null`).
- Integration tests that spawn the real binary, speak v0 over pipes, and
  cover: version handshake; open→meta→window→close happy path on a small
  fixture xlsx; window clipping + out-of-extent; window cap rejection;
  export csv (content asserted, RFC 4180 quoting); bad_handle after
  close and after idle expiry (short `--idle-timeout-ms`); cancel of an
  in-flight open; out-of-order id correlation; EOF exit 0.
- A malformed-input fuzz-ish test: throw junk lines at the loop, assert it
  never panics and always answers or ignores per spec.

## Boundaries (do not touch)

`crates/wax-store/**` (consume only), `crates/wax-read/**` (consume only),
`crates/wax-core/**`, `crates/wax-fmt/**`, `harness/**`, `corpus/**`,
`scripts/**`, `.github/**`, `docs/**`, `ASSIGNMENTS.json`. You own
`crates/wax-proto/**` and `crates/wax-cli/**`. If the store API or the
frozen wire spec blocks you, buz the coordinator — do not improvise a
different protocol.

## Definition of done

- `scripts/check.sh` fully green in your worktree.
- All §1 ops implemented per spec with the integration suite above.
- Commit messages end with your bee name.

## Sealing

`hive seal <your-bee-name> --from seal.json` with status, summary,
deliverables, exact test counts, any spec ambiguities you resolved (list
them — the coordinator folds real ones back into the contract), and
deviations. Then
`hive buz send CL.d73 --sender <your-bee-name> --tier queue -p "sealed"`.
Blockers → buz immediately; never a silent stall.
