# wax

Out-of-process sheet engine: a prebuilt Rust sidecar that reads (calamine)
and writes (rust_xlsxwriter) spreadsheet files behind a windowed NDJSON
stdio protocol — normalization, Excel number-format rendering, memory-bounded
columnar store, hostile-input rails, and a corpus-driven compatibility
harness. Built for Apiary's Sheet pane; useful standalone.

No consumer ever needs a Rust toolchain: CI publishes per-platform binaries.

See MISSION.md for the build plan; SCOREBOARD.md (once W1 lands) for state.
