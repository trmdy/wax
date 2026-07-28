# wax differential harness

This standalone crate compares normalized wax and SheetJS dumps and writes:

- `harness/results.jsonl` — additive per-file comparison records, including
  per-request `wax serve` timings and failures when serve is available.
- `harness/scoreboard.json` and the repository-root `SCOREBOARD.md` — aggregate
  compatibility, including dump/serve open rates, window latency, serve RSS,
  display-string match, and per-extension formula/cached-result fidelity.
- `harness/format-coverage.json` — per-format display coverage and exact-match
  metrics. When present, `harness/formats/corpus-formats.json` supplies the
  corpus-wide ranking without becoming a required dependency.
- `harness/triage.md` — top open, value, and display disagreement buckets with
  privacy-filtered examples.

The public entry point is run from the repository root:

```sh
harness/run.sh [--manifest corpus/manifest.jsonl] [--limit N] [--jobs N] \
  [--no-serve]
```

The serve pass is on by default. It starts a fresh `wax serve` for every corpus
file and degrades to `n/a (serve unavailable)` when the selected binary does
not expose that subcommand. `--no-serve` explicitly disables the pass.

`WAX_BIN` overrides the default `target/release/wax`. The test-only
`WAX_ORACLE_SCRIPT`, `WAX_REPO_ROOT`, and `WAX_HARNESS_BIN` overrides let the
integration suite exercise the complete shell entry point without depending
on the parallel W1A/W1C shards. Run that three-file fake-binary check with:

```sh
cargo test --manifest-path harness/wax-harness/Cargo.toml \
  --test runner run_sh_is_an_end_to_end_entry_point_for_the_fake_contract_tools
```

The protocol fixture covers out-of-order responses, a server error, process
death, and client-side timeout/kill behavior:

```sh
cargo test --manifest-path harness/wax-harness/Cargo.toml --test serve
```
