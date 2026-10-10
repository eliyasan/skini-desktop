#!/usr/bin/env bash
# Builds a universal (Apple Silicon + Intel) Skini.app, Skini-mac.zip and Skini-mac.dmg in dist/.
# Run on a Mac with Rust installed (rustup target add aarch64-apple-darwin x86_64-apple-darwin).
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
export MACOSX_DEPLOYMENT_TARGET=11.0
for t in aarch64-apple-darwin x86_64-apple-darwin; do
  cargo build --release --target "$t"
done

rm -rf dist && mkdir -p dist
APP=dist/Skini.app
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create -output "$APP/Contents/MacOS/skini" \
  target/aarch64-apple-darwin/release/skini target/x86_64-apple-darwin/release/skini
sed "s/__VERSION__/$VERSION/g" macos/Info.plist > "$APP/Contents/Info.plist"

# Icon from the Windows .ico (best effort: the app works without it).
if sips -s format png skini.ico --out dist/icon.png >/dev/null 2>&1; then
  ICONSET=dist/AppIcon.iconset; mkdir -p "$ICONSET"
  for s in 16 32 128 256 512; do
    sips -z $s $s dist/icon.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
    sips -z $((s*2)) $((s*2)) dist/icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
  done
  iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns" || true
  rm -rf "$ICONSET" dist/icon.png
fi

# Ad-hoc signature (no Apple Developer account): required for Apple Silicon to run it at all.
codesign --force --deep --sign - "$APP"
codesign --verify --verbose "$APP"

ditto -c -k --keepParent "$APP" dist/Skini-mac.zip

STAGE=dist/dmg; mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname Skini -srcfolder "$STAGE" -ov -format UDZO dist/Skini-mac.dmg
rm -rf "$STAGE"
ls -la dist
