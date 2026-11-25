#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
klad="${1:?give the klad binary that builds the samples}"
dest="${2:?give the folder the samples go in}"
python=""
for candidate in python3 python; do
  if command -v "$candidate" >/dev/null && "$candidate" -c "" 2>/dev/null; then
    python="$(command -v "$candidate")"
    break
  fi
done
[ -n "$python" ] || { echo "error: the samples need Python 3, as python3 or python" >&2; exit 1; }
shell="$BASH"
if command -v cygpath >/dev/null; then shell="$(cygpath -w "$BASH")"; fi
export PYTHONUTF8=1
rm -rf "$dest"
mkdir -p "$dest"
"$python" - "$root/samples" "$dest" "$klad" "$python" "$shell" <<'PY'
import json, os, pathlib, shutil, subprocess, sys
src, dest, klad, python, shell = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5]
def pixel_art(layers):
    return bool(layers) and all(l.get("pixel") or pixel_art(l.get("children", [])) for l in layers)
def run(command, env=None):
    done = subprocess.run(command, env=env, capture_output=True, text=True)
    if done.returncode != 0:
        sys.exit(f"{' '.join(command)} failed:\n{done.stderr}")
for entry in json.loads((src / "shipped.json").read_text()):
    bundle = src / entry["bundle"]
    if not (bundle / "doc.json").is_file():
        env = dict(os.environ, KLAD=klad, PYTHON=python, FORCE="1")
        run([shell, (bundle.parent / "build.sh").as_posix(), bundle.as_posix()], env)
    shutil.copytree(bundle, dest / entry["bundle"], ignore=shutil.ignore_patterns(".cache"))
    doc = json.loads((bundle / "doc.json").read_text())
    canvas = doc["canvas"]
    side = max(canvas["width"], canvas["height"])
    scale = max(1, 336 // side) if pixel_art(doc["layers"]) else min(1.0, 336 / side)
    thumb = (dest / entry["bundle"]).with_name(bundle.stem + ".thumb.png")
    run([klad, "render", str(bundle), "--scale", f"{scale:.4f}", "-o", str(thumb)])
shutil.copy2(src / "shipped.json", dest / "shipped.json")
PY
echo "$dest"
