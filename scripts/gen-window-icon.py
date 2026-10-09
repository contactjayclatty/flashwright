#!/usr/bin/env python3
"""Draw the window icon. Original geometry, brand colours only."""

import struct
import zlib
from pathlib import Path

TEAL = (10, 92, 92, 255)
PAPER = (242, 244, 245, 255)
AMBER = (227, 155, 45, 255)

# 16×16. T teal, P paper, A amber.
GRID = [
    "TTTTTTTTTTTTTTTT",
    "TTTTTTTTTTTTTTTT",
    "TTPPPPPPPPPPPPTT",
    "TTPPPPPPPPPPPPTT",
    "TTPPAAAAAAPPPPTT",
    "TTPPPPPPPAPPPPTT",
    "TTPPPPPPAPPPPPTT",
    "TTPPPPPAPPPPPPTT",
    "TTPPPPAPPPPPPPTT",
    "TTPPPAPPPPPPPPTT",
    "TTPPAAAAAAPPPPTT",
    "TTPPPPPPPPPPPPTT",
    "TTPPPPPPPPPPPPTT",
    "TTTTTTTTTTTTTTTT",
    "TTTTTTTTTTTTTTTT",
    "TTTTTTTTTTTTTTTT",
]

COLOURS = {"T": TEAL, "P": PAPER, "A": AMBER}


def png_bytes(width: int, height: int, rgba: bytes) -> bytes:
    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    raw = b"".join(b"\x00" + rgba[y * width * 4 : (y + 1) * width * 4] for y in range(height))
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def base_pixels() -> list[tuple[int, int, int, int]]:
    pixels: list[tuple[int, int, int, int]] = []
    for row in GRID:
        for cell in row:
            pixels.append(COLOURS[cell])
    return pixels


def scale(pixels: list[tuple[int, int, int, int]], factor: int) -> tuple[int, bytes]:
    size = 16 * factor
    out = bytearray()
    for y in range(size):
        src_y = y // factor
        for x in range(size):
            src_x = x // factor
            out.extend(pixels[src_y * 16 + src_x])
    return size, bytes(out)


def write_png(path: Path, size: int, rgba: bytes) -> bytes:
    data = png_bytes(size, size, rgba)
    path.write_bytes(data)
    return data


def write_ico(path: Path, images: list[tuple[int, bytes]]) -> None:
    count = len(images)
    header = struct.pack("<HHH", 0, 1, count)
    offset = 6 + 16 * count
    entries = b""
    blobs = b""
    for size, png in images:
        width = 0 if size >= 256 else size
        height = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", width, height, 0, 0, 1, 32, len(png), offset)
        offset += len(png)
        blobs += png
    path.write_bytes(header + entries + blobs)


def main() -> None:
    root = Path(__file__).resolve().parents[1]
    tauri_icons = root / "apps/flashwright-gui/src-tauri/icons"
    theme_assets = root / "apps/flashwright-gui/ui/theme/assets"
    tauri_icons.mkdir(parents=True, exist_ok=True)
    theme_assets.mkdir(parents=True, exist_ok=True)
    pixels = base_pixels()
    ico_images: list[tuple[int, bytes]] = []
    names = {1: "16x16.png", 2: "32x32.png", 8: "128x128.png", 16: "128x128@2x.png"}
    for factor, name in names.items():
        size, rgba = scale(pixels, factor)
        data = write_png(tauri_icons / name, size, rgba)
        if factor in (2, 8, 16):
            ico_images.append((size, data))
        if factor == 2:
            write_png(theme_assets / "app-icon.png", size, rgba)
    write_ico(tauri_icons / "icon.ico", ico_images)
    (tauri_icons / "icon.png").write_bytes((tauri_icons / "128x128@2x.png").read_bytes())


if __name__ == "__main__":
    main()
