#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
stage="$(cd "${1:?give the folder archive.sh made, for example dist/klad-1.0.0-windows-x86_64}" && pwd)"
out="$(cd "${2:-$(dirname "$stage")}" && pwd)"
iscc="${ISCC:-/c/Program Files (x86)/Inno Setup 6/ISCC.exe}"
[ -f "$iscc" ] || { echo "error: Inno Setup's ISCC.exe is not at $iscc; install Inno Setup 6, or point ISCC at it" >&2; exit 1; }
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
export KLAD_VERSION="$version" KLAD_NUMERIC="${version%%-*}" KLAD_STAGE="$(cygpath -w "$stage")" KLAD_OUT="$(cygpath -w "$out")"
MSYS_NO_PATHCONV=1 "$iscc" /Q "$(cygpath -w "$here/klad.iss")"
echo "$out/klad-$version-windows-x86_64-setup.exe"
