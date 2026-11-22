#!/usr/bin/env bash
set -euo pipefail

die() { echo "error: $*" >&2; exit 1; }
step() { echo; echo "==> $*"; }

identity() {
  if [ -n "${CODESIGN_IDENTITY:-}" ]; then
    echo "$CODESIGN_IDENTITY"
    return
  fi
  { security find-identity -v -p codesigning 2>/dev/null || true; } | sed -n '/"Developer ID Application: /{s/.*"\(Developer ID Application: .*\)"/\1/p;q;}'
}

mode="${1:-}"
target="${2:-}"
target="${target%/}"
[ "$mode" = identity ] && { identity; exit 0; }
[ "$mode" = app ] || [ "$mode" = dmg ] || die "usage: sign.sh identity | sign.sh app <path/to/klad.app> | sign.sh dmg <path/to/klad.dmg>"
[ -e "$target" ] || die "nothing to sign at $target"

id="$(identity)"
[ -n "$id" ] || die "no Developer ID Application identity in the keychain (security find-identity -v -p codesigning)"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

notary=(--keychain-profile "${NOTARY_PROFILE:-klad-notary}")
if [ -n "${APPLE_API_KEY_P8:-}" ] && [ -n "${APPLE_API_KEY_ID:-}" ] && [ -n "${APPLE_API_ISSUER_ID:-}" ]; then
  echo "$APPLE_API_KEY_P8" | base64 --decode > "$work/key.p8"
  notary=(--key "$work/key.p8" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER_ID")
fi

echo "Signing:  $target"
echo "Identity: $id"

notarise() {
  local upload="$1"
  step "Sending $(basename "$upload") to Apple's notary service (a few minutes)"
  local result="$work/result.json"
  if ! xcrun notarytool submit "$upload" "${notary[@]}" --wait --output-format json > "$result"; then
    cat "$result" >&2 || true
    die "notarytool submit failed (network or service error, see above)"
  fi
  local status submission
  status="$(plutil -extract status raw -o - "$result" 2>/dev/null || echo unknown)"
  submission="$(plutil -extract id raw -o - "$result" 2>/dev/null || echo unknown)"
  echo "Submission: $submission"
  echo "Status:     $status"
  if [ "$status" != "Accepted" ]; then
    xcrun notarytool log "$submission" "${notary[@]}" || true
    die "notarisation was not accepted (status: $status)"
  fi
}

staple() {
  step "Stapling the ticket to $(basename "$1")"
  xcrun stapler staple "$1"
  xcrun stapler validate "$1"
}

if [ "$mode" = dmg ]; then
  step "Signing the disk image"
  codesign --force --timestamp --sign "$id" "$target"
  codesign --verify --strict --verbose=2 "$target"
  notarise "$target"
  staple "$target"
  step "Gatekeeper"
  spctl -a -vvv -t open --context context:primary-signature "$target"
  exit 0
fi

[ -f "$target/Contents/Info.plist" ] || die "not an app bundle (no Contents/Info.plist): $target"
exe="$(grep -A1 CFBundleExecutable "$target/Contents/Info.plist" | sed -n 's/.*<string>\(.*\)<\/string>.*/\1/p')"
main="$target/Contents/MacOS/$exe"
[ -f "$main" ] || die "the main executable is missing: $main"

step "Signing the code inside, innermost first"
find "$target/Contents" -type f ! -path "$main" -print0 | while IFS= read -r -d '' file; do
  if file -b "$file" | grep -q "Mach-O"; then
    echo "  $file"
    codesign --force --options runtime --timestamp --sign "$id" "$file"
  fi
done
find "$target/Contents" -depth -type d \( -name "*.framework" -o -name "*.app" -o -name "*.xpc" -o -name "*.bundle" \) -print0 \
  | while IFS= read -r -d '' bundle; do
    echo "  $bundle"
    codesign --force --options runtime --timestamp --sign "$id" "$bundle"
  done

step "Signing the app"
entitlements=()
if [ -n "${ENTITLEMENTS:-}" ]; then
  [ -f "$ENTITLEMENTS" ] || die "no entitlements file at $ENTITLEMENTS"
  entitlements=(--entitlements "$ENTITLEMENTS")
fi
codesign --force --options runtime --timestamp ${entitlements[@]+"${entitlements[@]}"} --sign "$id" "$target"

step "Checking the signature"
codesign --verify --deep --strict --verbose=2 "$target"
info="$(codesign -dv --verbose=4 "$target" 2>&1)"
echo "$info" | grep -q "^Authority=Developer ID Application" || die "not signed with a Developer ID Application certificate"
echo "$info" | grep -q "flags=.*runtime" || die "the hardened runtime is not on"
echo "$info" | grep -q "^Timestamp=" || die "the secure timestamp is missing"

step "Packing the app to send"
upload="$work/$(basename "$target" .app).zip"
ditto -c -k --keepParent "$target" "$upload"
mkdir "$work/check"
ditto -x -k "$upload" "$work/check"
codesign --verify --deep --strict "$work/check/$(basename "$target")" || die "the app inside the archive does not verify"

notarise "$upload"
staple "$target"
step "Gatekeeper"
spctl -a -vvv -t exec "$target"
