"""Generate Atomix .ico from the simplified icon.svg
Usage: python scripts/gen-icon.py

Requires: Pillow (pip install Pillow)
"""

import math
import struct
from pathlib import Path

try:
    from PIL import Image, ImageDraw
except ImportError:
    print("Pillow not installed. Run: pip install Pillow")
    exit(1)

def hexagon_center(size: int) -> list[tuple[float, float]]:
    """Return 6 vertices of a regular hexagon centered in `size` square."""
    cx = cy = size / 2
    r = size * 0.38  # hexagon radius (leave margin)
    points = []
    for i in range(6):
        angle = math.pi / 3 * i - math.pi / 6  # pointy-top
        x = cx + r * math.cos(angle)
        y = cy + r * math.sin(angle)
        points.append((x, y))
    return points

def draw_icon(size: int) -> Image.Image:
    """Draw the Atomix icon (hexagon + nucleus) at given size."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    cx = cy = size / 2
    hex_points = hexagon_center(size)

    # Brand colors
    orange = (232, 93, 58, 255)
    dark_orange = (201, 71, 42, 255)

    # Outer hexagon
    stroke_w = max(2, size // 14)
    draw.polygon(hex_points, outline=orange, width=stroke_w)

    # Nucleus (filled circle)
    nucleus_r = size * 0.13
    draw.ellipse(
        [cx - nucleus_r, cy - nucleus_r, cx + nucleus_r, cy + nucleus_r],
        fill=orange,
    )

    return img


if __name__ == "__main__":
    out_dir = Path(__file__).resolve().parent.parent / "docs"
    sizes = [16, 24, 32, 48, 64, 128, 256]

    # Generate .ico (Windows icon, multi-resolution)
    ico_path = out_dir / "icon.ico"
    images = [draw_icon(s) for s in sizes]
    # Save first image with the rest as frames; ICO format uses append_images
    images[0].save(
        ico_path,
        format="ICO",
        sizes=[(s, s) for s in sizes],
        append_images=images[1:],
    )
    print(f"✓ {ico_path}  ({len(sizes)} sizes)")

    # Generate .png (GitHub avatar, single large)
    png_path = out_dir / "icon.png"
    draw_icon(512).save(png_path, format="PNG")
    print(f"✓ {png_path}  (512×512)")
