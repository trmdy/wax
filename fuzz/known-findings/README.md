# Known open fuzz findings

Inputs here reproduce a defect that wax does **not** yet fully contain.
They are deliberately kept out of `fuzz/corpus/` and `fuzz/artifacts/` so
that `scripts/check.sh`'s deterministic replay stays a regression gate for
*fixed* findings. Every entry must name the defect, the current behavior,
and the intended fix. Clearing this directory is W5 (hardening) scope.

## legacy_xls_reader / calamine unbounded FAT-driven growth

- **Input:** `legacy_xls_reader/calamine-unbounded-fat-growth.xls`
  (5,640 bytes; recovered from a 5-minute burn, 2026-07-28).
- **Defect:** inside `calamine 0.36.1`'s own CFB reader (`src/cfb.rs`, not
  the `cfb` crate wax preflights with), a corrupt sector chain drives an
  unbounded `Vec` growth — libFuzzer reports `malloc(137438953472)`
  (128 GiB). Related smells in the same file: the DIFAT walk carries an
  upstream `//TODO: check if in infinite loop`, and `Header::from_reader`
  reads the DIFAT length from offset 62 instead of 72.
- **Current wax behavior:** `wax dump` **does not crash or hang forever** —
  `read_with_deadline` returns a structured `{"ok":false,"code":"timeout"}`
  document at the 30 s wall clock. But the abandoned worker reached
  **87 GiB peak RSS** first, which is a host-OOM risk for Apiary. wax's
  preflight rails (input size, header sector counts, DIFAT chain walk,
  stream sizes, BIFF record structure, declared extent) all pass this file
  legitimately; the growth happens in chain-following code wax does not
  mirror.
- **Intended fix (W5):** either (a) mirror calamine's FAT/DIFAT chain walk
  in preflight with a cycle + length bound, (b) upstream a bounded-growth
  patch to calamine, or (c) run the parse in a child process with an
  address-space rlimit so any dependency blow-up is contained by the OS.
  Option (c) also caps every future unknown of this class and fits the
  "short-lived subprocess" posture the mission already describes.
