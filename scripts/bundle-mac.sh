#!/bin/bash
# Builds "dist/End Grain Calc.app", a double-clickable Mac app.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

cargo build --release

APP="dist/End Grain Calc.app"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/endgrain "$APP/Contents/MacOS/endgrain"
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

# Ad-hoc signature: enough to run on this Mac. Another Mac will show a
# Gatekeeper warning the first time (right-click the app, then Open).
codesign --force --sign - "$APP"

echo "Built $APP ($(du -sh "$APP" | cut -f1))"
