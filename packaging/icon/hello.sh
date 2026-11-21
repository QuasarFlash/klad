#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
klad="${KLAD:?set KLAD to the klad binary, or run just hello}"
work="${1:?give a folder for the three drawings, then the pictures go to crates/app/assets/hello}"
out="$here/../../crates/app/assets/hello"
mkdir -p "$work" "$out"
for part in plate ember ink; do
  if [ -e "$work/$part.klad" ]; then
    echo "$work/$part.klad is there already: give an empty folder" >&2
    exit 1
  fi
  "$klad" new "$work/$part.klad" --width 1024 --height 1024 --layers 0 >/dev/null
  for brush in icon-wet icon-dry; do
    python3 "$here/generate.py" --brush "$brush" | "$klad" brushes "$work/$part.klad" --set "$brush" - >/dev/null
  done
  python3 "$here/hello.py" "$part" | "$klad" apply "$work/$part.klad" >/dev/null
  "$klad" render "$work/$part.klad" --scale 0.5 --out "$out/$part.png" >/dev/null
done
python3 "$here/hello.py" paths >"$out/paths.json"
echo "$out"
