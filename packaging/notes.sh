#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
version="${1:?give the version, like 1.0.0}"
version="${version#v}"
notes="$(awk -v v="$version" '$0 ~ "^## "v"( |$)" {on=1; next} /^## / {on=0} on' "$root/CHANGELOG.md")"
if [ -z "$(printf '%s' "$notes" | tr -d '[:space:]')" ]; then
  echo "klad $version"
else
  printf '%s\n' "$notes" | sed '/./,$!d'
fi
