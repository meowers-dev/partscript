"""Smooth noise: Perlin gradient noise in 1-3D and a layered (fractal) version, seeded.

noise() wanders smoothly between 0 and 1 (about .5 on average): hills, wear, where moss grows. rough()
adds finer layers of the same on top, for ground and stone that is lumpy at every size. The same seed
and point always give the same value.
"""

from __future__ import annotations

import math
import random

_TABLES: dict = {}


def _table(seed: str) -> list:
	table = _TABLES.get(seed)
	if table is None:
		values = list(range(256))
		random.Random(f"noise#{seed}").shuffle(values)
		table = _TABLES[seed] = values + values
	return table


def _fade(t: float) -> float:
	return t * t * t * (t * (t * 6 - 15) + 10)


def _grad(h: int, x: float, y: float, z: float) -> float:
	h &= 15
	u = x if h < 8 else y
	v = y if h < 4 else (x if h in (12, 14) else z)
	return (u if h & 1 == 0 else -u) + (v if h & 2 == 0 else -v)


def perlin(x: float, y: float = 0.0, z: float = 0.0, seed: str = "") -> float:
	"""Gradient noise about 0 (roughly -1..1), smooth, 1 unit across a bump."""
	p = _table(seed)
	xi, yi, zi = math.floor(x), math.floor(y), math.floor(z)
	xf, yf, zf = x - xi, y - yi, z - zi
	xi, yi, zi = xi & 255, yi & 255, zi & 255
	u, v, w = _fade(xf), _fade(yf), _fade(zf)
	a, b = p[xi] + yi, p[xi + 1] + yi
	aa, ab, ba, bb = p[a] + zi, p[a + 1] + zi, p[b] + zi, p[b + 1] + zi

	def lerp(t: float, lo: float, hi: float) -> float:
		return lo + t * (hi - lo)

	return lerp(w,
		lerp(v, lerp(u, _grad(p[aa], xf, yf, zf), _grad(p[ba], xf - 1, yf, zf)),
			lerp(u, _grad(p[ab], xf, yf - 1, zf), _grad(p[bb], xf - 1, yf - 1, zf))),
		lerp(v, lerp(u, _grad(p[aa + 1], xf, yf, zf - 1), _grad(p[ba + 1], xf - 1, yf, zf - 1)),
			lerp(u, _grad(p[ab + 1], xf, yf - 1, zf - 1), _grad(p[bb + 1], xf - 1, yf - 1, zf - 1))))


def noise(x: float, y: float = 0.0, z: float = 0.0, seed: str = "") -> float:
	"""Smooth noise from 0 to 1."""
	return max(0.0, min(1.0, 0.5 + 0.5 * perlin(x, y, z, seed) * 1.15))


def rough(x: float, y: float = 0.0, z: float = 0.0, seed: str = "", octaves: int = 4) -> float:
	"""Layered noise from 0 to 1: each layer twice as fine and half as strong as the last."""
	total, amplitude, frequency, norm = 0.0, 1.0, 1.0, 0.0
	for k in range(octaves):
		total += amplitude * perlin(x * frequency + k * 17.3, y * frequency + k * 9.1, z * frequency + k * 5.7, seed)
		norm += amplitude
		amplitude *= 0.5
		frequency *= 2.0
	return max(0.0, min(1.0, 0.5 + 0.5 * total / norm * 1.4))
