from __future__ import annotations

import struct
import zlib

import numpy as np

from partscript.textures import BasicProvider, TextureStore, encode_png, text_mask


def decode_png(data: bytes) -> np.ndarray:
	assert data[:8] == b"\x89PNG\r\n\x1a\n"
	pos, idat, width = 8, b"", 0
	while pos < len(data):
		length, kind = struct.unpack_from(">I4s", data, pos)
		body = data[pos + 8:pos + 8 + length]
		if kind == b"IHDR":
			width, height, _, colour, _, _, _ = struct.unpack(">IIBBBBB", body)
			channels = {2: 3, 6: 4}[colour]
		elif kind == b"IDAT":
			idat += body
		pos += 12 + length
	rows = np.frombuffer(zlib.decompress(idat), dtype=np.uint8).reshape(height, width * channels + 1)
	assert (rows[:, 0] == 0).all()
	return rows[:, 1:].reshape(height, width, channels)


def test_png_round_trip() -> None:
	image = np.random.default_rng(1).integers(0, 255, (16, 32, 3), dtype=np.uint8)
	assert (decode_png(encode_png(image)) == image).all()
	rgba = np.random.default_rng(2).integers(0, 255, (8, 8, 4), dtype=np.uint8)
	assert (decode_png(encode_png(rgba)) == rgba).all()


def test_every_finish_makes_a_power_of_two_texture() -> None:
	provider = BasicProvider()
	for finish in ("paint", "metal", "plastic", "rubber", "fabric", "wood", "plaster", "concrete", "stone", "glow", "glass", "brick",
			"planks", "rust", "hazard"):
		image = provider.make(("surface", finish, "8a5a32"))
		height, width = image.shape[:2]
		assert image.dtype == np.uint8 and image.shape[2] == 3
		assert width == height and width & (width - 1) == 0, finish


def test_signs_draw_their_text() -> None:
	spec = {"text": "HI", "sub": "", "bg": (0, 0, 0), "fg": (255, 255, 255), "lit": 1.1, "tex": (64, 32)}
	image = BasicProvider().make(("sign", spec))
	assert image.shape == (32, 64, 3)
	lit = image.max(axis=2) > 128
	from partscript.textures import sign_layout
	(_, _, scale, _), = sign_layout("HI", "", 64, 32)
	assert scale == 3  # the biggest that fits inside the margin
	assert lit.sum() == text_mask("HI").sum() * scale * scale


def test_store_makes_each_texture_once(tmp_path) -> None:
	store = TextureStore(BasicProvider(), tmp_path)
	store.add("a", ("surface", "metal", "336699"))
	store.add("b", ("surface", "metal", "336699"))
	assert store("a") == store("b")
	assert store.made == 1
	assert store("missing") is None


def test_sign_text_never_leaves_the_label() -> None:
	"""Every label in the examples, and some awkward ones, lays out inside its margin without clipping."""
	from pathlib import Path

	import partscript as ps
	from partscript.textures import sign_layout

	project = ps.Project.from_paths([Path(__file__).resolve().parents[1] / "examples"])
	for prop in project.props():
		project.build(prop["id"])
	specs = [r[1] for r in project.host.textures.recipes.values() if r[0] == "sign"]
	specs += [{"text": "THE HOPE AND ANCHOR FREE HOUSE", "sub": "EST 1887  REAL ALES  GOOD FOOD", "bg": (0, 0, 0), "fg": (255, 255, 255),
		"lit": 1.0, "tex": (64, 32)}, {"text": "TIMES", "sub": "EVERY 10 MIN", "bg": (0, 0, 0), "fg": (255, 255, 255), "lit": 0, "tex": (64, 64)}]
	provider = BasicProvider()
	for spec in specs:
		image = provider.make(("sign", spec))
		height, width = image.shape[:2]
		layout = sign_layout(spec["text"], spec["sub"], width, height)
		assert layout is not None, spec
		pad = 2
		for mask, top, scale, _ in layout:
			line_width = mask.shape[1] * scale
			assert line_width <= width - 2 * pad and top >= pad and top + 7 * scale <= height - pad, (spec["text"], width, height)


def test_a_crowded_label_is_drawn_at_a_higher_resolution() -> None:
	spec = {"text": "TIMES", "sub": "EVERY 10 MIN", "bg": (0, 0, 0), "fg": (255, 255, 255), "lit": 0, "tex": (32, 32)}
	image = BasicProvider().make(("sign", spec))
	assert image.shape[:2] == (64, 64)
