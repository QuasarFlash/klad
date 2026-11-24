#!/usr/bin/env bash
set -euo pipefail
klad="${1:?give the klad binary}"
sizes="${2:-4096 8192 16384}"
out=/tmp/klad-memory
mkdir -p "$out"

measure() {
  python3 - "$@" <<'PY'
import resource, subprocess, sys, time
start = time.time()
done = subprocess.run(sys.argv[1:], stdout=subprocess.DEVNULL)
peak = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
scale = 1 if sys.platform == "darwin" else 1024
print(f"{peak * scale / 1048576:.0f} MB peak, {time.time() - start:.1f} s" + ("" if done.returncode == 0 else f", failed ({done.returncode})"))
PY
}

echo "$("$klad" --version), $(uname -sm), $(( $(sysctl -n hw.memsize 2>/dev/null || awk '/MemTotal/ {print $2 * 1024}' /proc/meminfo) / 1073741824 )) GB"
for s in $sizes; do
  drawing="$out/s$s.klad"
  if [ ! -d "$drawing" ]; then
    echo "s$s: making a $s x $s drawing of 30 layers and 12 frames..."
    "$klad" stress "$drawing" --size "$s" --frames 12 >/dev/null
  fi
  echo "s$s render: $(measure "$klad" render "$drawing" -o "$out/s$s.png")"
done
echo "the drawings are in $out; open them with: $klad open $out/s8192.klad"
