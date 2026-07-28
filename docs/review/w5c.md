# W5C review — release CI + README + vendoring doc

- **Shard:** w5c (`agent/wax-w5c-release`, bee CO.7011 / wax-w5c)
- **Commit reviewed:** `2c11f72`
- **Reviewer:** coordinator CL.7c63, 2026-07-28
- **Verdict:** merge. No required fixes.

## What was reviewed

Full diff (6 files, +558/−32): `release.yml` (prepare → build/package matrix
→ bundle → tag-gated release), new `scripts/package.sh` +
`scripts/test-package.sh`, `check-ci-defs.sh` extension, README rewrite,
`docs/vendoring.md`. Every protocol/harness claim in the README was
verified against source: serve defaults (5M cells / 100 MiB / 30 s
timeout), window cap 262,144, 16 handles, 5-minute idle expiry, the nine
v0 error codes, `cancel.target`, and the harness `--limit`/`--jobs` flags.

## Real-matrix verification (coordinator)

Branch pushed and `workflow_dispatch` run on
`agent/wax-w5c-release` — run 30395599877, fully green:

- All three runners built and packaged (`macos-15` arm64,
  `macos-15-intel` x64, `ubuntu-24.04` linux-x64); the `macos-15-intel`
  label resolves.
- Bundle job produced `release-assets-v0.1.0` with exactly the three
  tarballs + `SHA256SUMS.txt`; the publish job was correctly **skipped**
  on dispatch (tag-push gated).
- Downloaded the bundle on this machine: `shasum -a 256 -c SHA256SUMS.txt`
  → 3× OK; tarball contains exactly `wax` (0755) + `README.md`; the
  CI-built macOS arm64 binary runs here and prints `wax 0.1.0 (proto 0)`.

## Findings

1. Version guard chain is complete: tag ↔ workspace `Cargo.toml` ↔
   `wax --version` output all cross-checked before packaging; malformed
   tags fail the prepare job.
2. Release publish is idempotent (`gh release view` → upload `--clobber`
   vs `create --verify-tag`), and `contents: write` is scoped to the
   publish job only.
3. Packaging is locally testable (`scripts/package.sh`) with a real
   regression test (`test-package.sh`: entries, modes, overwrite refusal,
   platform validation) wired into `check-ci-defs.sh`.
4. Cosmetic only: GitHub annotates actions v4 (checkout/upload/download)
   with Node 20 deprecation warnings — non-blocking, fine for v0.1.0.

## Accepted deviations

- SHA-256 checksums only; no code signing / notarization (operator-ratified
  W5 scope vs the older MISSION.md "signed tarballs" wording).
