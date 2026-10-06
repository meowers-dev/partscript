"""Textures: where the pixels come from, and a cache so each is made once.

PartScript names textures; a TextureProvider makes them from recipes. A recipe is a tuple whose
first item says what to make:

	("surface", finish, "rrggbb")   a colour material's texture (#rrggbb/finish in a .parts file)
	("sign", spec)                  a sign or label: text, sub line, colours, texture size
	anything else                   the provider's own (its library textures, decal sheets)

The TextureStore asks the provider once per recipe and keeps the PNG, in memory and, given a
cache directory, on disk keyed by (provider id, provider version, recipe), so a rebuild only
makes textures whose recipe changed. BasicProvider is the open default: flat colours with a
little texture per finish, ordered dither and a palette quantize, and a pixel font for signs.
"""

from __future__ import annotations

import hashlib
import json
import struct
import zlib
from pathlib import Path
from typing import Protocol

import numpy as np

from kitlib.geom import Atlas, Mat


class TextureProvider(Protocol):
	id: str
	version: str

	def library(self) -> dict[str, tuple[Mat, tuple]]:
		"""Materials every .parts file can name: key -> (Mat, recipe of its texture)."""

	def atlases(self) -> list[Atlas]:
		"""Decal sheets for the trim statement (their textures come from library())."""

	def make(self, recipe: tuple) -> np.ndarray:
		"""The texture for a recipe: uint8 RGB or RGBA, height x width x channels."""


# ------------------------------------------------------------------ PNG
def encode_png(image: np.ndarray, level: int = 6) -> bytes:
	"""PNG bytes of a uint8 RGB or RGBA image (no dependencies beyond zlib)."""
	image = np.ascontiguousarray(image, dtype=np.uint8)
	height, width, channels = image.shape
	colour_type = {3: 2, 4: 6}[channels]
	rows = np.zeros((height, width * channels + 1), dtype=np.uint8)  # filter byte 0 (none) per row
	rows[:, 1:] = image.reshape(height, width * channels)

	def chunk(kind: bytes, data: bytes) -> bytes:
		return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

	header = struct.pack(">IIBBBBB", width, height, 8, colour_type, 0, 0, 0)
	return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rows.tobytes(), level)) + chunk(b"IEND", b"")


# ------------------------------------------------------------------ the store
class TextureStore:
	"""Texture name -> recipe, and the PNG of each, made once."""

	def __init__(self, provider: TextureProvider, cache_dir: Path | str | None = None):
		self.provider = provider
		self.cache_dir = Path(cache_dir) if cache_dir else None
		self.recipes: dict[str, tuple] = {}
		self._png: dict[str, bytes] = {}
		self.made = 0  # textures made this session (not served from a cache)

	def add(self, name: str, recipe: tuple) -> None:
		if self.recipes.get(name) != recipe:
			self.recipes[name] = recipe
			self._png.pop(name, None)

	def key(self, recipe: tuple) -> str:
		text = json.dumps([self.provider.id, self.provider.version, recipe], sort_keys=True, default=list)
		return hashlib.sha1(text.encode()).hexdigest()

	def png(self, name: str) -> bytes | None:
		"""The PNG of a named texture, or None when nothing knows how to make it."""
		if name in self._png:
			return self._png[name]
		recipe = self.recipes.get(name)
		if recipe is None:
			return None
		path = self.cache_dir / f"{self.key(recipe)}.png" if self.cache_dir else None
		if path is not None and path.exists():
			data = path.read_bytes()
		else:
			data = encode_png(self.provider.make(recipe))
			self.made += 1
			if path is not None:
				path.parent.mkdir(parents=True, exist_ok=True)
				temp = path.with_suffix(".tmp")
				temp.write_bytes(data)
				temp.replace(path)
		self._png[name] = data
		return data

	__call__ = png


# ------------------------------------------------------------------ pixel helpers
_BAYER4 = np.array([[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]], dtype=np.float32) / 16.0 - 0.5


