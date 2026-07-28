# Review — W3A: NDJSON stdio protocol server (`agent/wax-w3a-proto`, 9b4abb0)

Reviewer: CL.d73 (coordinator). Verdict: **merge**.

## Scope check

Diff against merge base touches only `crates/wax-proto/**`,
`crates/wax-cli/**`, `Cargo.lock` — exactly the assigned lane. 1,857
insertions. `scripts/check.sh` re-verified green by the coordinator in the
shard worktree (not just trusted from the seal).

## What's there

- `wax-proto`: typed `Request` parser (serde_json `Value` → enum, per-field
  errors carrying the request id when recoverable, `id:null` when not),
  full `Response` family with `#[serde(untagged)]`, `WireCell` omitting
  null `d/f/fmt` but keeping `v` — matches the frozen §1 wire spec,
  including serve-specific open defaults (5M cells / 100 MiB / 30 s) that
  deliberately do not leak into `wax dump`'s 200k default.
- `serve.rs`: single-writer main loop (BufWriter + flush per line, no
  interleaving possible), reader thread for stdin, worker threads for
  open/window/export, `id`-correlated out-of-order completion.
- Lifecycle: `--idle-timeout-ms` / `--max-handles` with pending-open
  reservation against the cap; expired handles are remembered so the error
  distinguishes `"expired"` from `"unknown"` (nice touch beyond spec);
  SIGTERM via async-signal-safe atomic; EOF → cancel-all → exit 0.
- Cancellation/timeout: cooperative checkpoints + main-loop deadline sweep.
  The complete/cancel/expire triangle is race-clean: the in-flight entry is
  removed exactly once, whichever of {worker result, synthetic cancel
  result, deadline expiry} wins, and late worker results for a forgotten id
  are silently dropped. Abandoned workers documented as accepted v0
  behavior (contract §1).
- CSV export: RFC 4180, CRLF, display-string-first, shortest-round-trip
  numbers, TRUE/FALSE, windowed in 64-row blocks through the frozen store
  API (so W3B's O(window) rewrite directly speeds it up), honest `dropped`
  list.
- W3C seam honored: `read_document_with_deadline` is an explicitly marked
  one-line swap point, with a comment noting `maxBytes → ReaderOptions.max_bytes`
  mapping to wire up at the W3C merge (input-size cap is enforced locally
  via metadata meanwhile).

## Tests

13 proto unit tests, 3 serve unit tests, 9 real-binary integration tests
(spawned `wax` over pipes): handshake+EOF, happy path, clipping/merges/cap
rejection, CSV + xlsx-unsupported, max-handles + idle expiry, malformed
lines (never panic), cancel of a genuinely blocked open (FIFO trick),
wall-clock timeout with server survival, SIGTERM. The FIFO-as-input
approach to force a blocked reader is a genuinely good way to make the
cancel/timeout paths testable deterministically.

## Findings (non-blocking, tracked)

1. **Idle poll cadence**: main loop wakes every 5 ms (`EVENT_POLL`) even
   when idle. Harmless for Apiary's short-lived per-file servers; a
   long-lived idle server burns a little CPU. Fine for v0; worth an
   event-driven rework only if serve becomes long-lived. → note for W5.
2. **Unbounded stdin line length**: `read_line` will buffer an arbitrarily
   long line; a hostile client could OOM the server. The client is Apiary
   (trusted) and the file side is where hostility lives, but a line-length
   cap is cheap. → W5 hardening list.
3. **`timeoutMs` near `u64::MAX` overflows `checked_add` and degrades to
   an immediate deadline** (instant timeout). Pathological input only,
   fails safe (times out rather than never timing out). Not worth a
   respin.
4. Redundant double sheet-bounds validation (dispatch + worker) — benign.

## Deviations from seal

None found; seal claims verified (test counts, check.sh, contract
adherence).
