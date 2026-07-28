#!/bin/sh
set -eu

fixture_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
archive_tmp="$fixture_dir/reader.xlsx.tmp"

(
  cd "$fixture_dir/reader-src"
  zip -q -X -r "$archive_tmp" .
)
mv -f "$archive_tmp" "$fixture_dir/reader.xlsx"