def quantize(rgb: np.ndarray, levels: int = 24, dither: float = 1.0) -> np.ndarray:
	"""Float RGB 0..255 -> uint8 on `levels` steps per channel, ordered (Bayer) dither between them."""
	height, width = rgb.shape[:2]
	step = 255.0 / (levels - 1)
	threshold = np.tile(_BAYER4, (height // 4 + 1, width // 4 + 1))[:height, :width, None] * dither
	out = np.floor(rgb / step + 0.5 + threshold) * step
	return np.clip(out, 0, 255).astype(np.uint8)


def value_noise(size: int, cells: int, seed: int) -> np.ndarray:
	"""Smooth tileable noise, 0..1, size x size, `cells` lattice cells across."""
	rng = np.random.default_rng(seed)
	lattice = rng.random((cells, cells), dtype=np.float32)
	coords = np.arange(size, dtype=np.float32) * cells / size
	i0 = coords.astype(np.int32)
	t = coords - i0
	t = t * t * (3 - 2 * t)
	i1 = (i0 + 1) % cells
	rows0, rows1 = lattice[i0], lattice[i1]
	top = rows0[:, i0] * (1 - t)[None, :] + rows0[:, i1] * t[None, :]
	bottom = rows1[:, i0] * (1 - t)[None, :] + rows1[:, i1] * t[None, :]
	return top * (1 - t)[:, None] + bottom * t[:, None]


def fbm(size: int, cells: int, octaves: int, seed: int) -> np.ndarray:
	total = np.zeros((size, size), dtype=np.float32)
	weight, norm = 1.0, 0.0
	for octave in range(octaves):
		total += value_noise(size, min(size, cells << octave), seed + octave * 7919) * weight
		norm += weight
		weight *= 0.5
	return total / norm


def _hex(hex_: str) -> np.ndarray:
	return np.array([int(hex_[i:i + 2], 16) for i in (0, 2, 4)], dtype=np.float32)


# ------------------------------------------------------------------ pixel font (5 x 7)
_FONT = {
	"A": "01110 10001 10001 11111 10001 10001 10001", "B": "11110 10001 10001 11110 10001 10001 11110",
	"C": "01110 10001 10000 10000 10000 10001 01110", "D": "11110 10001 10001 10001 10001 10001 11110",
	"E": "11111 10000 10000 11110 10000 10000 11111", "F": "11111 10000 10000 11110 10000 10000 10000",
	"G": "01110 10001 10000 10111 10001 10001 01111", "H": "10001 10001 10001 11111 10001 10001 10001",
	"I": "01110 00100 00100 00100 00100 00100 01110", "J": "00111 00010 00010 00010 00010 10010 01100",
	"K": "10001 10010 10100 11000 10100 10010 10001", "L": "10000 10000 10000 10000 10000 10000 11111",
	"M": "10001 11011 10101 10101 10001 10001 10001", "N": "10001 10001 11001 10101 10011 10001 10001",
	"O": "01110 10001 10001 10001 10001 10001 01110", "P": "11110 10001 10001 11110 10000 10000 10000",
	"Q": "01110 10001 10001 10001 10101 10010 01101", "R": "11110 10001 10001 11110 10100 10010 10001",
	"S": "01111 10000 10000 01110 00001 00001 11110", "T": "11111 00100 00100 00100 00100 00100 00100",
	"U": "10001 10001 10001 10001 10001 10001 01110", "V": "10001 10001 10001 10001 10001 01010 00100",
	"W": "10001 10001 10001 10101 10101 10101 01010", "X": "10001 10001 01010 00100 01010 10001 10001",
	"Y": "10001 10001 01010 00100 00100 00100 00100", "Z": "11111 00001 00010 00100 01000 10000 11111",
	"0": "01110 10001 10011 10101 11001 10001 01110", "1": "00100 01100 00100 00100 00100 00100 01110",
	"2": "01110 10001 00001 00010 00100 01000 11111", "3": "11111 00010 00100 00010 00001 10001 01110",
	"4": "00010 00110 01010 10010 11111 00010 00010", "5": "11111 10000 11110 00001 00001 10001 01110",
	"6": "00110 01000 10000 11110 10001 10001 01110", "7": "11111 00001 00010 00100 01000 01000 01000",
	"8": "01110 10001 10001 01110 10001 10001 01110", "9": "01110 10001 10001 01111 00001 00010 01100",
	" ": "00000 00000 00000 00000 00000 00000 00000", ".": "00000 00000 00000 00000 00000 01100 01100",
	",": "00000 00000 00000 00000 01100 00100 01000", "!": "00100 00100 00100 00100 00100 00000 00100",
	"?": "01110 10001 00001 00010 00100 00000 00100", "'": "01100 00100 01000 00000 00000 00000 00000",
	"-": "00000 00000 00000 11111 00000 00000 00000", "+": "00000 00100 00100 11111 00100 00100 00000",
	"&": "01100 10010 10100 01000 10101 10010 01101", "/": "00001 00010 00010 00100 01000 01000 10000",
	":": "00000 01100 01100 00000 01100 01100 00000", "#": "01010 01010 11111 01010 11111 01010 01010",
	"(": "00010 00100 01000 01000 01000 00100 00010", ")": "01000 00100 00010 00010 00010 00100 01000",
	"%": "11000 11001 00010 00100 01000 10011 00011", "$": "00100 01111 10100 01110 00101 11110 00100",
	"*": "00000 00100 10101 01110 10101 00100 00000", "=": "00000 00000 11111 00000 11111 00000 00000",
	"@": "01110 10001 10111 10101 10111 10000 01111", '"': "01010 01010 00000 00000 00000 00000 00000",
}
_GLYPHS = {ch: np.array([[c == "1" for c in row] for row in rows.split()], dtype=bool) for ch, rows in _FONT.items()}


def text_mask(text: str) -> np.ndarray:
	"""A 7-pixel-high bool mask of text in the pixel font (unknown characters are blanks)."""
	glyphs = [_GLYPHS.get(ch, _GLYPHS["?"] if not ch.isspace() else _GLYPHS[" "]) for ch in text.upper()]
	if not glyphs:
		return np.zeros((7, 1), dtype=bool)
	gap = np.zeros((7, 1), dtype=bool)
	parts = []
	for glyph in glyphs:
		parts += [glyph, gap]
	return np.hstack(parts[:-1])


def _stamp(canvas: np.ndarray, mask: np.ndarray, colour, centre_x: float, top: int, scale: int) -> None:
	big = np.kron(mask, np.ones((scale, scale), dtype=bool))
	height, width = big.shape
	x0 = int(round(centre_x - width / 2))
	y0 = top
	ch, cw = canvas.shape[:2]
	xs, ys = max(0, -x0), max(0, -y0)
	xe, ye = min(width, cw - x0), min(height, ch - y0)
	if xe <= xs or ye <= ys:
		return
	region = canvas[y0 + ys:y0 + ye, x0 + xs:x0 + xe]
	region[big[ys:ye, xs:xe]] = colour


# ------------------------------------------------------------------ the open provider
def _mat(texture: str, tile: float = 2.0, **kwargs) -> Mat:
	return Mat(texture, tile, **kwargs)


# Library materials: key -> (finish, colour, Mat options). Textures are made from (finish, colour).
_LIBRARY = {
	"wood": ("wood", "8a5a32", {"tile": 1.6}),
	"crate_wood": ("planks", "a07a48", {"tile": 1.0}),
	"steel_dark": ("metal", "3a3d40", {"tile": 2.4, "roughness": 0.6}),
	"steel_grey": ("metal", "7c8084", {"tile": 2.4}),
	"steel_white": ("metal", "d8d8d0", {"tile": 2.4}),
	"steel_blue": ("metal", "2f5a8a", {"tile": 2.4}),
	"steel_red": ("metal", "9a2a22", {"tile": 2.4}),
	"steel_teal": ("metal", "2a7a72", {"tile": 2.4}),
	"steel_olive": ("metal", "5c6236", {"tile": 2.4}),
	"steel_green": ("metal", "3a7a3a", {"tile": 2.4}),
	"steel_yellow": ("metal", "d0a020", {"tile": 2.4}),
	"steel_orange": ("metal", "d06a1a", {"tile": 2.4}),
	"steel_purple": ("metal", "5a3a8a", {"tile": 2.4}),
	"brass": ("metal", "b08a3a", {"tile": 2.4, "roughness": 0.5}),
	"rust": ("rust", "8a4a22", {"tile": 1.5}),
	"rubber": ("rubber", "2a2a2a", {"tile": 2.4}),
	"canvas": ("fabric", "b0a080", {"tile": 1.0}),
	"leather": ("fabric", "5a3a22", {"tile": 0.8}),
	"paper": ("plaster", "e8e4d8", {"tile": 1.0}),
	"concrete": ("concrete", "8c8a84", {"tile": 3.2}),
	"plaster": ("plaster", "c8c0b0", {"tile": 4.0}),
	"brick": ("brick", "8a3a2a", {"tile": 3.0}),
	"stone": ("stone", "8a8478", {"tile": 3.0}),
	"hazard": ("hazard", "e0b020", {"tile": 1.0}),
	"glass": ("glass", "a8c8d8", {"tile": 1.0, "roughness": 0.4, "ao": False, "alpha": 0.3}),
	"glass_opaque": ("glass", "4a6a7a", {"tile": 1.0, "roughness": 0.15, "metallic": 0.4, "ao": False}),
	"lamp_warm": ("glow", "5a4628", {"tile": 1.0, "emission_color": (1.0, 0.78, 0.5), "emission_strength": 1.1, "ao": False}),
	"lamp_cold": ("glow", "3a4450", {"tile": 1.0, "emission_color": (0.8, 0.9, 1.0), "emission_strength": 1.1, "ao": False}),
	"lamp_sodium": ("glow", "3a3d40", {"tile": 1.0, "emission_color": (1.0, 0.55, 0.18), "emission_strength": 1.3, "ao": False}),
	"lamp_red": ("glow", "5a1410", {"tile": 1.0, "emission_color": (1.0, 0.06, 0.03), "emission_strength": 1.2, "ao": False}),
	"lamp_green": ("glow", "1a4a3a", {"tile": 1.0, "emission_color": (0.3, 1.0, 0.45), "emission_strength": 1.1, "ao": False}),
	"lamp_amber": ("glow", "3a3d40", {"tile": 1.0, "emission_color": (1.0, 0.55, 0.1), "emission_strength": 1.2, "ao": False}),
	"screen": ("glass", "1a2a22", {"tile": 1.0, "emission_color": (0.35, 0.95, 0.55), "emission_strength": 1.6, "ao": False}),
	"screen_amber": ("glass", "2a2218", {"tile": 1.0, "emission_color": (1.0, 0.62, 0.2), "emission_strength": 1.6, "ao": False}),
}


class BasicProvider:
	"""The open default: every finish as a small quantized texture, signs in a pixel font, no decal sheets."""

	id = "partscript.basic"
	version = "2"

	def __init__(self, levels: int = 24, size: int = 64):
		self.levels = levels
		self.size = size

	def library(self) -> dict[str, tuple[Mat, tuple]]:
		out = {}
		for key, (finish, colour, opts) in _LIBRARY.items():
			opts = dict(opts)
			out[key] = (_mat(key, opts.pop("tile"), **opts), ("surface", finish, colour))
		return out

	def atlases(self) -> list[Atlas]:
		return []

	def make(self, recipe: tuple) -> np.ndarray:
		kind = recipe[0]
		if kind == "surface":
			return self.surface(recipe[1], recipe[2])
		if kind == "sign":
			return self.sign(recipe[1])
		raise ValueError(f"BasicProvider cannot make {kind!r} textures")

	# -- surfaces
	def surface(self, finish: str, hex_: str) -> np.ndarray:
		size = self.size if finish not in ("glow", "glass") else 16
		seed = int(hex_, 16) % 997
		base = _hex(hex_)
		y, x = np.mgrid[0:size, 0:size].astype(np.float32)
		shade = np.ones((size, size), dtype=np.float32)
		if finish == "paint":
			shade += (fbm(size, 4, 3, seed) - 0.5) * 0.10
		elif finish == "metal":
			streak = np.random.default_rng(seed).random(size, dtype=np.float32)
			shade += (streak[:, None] - 0.5) * 0.08 + (fbm(size, 8, 2, seed) - 0.5) * 0.06
		elif finish == "plastic":
			shade += (fbm(size, 4, 2, seed) - 0.5) * 0.05
		elif finish == "rubber":
			shade += (np.random.default_rng(seed).random((size, size), dtype=np.float32) - 0.5) * 0.10
		elif finish == "fabric":
			shade += np.where((x.astype(int) // 2 + y.astype(int) // 2) % 2 == 0, 0.06, -0.06) + (fbm(size, 4, 2, seed) - 0.5) * 0.08
		elif finish in ("wood", "planks"):
			warp = fbm(size, 4, 3, seed) * 6.0
			shade += np.sin((y + warp) * 0.9) * 0.08 + (fbm(size, 16, 2, seed + 1) - 0.5) * 0.08
			if finish == "planks":
				shade -= np.where(y.astype(int) % (size // 4) == 0, 0.35, 0.0)
		elif finish in ("plaster", "concrete"):
			shade += (fbm(size, 4, 4, seed) - 0.5) * (0.10 if finish == "plaster" else 0.16)
			if finish == "concrete":
				specks = np.random.default_rng(seed).random((size, size)) < 0.03
				shade -= specks * 0.18
		elif finish in ("stone", "brick"):
			rows = size // (8 if finish == "brick" else 4)
			course = (y.astype(int) // rows)
			width = rows * (2 if finish == "brick" else 1)
			offset = np.where(course % 2 == 1, width // 2, 0)
			column = (x.astype(int) + offset) // width
			jitter = np.random.default_rng(seed).random((size, size), dtype=np.float32)
			tint = jitter[course % size, column % size]
			shade += (tint - 0.5) * 0.16 + (fbm(size, 8, 2, seed) - 0.5) * 0.10
			mortar = (y.astype(int) % rows == 0) | ((x.astype(int) + offset) % width == 0)
			shade = np.where(mortar, 0.55 if finish == "brick" else 0.7, shade)
		elif finish == "rust":
			shade += (fbm(size, 4, 4, seed) - 0.5) * 0.35
		elif finish == "hazard":
			stripe = ((x + y).astype(int) // (size // 4)) % 2 == 0
			rgb = np.where(stripe[..., None], base, np.array([28, 28, 28], dtype=np.float32))
			return quantize(rgb * (1 + (fbm(size, 4, 2, seed) - 0.5)[..., None] * 0.08), self.levels)
		elif finish == "glass":
			shade += np.where(np.abs(x - y) < 2, 0.12, 0.0)
		rgb = base[None, None, :] * shade[..., None]
		return quantize(rgb, self.levels)

	# -- signs and labels
	def sign(self, spec: dict) -> np.ndarray:
		"""A sign or label: its text, then its sub line smaller, word-wrapped and centred with a margin
		at the biggest pixel size that fits. When even the smallest does not fit the texture's size,
		it is drawn at 2x or 4x the resolution (to 256 px, same shape): the panel's UVs do not change."""
		base_w, base_h = spec["tex"]
		text, sub = spec.get("text", ""), spec.get("sub", "")
		for mult in (1, 2, 4):
			width, height = base_w * mult, base_h * mult
			if mult > 1 and max(width, height) > 256:
				break
			layout = sign_layout(text, sub, width, height)
			if layout is not None:
				break
		else:
			width, height = base_w * mult, base_h * mult
		if layout is None:  # nothing fits: the smallest text, clipped at the edges
			width, height = min(width, 256), min(height, 256)
			layout = sign_layout(text, sub, width, height, force=True)
		bg, fg = np.array(spec["bg"], dtype=np.float32), np.array(spec["fg"], dtype=np.float32)
		canvas = np.empty((height, width, 3), dtype=np.float32)
		canvas[:] = bg
		canvas[[0, -1], :] = bg * 0.6
		canvas[:, [0, -1]] = bg * 0.6
		for mask, top, scale, dim in layout:
			_stamp(canvas, mask, fg * dim, width / 2, top, scale)
		return quantize(canvas, self.levels, dither=0.0)


def _wrap(text: str, scale: int, room: int) -> list[str] | None:
	"""text in lines no wider than room pixels at scale (None when a single word is wider)."""
	lines: list[str] = []
	for word in text.split():
		if (6 * len(word) - 1) * scale > room:
			return None
		if lines and (6 * (len(lines[-1]) + 1 + len(word)) - 1) * scale <= room:
			lines[-1] += " " + word
		else:
			lines.append(word)
	return lines


def sign_layout(text: str, sub: str, width: int, height: int, force: bool = False) -> list | None:
	"""[(mask, top, scale, tone)] placing text and its sub line on a width x height label: the largest
	scale whose wrapped lines fit inside the margin, the sub line at about 60 % of it. None when
	nothing fits (force: scale 1, lines cut at the edges)."""
	for scale in range(8, 0, -1):
		sub_scale = max(1, (scale * 3 + 2) // 5)
		pad = 2 + max(scale // 2, min(width, height) // 16)
		room_w, room_h = width - 2 * pad, height - 2 * pad
		main = _wrap(text, scale, room_w)
		subs = _wrap(sub, sub_scale, room_w) if sub else []
		if force and (main is None or subs is None):
			main, subs = [text], [sub] if sub else []
		if main is None or subs is None:
			continue
		rows = [(line, scale, 1.0) for line in main] + [(line, sub_scale, 0.85) for line in subs]
		heights = [7 * s for _, s, _ in rows]
		gaps = [s + 1 for _, s, _ in rows[1:]]
		if main and subs:
			gaps[len(main) - 1] = 2 * sub_scale
		block = sum(heights) + sum(gaps)
		if block > room_h and not force:
			continue
		top = (height - block) // 2
		out = []
		for k, (line, s, tone) in enumerate(rows):
			out.append((text_mask(line), top, s, tone))
			top += heights[k] + (gaps[k] if k < len(gaps) else 0)
		return out
	return None
