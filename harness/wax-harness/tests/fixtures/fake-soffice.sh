#!/bin/sh
set -eu

output_dir=
input=
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--outdir" ]; then
    output_dir=$2
    shift 2
    continue
  fi
  input=$1
  shift
done

name=${input##*/}
cp "$input" "$output_dir/$name"
