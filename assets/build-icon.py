#!/usr/bin/env python3
"""Build the system icon from the canonical, plate-only dish-logo.svg.

Edit dish-logo.svg to change the design. Requires rsvg-convert for PNG output.
Run: python3 assets/build-icon.py
"""
from pathlib import Path
import shutil
import subprocess


def main() -> None:
    here = Path(__file__).resolve().parent
    renderer = shutil.which("rsvg-convert")
    if not renderer:
        raise SystemExit("Install rsvg-convert (librsvg2-bin) to generate the PNG icon.")
    source = here / "dish-logo.svg"
    for name in ("dish-icon.svg", "dish-mark.svg"):
        shutil.copyfile(source, here / name)
    subprocess.run(
        [renderer, str(source), "-w", "512", "-h", "512", "-o", str(here / "dish-icon.png")],
        check=True,
    )
    print("Generated dish-icon.svg, dish-mark.svg and dish-icon.png from dish-logo.svg")


if __name__ == "__main__":
    main()
