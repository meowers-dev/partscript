"""Renders the gallery's pictures (docs/img/*.webp) from the preview, headless.

	python3 -m http.server -d site 8765 &        # the preview, serving the built models
	uv run --with pillow python site/shots.py    # needs chromium on the PATH

Each shot is the contact sheet (sheet.html) showing one model from one direction, then shrunk to
WebP. Change SHOTS to change the gallery; site/build.py must have built the models first.
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "docs" / "img"
SERVER = "http://localhost:8765"

# name: (prop, view direction, zoom, PSX look)
SHOTS = {
	"fairground": ("fairground", ".3,.35,1", 1.5, False),
	"tavern": ("tavern_room", ".55,.6,1", 1.4, False),
	"hill_chapel": ("hill_chapel", ".9,.35,.7", 2.2, False),
	"ruin_stages": ("ruin_stages", ".4,.35,1", 1.5, False),
	"standing_stones": ("standing_stones", ".7,.45,1", 1.6, False),
	"apartment": ("apartment_open", ".55,.4,1", 1.3, False),
	"graveyard": ("graveyard", ".6,.5,1", 1.5, False),
	"writers_desk": ("writers_desk", ".7,.5,1", 1.3, False),
	"kitchen_dresser": ("kitchen_dresser", ".6,.3,1", 1.4, False),
	"oak_tree": ("oak_tree", ".7,.3,1", 1.2, False),
	"fenced_garden": ("fenced_garden", ".6,.6,1", 1.4, False),
	"bicycle": ("bicycle", "1,.3,.4", 1.4, False),
	"railing": ("railing_corners", ".6,.6,1", 1.4, False),
}


def shoot(name: str, prop: str, view: str, zoom: float, psx: bool, size: int = 720) -> Path:
	from PIL import Image
	with tempfile.TemporaryDirectory() as tmp:
		png = Path(tmp) / "shot.png"
		url = f"{SERVER}/sheet.html?only={prop}&cols=1&cell={size}&view={view}&zoom={zoom}" + ("&psx=1" if psx else "")
		subprocess.run(["chromium", "--headless=new", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--hide-scrollbars",
			f"--window-size={size},{size}", "--virtual-time-budget=60000", f"--screenshot={png}", url],
			check=True, capture_output=True, timeout=180)
		image = Image.open(png).convert("RGB").crop((0, 22, size, size))  # the label row off the top
		target = OUT / f"{name}.webp"
		image.save(target, "WEBP", quality=82, method=6)
	return target


def main(names: list[str]) -> None:
	OUT.mkdir(parents=True, exist_ok=True)
	for name, (prop, view, zoom, psx) in SHOTS.items():
		if names and name not in names:
			continue
		target = shoot(name, prop, view, zoom, psx)
		print(f"{target.relative_to(ROOT)}  {target.stat().st_size // 1024} KB")


if __name__ == "__main__":
	main(sys.argv[1:])
