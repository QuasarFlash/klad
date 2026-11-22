#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
out="${1:-$root/dist}"
targets="${KLAD_MAC_TARGETS:-aarch64-apple-darwin x86_64-apple-darwin}"
MACOSX_DEPLOYMENT_TARGET="$(grep -A1 LSMinimumSystemVersion "$root/packaging/macos/Info.plist" | sed -n 's/.*<string>\(.*\)<\/string>.*/\1/p')"
export MACOSX_DEPLOYMENT_TARGET
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)"
build="$(git -C "$root" describe --tags --always 2>/dev/null || echo "$version")"
export KLAD_BUILD="$build"
binaries=()
for target in $targets; do
  rustup target add "$target" >/dev/null 2>&1 || true
  cargo build --profile dist -p klad-cli --target "$target" --manifest-path "$root/Cargo.toml"
  binaries+=("${CARGO_TARGET_DIR:-$root/target}/$target/dist")
done
app="$out/klad.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
for exe in klad klad-editor; do
  inputs=()
  for dir in "${binaries[@]}"; do inputs+=("$dir/$exe"); done
  if [ "${#inputs[@]}" -gt 1 ]; then
    lipo -create "${inputs[@]}" -output "$app/Contents/MacOS/$exe"
  else
    cp "${inputs[0]}" "$app/Contents/MacOS/$exe"
  fi
done
sed -e "s/@VERSION@/$version/" -e "s/@BUILD@/$build/" "$root/packaging/macos/Info.plist" > "$app/Contents/Info.plist"
cp "$root/packaging/icon/klad.icns" "$app/Contents/Resources/"
rsync -a --exclude update.py "$root/fonts/" "$app/Contents/Resources/fonts/"
cp "$root/LICENSE" "$root/LICENSE-MIT" "$root/THIRD-PARTY-NOTICES.md" "$app/Contents/Resources/"
mkdir -p "$app/Contents/Resources/skill"
cp "$root/docs/skill/SKILL.md" "$app/Contents/Resources/skill/"
mkdir -p "$app/Contents/Resources/skill/drawing"
cp "$root/docs/skill/drawing/SKILL.md" "$app/Contents/Resources/skill/drawing/"
mkdir -p "$app/Contents/Resources/skill/pixel"
cp "$root/docs/skill/pixel/SKILL.md" "$app/Contents/Resources/skill/pixel/"
bash "$root/packaging/samples.sh" "${KLAD:-$app/Contents/MacOS/klad}" "$app/Contents/Resources/samples" >/dev/null
codesign --force --deep --sign - "$app" 2>/dev/null || true
echo "$app"
if [ "${KLAD_DMG:-1}" = "1" ]; then
  name="klad ${version%.*}"
  stage="$(mktemp -d)"
  cp -R "$app" "$stage/"
  ln -s /Applications "$stage/Applications"
  mkdir "$stage/.background"
  tiffutil -cathidpicheck "$root/packaging/macos/background.png" "$root/packaging/macos/background@2x.png" -out "$stage/.background/background.tiff" >/dev/null 2>&1 \
    || cp "$root/packaging/macos/background.png" "$stage/.background/background.tiff"
  cp "$root/packaging/icon/klad.icns" "$stage/.VolumeIcon.icns"
  dmg="$out/klad-$version-macos.dmg"
  work="$out/klad-$version-work.dmg"
  rm -f "$dmg" "$work"
  hdiutil create -volname "$name" -srcfolder "$stage" -ov -fs HFS+ -format UDRW "$work" >/dev/null
  rm -rf "$stage"
  mounted="$(hdiutil attach -readwrite -noverify -noautoopen "$work" | sed -n 's|.*\(/Volumes/.*\)|\1|p' | tail -1)"
  if [ -n "$mounted" ]; then
    xattr -wx com.apple.FinderInfo 0000000000000000040000000000000000000000000000000000000000000000 "$mounted" 2>/dev/null || true
    disk="$(basename "$mounted")"
    osascript <<APPLESCRIPT >/dev/null 2>&1 || echo "note: the window's look was not set (allow Terminal to control Finder in System Settings > Privacy & Security > Automation, then run this again)" >&2
tell application "Finder"
  tell disk "$disk"
    open
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set bounds of container window to {200, 120, 860, 520}
    set opts to the icon view options of container window
    set arrangement of opts to not arranged
    set icon size of opts to 112
    set text size of opts to 13
    set background picture of opts to file ".background:background.tiff"
    set position of item "klad.app" to {170, 190}
    set position of item "Applications" to {490, 190}
    update without registering applications
    delay 1
    close
  end tell
end tell
APPLESCRIPT
    sync
    hdiutil detach "$mounted" -quiet || hdiutil detach "$mounted" -force -quiet
  fi
  hdiutil convert "$work" -format UDZO -imagekey zlib-level=9 -o "$dmg" >/dev/null
  rm -f "$work"
  echo "$dmg"
fi
