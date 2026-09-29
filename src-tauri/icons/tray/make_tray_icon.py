#!/usr/bin/env python3
"""Draws the Pawtrol menu-bar paw as macOS template PNGs (black on transparent)."""
import math
import os
import struct
import zlib

# Shapes on a 36x36 canvas: (cx, cy, rx, ry, rotation_degrees)
TOES = [
    (7.0, 15.5, 3.3, 4.3, -28),
    (13.6, 8.6, 3.6, 4.7, -8),
    (22.4, 8.6, 3.6, 4.7, 8),
    (29.0, 15.5, 3.3, 4.3, 28),
]
SS = 8


def in_ellipse(x, y, cx, cy, rx, ry, rot):
    a = math.radians(rot)
    dx, dy = x - cx, y - cy
    u = dx * math.cos(a) + dy * math.sin(a)
    v = -dx * math.sin(a) + dy * math.cos(a)
    return (u / rx) ** 2 + (v / ry) ** 2 <= 1.0


def in_pad(x, y):
    # Rounded heart-ish pad: a wide ellipse plus two lower lobes.
    return (
        in_ellipse(x, y, 18, 22.5, 7.6, 6.2, 0)
        or in_ellipse(x, y, 13.4, 26.6, 4.6, 4.2, 0)
        or in_ellipse(x, y, 22.6, 26.6, 4.6, 4.2, 0)
    )


def coverage(px, py, scale):
    hits = 0
    for sy in range(SS):
        for sx in range(SS):
            x = (px + (sx + 0.5) / SS) * 36 / scale
            y = (py + (sy + 0.5) / SS) * 36 / scale
            if in_pad(x, y) or any(in_ellipse(x, y, *t) for t in TOES):
                hits += 1
    return round(255 * hits / (SS * SS))


def rgba_rows(size):
    return [b"".join(bytes((0, 0, 0, coverage(px, py, size))) for px in range(size)) for py in range(size)]


def write_png(path, size):
    rows = b"".join(b"\x00" + r for r in rgba_rows(size))

    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(rows, 9))
    png += chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


if __name__ == "__main__":
    here = os.path.dirname(os.path.abspath(__file__))
    write_png(os.path.join(here, "trayTemplate.png"), 18)
    write_png(os.path.join(here, "trayTemplate@2x.png"), 36)
    # Raw RGBA for include_bytes!, so the app needs no PNG decoder.
    with open(os.path.join(here, "trayTemplate@2x.rgba"), "wb") as f:
        f.write(b"".join(rgba_rows(36)))
