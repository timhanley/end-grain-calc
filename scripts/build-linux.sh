#!/bin/bash
# Builds End Grain Calc for Linux into dist/end-grain-calc/.
#
#   ./scripts/build-linux.sh              build it
#   ./scripts/build-linux.sh --install    build it and add it to your applications menu
#   ./scripts/build-linux.sh --uninstall  take it out of the menu again
#
# --install only writes inside your home folder (~/.local), so it needs no sudo.
set -euo pipefail
cd "$(dirname "$0")/.."
[ -d "$HOME/.cargo/bin" ] && export PATH="$HOME/.cargo/bin:$PATH"

BIN="$HOME/.local/bin/endgrain"
ICON="$HOME/.local/share/icons/hicolor/512x512/apps/end-grain-calc.png"
ENTRY="$HOME/.local/share/applications/end-grain-calc.desktop"

case "${1:-}" in
  "" | --install) ;;
  --uninstall)
    rm -f "$BIN" "$ICON" "$ENTRY"
    echo "Removed End Grain Calc from your applications menu. Your boards in ~/.endgrain are untouched."
    exit 0
    ;;
  *)
    echo "Unknown option: $1. Use --install, --uninstall or nothing." >&2
    exit 2
    ;;
esac

if ! command -v cargo >/dev/null; then
  echo "Rust isn't installed. Get it from https://rustup.rs, then run this again." >&2
  exit 1
fi

# The window is drawn by WebKitGTK, which has to be on the system to build.
if ! command -v pkg-config >/dev/null || ! pkg-config --exists webkit2gtk-4.1 gtk+-3.0; then
  cat >&2 <<'HELP'
The WebKitGTK and GTK 3 development files are needed to build the app window.
Install them, then run this again:

  Debian, Ubuntu, Mint:  sudo apt install build-essential pkg-config libgtk-3-dev libwebkit2gtk-4.1-dev
  Fedora:                sudo dnf install gcc pkgconf-pkg-config gtk3-devel webkit2gtk4.1-devel
  Arch, Manjaro:         sudo pacman -S --needed base-devel gtk3 webkit2gtk-4.1

Or skip the window and use your web browser instead, which needs none of them:

  cargo run --release --no-default-features
HELP
  exit 1
fi

cargo build --release

OUT="dist/end-grain-calc"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
rm -rf "$OUT"
mkdir -p "$OUT"
cp target/release/endgrain "$OUT/endgrain"
cp assets/icon.png "$OUT/icon.png"
echo "Built $OUT/endgrain (version $VERSION, $(du -sh "$OUT/endgrain" | cut -f1))"

if [ "${1:-}" != "--install" ]; then
  echo "Run it with: ./$OUT/endgrain"
  echo "To add it to your applications menu: ./scripts/build-linux.sh --install"
  exit 0
fi

mkdir -p "$(dirname "$BIN")" "$(dirname "$ICON")" "$(dirname "$ENTRY")"
cp "$OUT/endgrain" "$BIN"
# The icon is drawn at 1024 px; the menu wants the size its folder names.
if command -v convert >/dev/null; then
  convert "$OUT/icon.png" -resize 512x512 "$ICON"
else
  cp "$OUT/icon.png" "$ICON"
fi
cat > "$ENTRY" <<ENTRY
[Desktop Entry]
Type=Application
Name=End Grain Calc
Comment=End grain chopping board calculator
Exec="$BIN"
Icon=$ICON
Terminal=false
Categories=Utility;
ENTRY
command -v update-desktop-database >/dev/null && update-desktop-database "$(dirname "$ENTRY")" 2>/dev/null || true
echo "Installed. Look for End Grain Calc in your applications menu."
echo "To remove it: ./scripts/build-linux.sh --uninstall"
