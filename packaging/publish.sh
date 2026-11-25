#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
version="${1:?give the version the file belongs to, for example 1.0.0}"
platform="${2:?give the platform: macos, windows or linux}"
file="${3:?give the file to publish}"
case "$platform" in
  macos | windows | linux) ;;
  *) echo "error: the platform is macos, windows or linux, not $platform" >&2; exit 1 ;;
esac
[ -f "$file" ] || { echo "error: $file is not there" >&2; exit 1; }
for setting in R2_ACCOUNT_ID R2_BUCKET R2_ACCESS_KEY_ID R2_SECRET_ACCESS_KEY; do
  if [ -z "${!setting:-}" ] && command -v security >/dev/null; then
    kept="$(security find-generic-password -s klad-r2 -a "$setting" -w 2>/dev/null || true)"
    [ -z "$kept" ] || export "$setting=$kept"
  fi
  if [ -z "${!setting:-}" ]; then
    echo "error: $setting is not set; export it, or keep it in the keychain once with: security add-generic-password -s klad-r2 -a $setting -w" >&2
    exit 1
  fi
done
endpoint="${R2_ENDPOINT:-https://$R2_ACCOUNT_ID.r2.cloudflarestorage.com}"
region="${R2_REGION:-auto}"
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) python=python ;;
  *) python=python3 ;;
esac
hash() {
  if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}
work="$(mktemp -d)"
trap 'rm -f "$work/answer" "$work/headers" "$work/releases.json" "$work/shelf.json"; rmdir "$work"' EXIT
r2() {
  printf 'user = "%s:%s"\n' "$R2_ACCESS_KEY_ID" "$R2_SECRET_ACCESS_KEY" |
    curl --config - --silent --show-error --aws-sigv4 "aws:amz:$region:s3" "$@"
}
put() {
  local from="$1" key="$2" type="$3"
  shift 3
  r2 --output "$work/answer" --write-out '%{http_code}' --request PUT \
    --header "x-amz-content-sha256: $(hash "$from")" --header "content-type: $type" "$@" \
    --upload-file "$from" "$endpoint/$R2_BUCKET/$key"
}

name="$(basename "$file")"
key="$version/$name"
size="$(wc -c <"$file" | tr -d ' ')"
sha256="$(hash "$file")"
case "$name" in
  *.dmg) type=application/x-apple-diskimage ;;
  *.exe) type=application/vnd.microsoft.portable-executable ;;
  *.zip) type=application/zip ;;
  *.gz) type=application/gzip ;;
  *) type=application/octet-stream ;;
esac
echo "uploading $name ($size bytes) to $R2_BUCKET as $key"
status="$(put "$file" "$key" "$type")"
[ "$status" = 200 ] || { echo "error: the bucket answered $status to $key: $(cat "$work/answer")" >&2; exit 1; }

if [[ "$version" == *-* ]]; then
  echo "$version is a pre-release, so releases.json stays as it is; the file is at $key"
  exit 0
fi
empty=e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
for attempt in 1 2 3 4 5; do
  status="$(r2 --output "$work/releases.json" --dump-header "$work/headers" --write-out '%{http_code}' \
    --header "x-amz-content-sha256: $empty" "$endpoint/$R2_BUCKET/releases.json")"
  case "$status" in
    200)
      etag="$(tr -d '\r' <"$work/headers" | sed -n 's/^[Ee][Tt][Aa][Gg]: *//p' | tail -1)"
      condition=(--header "if-match: $etag")
      ;;
    404)
      echo '{"releases": []}' >"$work/releases.json"
      condition=(--header "if-none-match: *")
      ;;
    *)
      echo "error: the bucket answered $status to releases.json: $(cat "$work/releases.json")" >&2
      exit 1
      ;;
  esac
  "$python" "$here/releases.py" "$work/releases.json" "$version" "$platform" "$name" "$key" "$size" "$sha256" >"$work/shelf.json"
  status="$(put "$work/shelf.json" releases.json application/json "${condition[@]}")"
  case "$status" in
    200)
      echo "releases.json lists $key under $version"
      exit 0
      ;;
    412) echo "releases.json changed while it was read (try $attempt); reading it again" ;;
    *)
      echo "error: the bucket answered $status to releases.json: $(cat "$work/answer")" >&2
      exit 1
      ;;
  esac
done
echo "error: releases.json kept changing; run this again" >&2
exit 1
