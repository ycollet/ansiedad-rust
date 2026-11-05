#!/usr/bin/env bash
set -euo pipefail

: "${VERSION:?}" "${BASE_VERSION:?}" "${TARGET:?}" "${MACOSX_DEPLOYMENT_TARGET:?}"
mkdir -p target/packaging dist
work="$(mktemp -d "$PWD/target/packaging/macos.XXXXXX")"
app="$work/image/ANSIedad.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
for binary in ansiedad ansiedad-gui; do
  cp "target/$TARGET/release/$binary" "$app/Contents/MacOS/$binary"
done
cp LICENSE README.md "$app/Contents/Resources/"
cp assets/ansiedad.icns "$app/Contents/Resources/"
cp -R examples "$app/Contents/Resources/"
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>ansiedad-gui</string>
  <key>CFBundleName</key><string>ANSIedad</string>
  <key>CFBundleDisplayName</key><string>ANSIedad</string>
  <key>CFBundleIdentifier</key><string>io.github.ansiedad-rust.gui</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleIconFile</key><string>ansiedad.icns</string>
  <key>CFBundleShortVersionString</key><string>$BASE_VERSION</string>
  <key>CFBundleVersion</key><string>$BASE_VERSION</string>
  <key>LSMinimumSystemVersion</key><string>$MACOSX_DEPLOYMENT_TARGET</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
EOF
plutil -lint "$app/Contents/Info.plist"
codesign --force --sign - "$app/Contents/MacOS/ansiedad"
codesign --force --sign - "$app"
codesign --verify --deep --strict "$app"
ln -s /Applications "$work/image/Applications"
image="$PWD/dist/ansiedad-$VERSION-$TARGET.dmg"
hdiutil create -volname "ANSIedad" -srcfolder "$work/image" -ov -format UDZO "$image"
hdiutil verify "$image"
mkdir -p "$work/mounted"
hdiutil attach "$image" -readonly -nobrowse -mountpoint "$work/mounted"
trap 'hdiutil detach "$work/mounted"' EXIT
ditto "$work/mounted/ANSIedad.app" "$work/installed/ANSIedad.app"
installed="$work/installed/ANSIedad.app"
codesign --verify --deep --strict "$installed"
for binary in ansiedad ansiedad-gui; do
  "$installed/Contents/MacOS/$binary" --version
  "$installed/Contents/MacOS/$binary" --check
done
"$installed/Contents/MacOS/ansiedad-gui" --check "$installed/Contents/Resources/examples/pixels.txt"
"$installed/Contents/MacOS/ansiedad-gui" --check "$installed/Contents/Resources/examples/characters.txt"
