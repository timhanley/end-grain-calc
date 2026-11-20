#!/usr/bin/env python3
"""Draws the app icon (a walnut and cherry end grain board) and packs it as
assets/icon.icns for the Mac and assets/icon.ico for Windows. Only needs
Python 3 and the sips/iconutil tools that ship with macOS. Run it again only
if you change the drawing below; --ico-only remakes just the Windows icon
from the existing assets/icon.png."""
import math, os, struct, subprocess, sys, tempfile, zlib

SIZE, INSET, RADIUS = 1024, 100, 185
SIDE = SIZE - 2 * INSET
WALNUT, CHERRY = (0x5A, 0x3D, 0x2B), (0xB9, 0x69, 0x3C)
# (start, end, timber) bands down the board, in board pixels
BANDS = [(0, 230, WALNUT), (230, 460, WALNUT), (460, 540, CHERRY), (540, 590, WALNUT),
         (590, 670, CHERRY), (670, 720, WALNUT), (720, 780, CHERRY), (780, SIDE, WALNUT)]
COLS = 6


def coverage(x, y):
    """How much of pixel (x, y) is inside the rounded square, 0..1."""
    half = SIDE / 2
    dx = abs(x + 0.5 - SIZE / 2) - (half - RADIUS)
    dy = abs(y + 0.5 - SIZE / 2) - (half - RADIUS)
    dist = math.hypot(max(dx, 0), max(dy, 0)) + min(max(dx, dy), 0) - RADIUS
    return min(max(0.5 - dist, 0), 1)


def pixel(x, y):
    a = coverage(x, y)
    if a <= 0:
        return (0, 0, 0, 0)
    u, v = x - INSET, y - INSET
    row = next(i for i, b in enumerate(BANDS) if v < b[1] or i == len(BANDS) - 1)
    top, bottom, colour = BANDS[row]
    cell_w = SIDE / COLS
    col = min(int(u // cell_w), COLS - 1)
    # Growth rings curve around a corner of each cell; neighbours mirror.
    cx = (col + (col % 2)) * cell_w
    cy = bottom if row % 2 else top
    ring = math.sin(math.hypot(u - cx, (v - cy) * 1.25) / 7.5)
    shade = 1 + (0.085 if colour is WALNUT else 0.05) * ring
    shade *= 1.07 - 0.14 * v / SIDE
    # Glue lines between slices and strips.
    edge = min(u % cell_w, cell_w - u % cell_w, v - top, bottom - v)
    if edge < 2.2 and 0 < u < SIDE:
        shade *= 0.74
    return tuple(min(255, int(c * shade)) for c in colour) + (int(a * 255),)


def png(path):
    rows = bytearray()
    for y in range(SIZE):
        rows.append(0)
        for x in range(SIZE):
            rows.extend(pixel(x, y))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0))
                + chunk(b"IDAT", zlib.compress(bytes(rows), 9)) + chunk(b"IEND", b""))


def ico(master, path):
    """A Windows .ico holding PNG images at the sizes Windows asks for."""
    sizes = (16, 24, 32, 48, 64, 128, 256)
    images = []
    with tempfile.TemporaryDirectory() as tmp:
        for size in sizes:
            out = os.path.join(tmp, f"{size}.png")
            subprocess.run(["sips", "-z", str(size), str(size), master, "--out", out],
                           check=True, stdout=subprocess.DEVNULL)
            with open(out, "rb") as f:
                images.append(f.read())
    head = struct.pack("<HHH", 0, 1, len(sizes))
    offset = len(head) + 16 * len(sizes)
    entries = b""
    for size, data in zip(sizes, images):
        # A size of 256 is written as 0.
        entries += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    with open(path, "wb") as f:
        f.write(head + entries + b"".join(images))


def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    out = os.path.join(root, "assets")
    os.makedirs(out, exist_ok=True)
    master = os.path.join(out, "icon.png")
    if "--ico-only" in sys.argv[1:]:
        ico(master, os.path.join(out, "icon.ico"))
        print("Wrote assets/icon.ico")
        return 0
    png(master)
    ico(master, os.path.join(out, "icon.ico"))
    with tempfile.TemporaryDirectory() as tmp:
        iconset = os.path.join(tmp, "icon.iconset")
        os.mkdir(iconset)
        for size in (16, 32, 128, 256, 512):
            for scale in (1, 2):
                name = f"icon_{size}x{size}{'@2x' if scale == 2 else ''}.png"
                subprocess.run(["sips", "-z", str(size * scale), str(size * scale), master, "--out", os.path.join(iconset, name)],
                               check=True, stdout=subprocess.DEVNULL)
        subprocess.run(["iconutil", "-c", "icns", iconset, "-o", os.path.join(out, "icon.icns")], check=True)
    print("Wrote assets/icon.png, assets/icon.icns and assets/icon.ico")


if __name__ == "__main__":
    sys.exit(main())
