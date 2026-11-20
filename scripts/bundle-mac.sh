#!/bin/bash
# Builds "dist/End Grain Calc.app", a double-clickable Mac app.
#
#   ./scripts/bundle-mac.sh               for this Mac's processor
#   ./scripts/bundle-mac.sh --universal   for Apple silicon and Intel in one app
#   ./scripts/bundle-mac.sh --dmg         also pack it as dist/End-Grain-Calc-mac.dmg
#
# --universal needs both Rust targets:
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
set -euo pipefail
cd "$(dirname "$0")/.."
[ -d "$HOME/.cargo/bin" ] && export PATH="$HOME/.cargo/bin:$PATH"

UNIVERSAL=0
DMG=0
for arg in "$@"; do
  case "$arg" in
    --universal) UNIVERSAL=1 ;;
    --dmg) DMG=1 ;;
    *) echo "Unknown option: $arg. Use --universal, --dmg or nothing." >&2; exit 2 ;;
  esac
done

# The oldest macOS the app promises to run on (LSMinimumSystemVersion below).
export MACOSX_DEPLOYMENT_TARGET=11.0

if [ "$UNIVERSAL" = 1 ]; then
  cargo build --release --target aarch64-apple-darwin
  cargo build --release --target x86_64-apple-darwin
  mkdir -p target/universal
  lipo -create -output target/universal/endgrain \
    target/aarch64-apple-darwin/release/endgrain \
    target/x86_64-apple-darwin/release/endgrain
  BIN=target/universal/endgrain
else
  cargo build --release
  BIN=target/release/endgrain
fi

APP="dist/End Grain Calc.app"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/endgrain"
cp assets/icon.icns "$APP/Contents/Resources/icon.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>End Grain Calc</string>
  <key>CFBundleDisplayName</key><string>End Grain Calc</string>
  <key>CFBundleIdentifier</key><string>app.endgrain.calculator</string>
  <key>CFBundleExecutable</key><string>endgrain</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
  <key>NSHumanReadableCopyright</key><string>© 2026 TEA JAY HAN. MIT licence.</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

# Ad-hoc signature: enough to run on the Mac that built it. Downloaded on
# another Mac, macOS asks for it to be allowed once (see the README).
codesign --force --sign - "$APP"

echo "Built $APP ($(du -sh "$APP" | cut -f1), $(lipo -archs "$APP/Contents/MacOS/endgrain"))"

if [ "$DMG" = 1 ]; then
  # A disk image with the app and a shortcut to Applications to drag it to.
  DMG_FILE="dist/End-Grain-Calc-mac.dmg"
  STAGE=$(mktemp -d)
  trap 'rm -rf "$STAGE"' EXIT
  cp -R "$APP" "$STAGE/"
  ln -s /Applications "$STAGE/Applications"
  rm -f "$DMG_FILE"
  hdiutil create -quiet -volname "End Grain Calc" -srcfolder "$STAGE" -fs HFS+ -format UDZO "$DMG_FILE"
  echo "Built $DMG_FILE ($(du -sh "$DMG_FILE" | cut -f1))"
fi
