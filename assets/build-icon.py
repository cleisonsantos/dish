#!/usr/bin/env python3
"""Build the system icons from the canonical, plate-only dish-logo.svg.

Edit dish-logo.svg to change the design. Requires rsvg-convert for raster output.
Run: python3 assets/build-icon.py

Produces, all derived from the same render of the same artwork:

  dish-icon.svg / dish-mark.svg   copies of the canonical artwork
  dish-icon.png                   512px PNG, the Linux hicolor icon
  dish-icon.ico                   Windows: 16, 32, 48, 64, 128 and 256px
  dish-icon.icns                  macOS: the ic07..ic14 element set

The `.ico` and `.icns` store their images as PNG, which Windows Vista+ and
macOS 10.7+ read directly, so the plate keeps one source and one look.
"""
from pathlib import Path
import shutil
import struct
import subprocess
import sys

ICO_SIZES = (16, 32, 48, 64, 128, 256)
# (element type, pixel size) — the standard modern set, including @2x slots.
ICNS_ELEMENTS = (
    ("ic07", 128),
    ("ic08", 256),
    ("ic09", 512),
    ("ic10", 1024),
    ("ic11", 32),
    ("ic12", 64),
    ("ic13", 256),
    ("ic14", 512),
)


def render(renderer: str, source: Path, size: int) -> bytes:
    """Render the canonical SVG at `size` and return the PNG bytes."""
    result = subprocess.run(
        [renderer, str(source), "-w", str(size), "-h", str(size), "-f", "png"],
        check=True,
        capture_output=True,
    )
    if result.stdout[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"rsvg-convert did not return a PNG for {size}px")
    return result.stdout


def write_ico(path: Path, images: list[tuple[int, bytes]]) -> None:
    """Pack PNG images into a Windows .ico.

    Each directory entry carries the dimensions, a 32-bit depth and the offset
    of its payload; 256px is encoded as 0, per the format.
    """
    header = struct.pack("<HHH", 0, 1, len(images))
    directory = b""
    payloads = b""
    offset = len(header) + 16 * len(images)
    for size, data in images:
        side = 0 if size >= 256 else size
        directory += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(data), offset)
        payloads += data
        offset += len(data)
    path.write_bytes(header + directory + payloads)


def write_icns(path: Path, elements: list[tuple[str, bytes]]) -> None:
    """Pack PNG images into a macOS .icns.

    Every element is a 4-byte OSType, a big-endian length that includes its own
    8-byte header, and the payload; the file itself repeats the length after
    the `icns` magic.
    """
    body = b"".join(
        kind.encode("ascii") + struct.pack(">I", len(data) + 8) + data
        for kind, data in elements
    )
    path.write_bytes(b"icns" + struct.pack(">I", len(body) + 8) + body)


def main() -> None:
    here = Path(__file__).resolve().parent
    renderer = shutil.which("rsvg-convert")
    if not renderer:
        raise SystemExit("Install rsvg-convert (librsvg2-bin) to generate the icons.")
    source = here / "dish-logo.svg"
    for name in ("dish-icon.svg", "dish-mark.svg"):
        shutil.copyfile(source, here / name)

    # Rasterise each distinct size once; every format reuses the same renders.
    wanted = sorted({512, *ICO_SIZES, *(size for _, size in ICNS_ELEMENTS)})
    rendered = {size: render(renderer, source, size) for size in wanted}

    (here / "dish-icon.png").write_bytes(rendered[512])
    write_ico(here / "dish-icon.ico", [(size, rendered[size]) for size in ICO_SIZES])
    write_icns(
        here / "dish-icon.icns",
        [(kind, rendered[size]) for kind, size in ICNS_ELEMENTS],
    )
    print(
        "Generated dish-icon.svg, dish-mark.svg, dish-icon.png, "
        f"dish-icon.ico ({len(ICO_SIZES)} sizes) and dish-icon.icns "
        f"({len(ICNS_ELEMENTS)} elements) from dish-logo.svg"
    )


if __name__ == "__main__":
    sys.exit(main())
