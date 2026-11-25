#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
version=2.10.1
sha256=1a856d2a9edb8e628de88121d7be04cf0e2167f8641650b78fca00b14244cf27
target="${CARGO_TARGET_DIR:-$root/target}"
case "$target" in
  /*) ;;
  *) target="$PWD/$target" ;;
esac
dir="$target/sparkle/$version"
if [ ! -x "$dir/bin/sign_update" ] || [ ! -d "$dir/Sparkle.framework" ]; then
  mkdir -p "$target/sparkle"
  work="$(mktemp -d "$target/sparkle/download.XXXXXX")"
  archive="$work/Sparkle-$version.tar.xz"
  curl -fsSL -o "$archive" "https://github.com/sparkle-project/Sparkle/releases/download/$version/Sparkle-$version.tar.xz"
  if command -v sha256sum >/dev/null; then got="$(sha256sum "$archive")"; else got="$(shasum -a 256 "$archive")"; fi
  got="${got%% *}"
  [ "$got" = "$sha256" ] || { echo "error: Sparkle-$version.tar.xz has sha256 $got, not $sha256" >&2; exit 1; }
  mkdir "$work/sparkle"
  tar -xJf "$archive" -C "$work/sparkle"
  [ ! -e "$dir" ] || mv "$dir" "$work/old"
  mv "$work/sparkle" "$dir"
  rm -rf "$work"
fi
echo "$dir"
