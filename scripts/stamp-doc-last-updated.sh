#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <base-ref> <head-ref>" >&2
  exit 1
fi

base_ref="$1"
head_ref="$2"
today="$(date -u +%F)"

mapfile -t changed_docs < <(git diff --name-only "$base_ref" "$head_ref" -- 'docs/*.md')

if [[ ${#changed_docs[@]} -eq 0 ]]; then
  echo "No changed docs to stamp."
  exit 0
fi

for file in "${changed_docs[@]}"; do
  tmp_file="$(mktemp)"
  awk -v today="$today" '
    /^last_updated:[[:space:]][0-9]{4}-[0-9]{2}-[0-9]{2}$/ {
      print "last_updated: " today
      next
    }
    { print }
  ' "$file" > "$tmp_file"
  mv "$tmp_file" "$file"
  echo "Stamped $file"
done
