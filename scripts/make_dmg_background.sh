#!/bin/bash
# Draws assets/dmg/background.png and background@2x.png from
# assets/dmg/background.html, using Google Chrome. Run it again only if you
# change that page.
set -euo pipefail
cd "$(dirname "$0")/.."

CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
if [ ! -x "$CHROME" ]; then
  echo "Google Chrome is needed to draw the background. Set CHROME to its path." >&2
  exit 1
fi

PAGE="file://$PWD/assets/dmg/background.html"
PROFILE=$(mktemp -d)
trap 'rm -rf "$PROFILE"' EXIT

shoot() {
  "$CHROME" --headless=new --disable-gpu --hide-scrollbars --user-data-dir="$PROFILE" \
    --window-size=660,460 --force-device-scale-factor="$1" \
    --screenshot="$PWD/$2" "$PAGE" > /dev/null 2>&1
}
shoot 1 assets/dmg/background.png
shoot 2 assets/dmg/background@2x.png
echo "Wrote assets/dmg/background.png and background@2x.png"
