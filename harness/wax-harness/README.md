# wax differential harness

This standalone W1 crate compares normalized wax and SheetJS dumps and writes
`harness/results.jsonl`, `harness/scoreboard.json`, and the repository-root
`SCOREBOARD.md`.

The public entry point is run from the repository root:

```sh
harness/run.sh [--manifest corpus/manifest.jsonl] [--limit N] [--jobs N]
```

`WAX_BIN` overrides the default `target/release/wax`. The test-only
`WAX_ORACLE_SCRIPT`, `WAX_REPO_ROOT`, and `WAX_HARNESS_BIN` overrides let the
integration suite exercise the complete shell entry point without depending
on the parallel W1A/W1C shards. Run that three-file fake-binary check with:

```sh
cargo test --manifest-path harness/wax-harness/Cargo.toml \
  --test runner run_sh_is_an_end_to_end_entry_point_for_the_fake_contract_tools
```
