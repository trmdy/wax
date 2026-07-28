#!/bin/sh
set -eu

file=$3
name=${file##*/}

if [ "$name" = "crash.xlsx" ]; then
  echo "fixture crash" >&2
  exit 7
fi

if [ "$name" = "diff.xlsx" ]; then
  sha=diff-sha
  value=2
  display='"2.0"'
  formula='"SUM(A1)"'
  format='"0.00"'
else
  sha=match-sha
  value=1
  display='"1.00"'
  formula=null
  format='"0.00"'
fi

printf '%s\n' \
  "{\"schema\":1,\"tool\":\"wax\",\"toolVersion\":\"0.1.0\",\"file\":\"$file\",\"sha256\":\"$sha\",\"ok\":true,\"error\":null,\"wallMs\":10,\"peakRssBytes\":100,\"truncated\":false,\"sheets\":[{\"name\":\"Sheet1\",\"index\":0,\"rows\":1,\"cols\":1,\"truncated\":false,\"merges\":[],\"cells\":[{\"r\":0,\"c\":0,\"t\":\"n\",\"v\":$value,\"d\":$display,\"f\":$formula,\"fmt\":$format}]}],\"warnings\":[]}" # canned normalized dump
