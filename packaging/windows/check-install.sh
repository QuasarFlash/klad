#!/usr/bin/env bash
set -euo pipefail
setup="${1:?give the setup .exe to check}"
app="$(cygpath -u "$LOCALAPPDATA")/Programs/klad"
on_path() {
  local value
  value="$(MSYS_NO_PATHCONV=1 reg query 'HKCU\Environment' /v Path 2>/dev/null || true)"
  grep -qi 'Programs\\klad' <<<"$value"
}
set -x
MSYS_NO_PATHCONV=1 "$setup" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART
"$app/klad.exe" --version
test -f "$app/klad-editor.exe"
test -f "$app/fonts/fonts.json"
test -d "$app/samples"
test -f "$app/unins000.exe"
on_path || { echo "error: the installer did not put $app on the PATH" >&2; exit 1; }
MSYS_NO_PATHCONV=1 "$app/unins000.exe" /VERYSILENT /SUPPRESSMSGBOXES
set +x
gone() { [ ! -e "$app/klad.exe" ] && [ ! -e "$app/unins000.exe" ] && ! on_path; }
for _ in $(seq 60); do
  gone && break
  sleep 1
done
[ ! -e "$app/klad.exe" ] || { echo "error: klad.exe is still there a minute after the uninstaller" >&2; exit 1; }
[ ! -e "$app/unins000.exe" ] || { echo "error: the uninstaller had not finished a minute later" >&2; exit 1; }
if on_path; then
  echo "error: the uninstaller left $app on the PATH:" >&2
  MSYS_NO_PATHCONV=1 reg query 'HKCU\Environment' /v Path >&2 || true
  exit 1
fi
echo "installed, on the PATH, uninstalled and off the PATH again"
