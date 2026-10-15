"""Write the shell's placeholder icons.

Not artwork — a build input. `tauri_build::build()` refuses a `tauri.conf.json` whose `bundle.icon` entries do not
exist, so the shell cannot compile until something is at those paths. A generated flat square is honest about being a
placeholder (a hand-drawn logo committed as if it were finished would not be) and it makes the build's failure mode
independent of an image pipeline that does not exist yet.

Run from `src-tauri`:  python scripts/make_icons.py
"""

from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent.parent
ICONS = HERE / "icons"

# The one colour the shell uses, so a placeholder is at least recognisably the product's.
INK = (24, 26, 31, 255)
PAPER = (240, 240, 236, 255)


def square(size: int) -> Image.Image:
    """A rounded paper tile with an ink bar across it — the shell's mark at placeholder fidelity."""
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    margin = max(1, size // 16)
    draw.rounded_rectangle(
        [margin, margin, size - margin - 1, size - margin - 1],
        radius=max(1, size // 8),
        fill=PAPER,
    )
    bar = max(2, size // 5)
    left = size // 5
    top = (size - bar) // 2
    draw.rectangle([left, top, size - left - 1, top + bar - 1], fill=INK)
    return image


def main() -> None:
    ICONS.mkdir(parents=True, exist_ok=True)
    for size in (32, 128, 256):
        square(size).save(ICONS / f"{size}x{size}.png")
    # `.icns` and `.ico` are containers around the PNGs above. Tauri only reads them when packaging for macOS or
    # Windows respectively, and both formats accept an embedded PNG.
    square(256).save(ICONS / "icon.icns", format="ICNS")
    square(128).save(ICONS / "icon.ico", format="ICO", sizes=[(128, 128)])
    for produced in sorted(ICONS.iterdir()):
        print(f"{produced.relative_to(HERE)} ({produced.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
