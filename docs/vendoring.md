# Vendoring wax into Apiary

This is the release handoff for the Apiary Sheet pane integrator (APIA-162).
wax remains a subprocess dependency: Apiary does not add Rust, a native Node
binding, or spreadsheet parsing to Electron main. That boundary is defined in
the [wax mission](../MISSION.md#what-wax-is).

## Select the artifact

Release tag `v<version>` has these assets:

| Apiary build host | Rust target | Platform slug | Release asset |
| --- | --- | --- | --- |
| macOS arm64 | `aarch64-apple-darwin` | `macos-arm64` | `wax-v<version>-macos-arm64.tar.gz` |
| macOS x64 | `x86_64-apple-darwin` | `macos-x64` | `wax-v<version>-macos-x64.tar.gz` |
| Linux x64 (glibc) | `x86_64-unknown-linux-gnu` | `linux-x64` | `wax-v<version>-linux-x64.tar.gz` |

Each tarball contains exactly an executable `wax` (mode 0755) and
`README.md`. Windows is not a v1 target.

`SHA256SUMS.txt` covers all three archives, one standard checksum record per
line:

```text
<64 lowercase hexadecimal characters>  wax-v<version>-<platform>.tar.gz
```

Verify the selected archive before extracting or committing it:

```bash
set -euo pipefail
archive="wax-v0.1.0-macos-arm64.tar.gz"
grep -F "  ${archive}" SHA256SUMS.txt | shasum -a 256 -c -
```

Keep the downloaded checksum file as release evidence, but pin the exact
64-character SHA-256 value in Apiary's dependency metadata. A version without
its checksum is not a complete pin.

## Version and protocol pairing

The executable reports one stable machine-checkable line:

```text
wax 0.1.0 (proto 0)
```

The semver is the release/Cargo workspace version. The protocol number is the
NDJSON wire compatibility version and is also returned by every successful
`open`. Protocol stays unchanged across compatible releases; it bumps when a
consumer must change its request/response handling. Apiary should record the
expected semver, protocol number, platform slug, and archive SHA-256 together,
then reject a binary whose `--version` or `open.proto` differs.

Updating wax therefore means one reviewed change that replaces the vendored
binary and updates all four values. Never select "latest" during an Apiary
build.

## Apiary binary resolution

The development resolution order is:

1. `APIARY_WAX_BIN`, when set, for an explicit local or test binary.
2. `wax` on `PATH`.

For packaged builds, stage the verified host binary as an Electron Builder
`extraResources` entry named `wax`, alongside Apiary's other helper
executables. Resolve it at runtime as `join(process.resourcesPath, "wax")`;
do not rely on the packaged application's `PATH`. This implements the
PATH-in-development / vendored-`extraResources` convention specified by
the [mission repository contract](../MISSION.md#mission--wax-v1-the-out-of-process-sheet-engine).
The packaging job must select the release asset from the table above before
Electron Builder runs; a universal macOS app still needs an explicit
architecture policy rather than silently shipping one of the two binaries.

## Subprocess contract

Spawn `wax serve` and exchange one `id`-correlated JSON object per line over
stdin/stdout; diagnostics belong on stderr, and responses may arrive out of
order. Close open handles when possible and send SIGTERM during application
shutdown; wax exits cleanly without writing a partial response line.
