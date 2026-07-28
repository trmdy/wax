#!/bin/sh
set -eu

fixture_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
archive_tmp="$fixture_dir/reader.xlsx.tmp"
unstyled_archive_tmp="$fixture_dir/unstyled.xlsx.tmp"

(
  cd "$fixture_dir/reader-src"
  zip -q -X -r "$archive_tmp" .
)
mv -f "$archive_tmp" "$fixture_dir/reader.xlsx"

(
  cd "$fixture_dir/unstyled-src"
  zip -q -X -r "$unstyled_archive_tmp" .
)
mv -f "$unstyled_archive_tmp" "$fixture_dir/unstyled.xlsx"
