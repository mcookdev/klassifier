#!/usr/bin/env python3
"""Renders klassifier/assets/klassifier.svg to klassifier.ico (Windows exe icon), klassifier.icns
(macOS app icon) and klassifier.png (window icon). Needs: pip install resvg-py pillow. Run after
editing the SVG."""
import io, pathlib
import resvg_py
from PIL import Image

assets = pathlib.Path(__file__).resolve().parent.parent / "klassifier" / "assets"
svg = (assets / "klassifier.svg").read_text()

def render(size):
    png = bytes(resvg_py.svg_to_bytes(svg_string=svg, width=size, height=size))
    return Image.open(io.BytesIO(png)).convert("RGBA")

render(256).save(assets / "klassifier.png")
render(256).save(assets / "klassifier.ico", sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)])
# Pillow writes every ICNS size from 16 to 1024 (512@2x, for Retina) from the largest image.
render(1024).save(assets / "klassifier.icns")
