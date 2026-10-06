"""Surfaces: what is under a point (for things that settle onto the ground) and points spread over faces
(for things that grow on them: moss on tops, ivy up walls, rivets on a hull).

SurfaceIndex buckets the up-facing triangles of a set of faces on a grid in plan, so asking for the
surface under a point looks at a handful of triangles. spread() picks points over faces by area, with
their normals, kept to the faces that face a given way.
"""

from __future__ import annotations

import math
import random


def _triangles(points: list) -> list:
	"""A polygon's fan of triangles."""
	return [(points[0], points[k], points[k + 1]) for k in range(1, len(points) - 1)]


def _normal(a, b, c) -> tuple:
	ux, uy, uz = b[0] - a[0], b[1] - a[1], b[2] - a[2]
	vx, vy, vz = c[0] - a[0], c[1] - a[1], c[2] - a[2]
	n = (uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx)
	length = math.sqrt(n[0] * n[0] + n[1] * n[1] + n[2] * n[2])
	return (n[0] / length, n[1] / length, n[2] / length, length / 2) if length > 1e-12 else (0.0, 0.0, 0.0, 0.0)


class SurfaceIndex:
	"""The up-facing triangles of some faces, found by where they lie in plan."""

	def __init__(self, polygons: list = (), cell: float = 0.5):
		self.cell = cell
		self.buckets: dict = {}
		for points in polygons:
			self.add(points)

	def add(self, points: list) -> None:
		for a, b, c in _triangles(points):
			nx, ny, nz, area = _normal(a, b, c)
			if nz < 0.15 or area <= 0:
				continue  # walls and undersides hold nothing up
			x0, x1 = min(a[0], b[0], c[0]), max(a[0], b[0], c[0])
			y0, y1 = min(a[1], b[1], c[1]), max(a[1], b[1], c[1])
			tri = ((a[0], a[1], a[2]), (b[0], b[1], b[2]), (c[0], c[1], c[2]), (nx, ny, nz))
			for i in range(math.floor(x0 / self.cell), math.floor(x1 / self.cell) + 1):
				for j in range(math.floor(y0 / self.cell), math.floor(y1 / self.cell) + 1):
					self.buckets.setdefault((i, j), []).append(tri)

	def heights(self, x: float, y: float) -> list:
		"""Every up-facing surface over (x, y): [(z, normal)], highest first."""
		out = []
		for a, b, c, n in self.buckets.get((math.floor(x / self.cell), math.floor(y / self.cell)), ()):
			d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1])
			if abs(d) < 1e-12:
				continue
			l1 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / d
			l2 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / d
			l3 = 1.0 - l1 - l2
			if min(l1, l2, l3) < -1e-6:
				continue
			out.append((l1 * a[2] + l2 * b[2] + l3 * c[2], n))
		return sorted(out, key=lambda hit: -hit[0])

	def below(self, x: float, y: float, z: float) -> tuple | None:
		"""The first surface at or below z over (x, y), else the lowest one above it (a thing set inside a hill
		comes up onto it); None when there is nothing there at all."""
		hits = self.heights(x, y)
		under = [hit for hit in hits if hit[0] <= z + 1e-6]
		if under:
			return under[0]
		return hits[-1] if hits else None


FACING = {"up": lambda n: n[2] > 0.7, "down": lambda n: n[2] < -0.7, "side": lambda n: abs(n[2]) < 0.35,
	"any": lambda n: True, "out": lambda n: True}


def spread(polygons: list, count: int, rng: random.Random, facing: str = "any", apart: float = 0.0) -> list:
	"""count points over the polygons' triangles, by area, on those facing the given way (up, down,
	side, any): [(point, normal)]. Points keep apart where they can."""
	keep = FACING[facing]
	triangles, total = [], 0.0
	for points in polygons:
		for a, b, c in _triangles(points):
			nx, ny, nz, area = _normal(a, b, c)
			if area > 0 and keep((nx, ny, nz)):
				total += area
				triangles.append((total, a, b, c, (nx, ny, nz)))
	if not triangles:
		return []
	out: list = []
	for _ in range(count):
		for _attempt in range(30):
			pick = rng.random() * total
			lo, hi = 0, len(triangles) - 1
			while lo < hi:
				mid = (lo + hi) // 2
				if triangles[mid][0] < pick:
					lo = mid + 1
				else:
					hi = mid
			_, a, b, c, n = triangles[lo]
			r1, r2 = rng.random(), rng.random()
			if r1 + r2 > 1:
				r1, r2 = 1 - r1, 1 - r2
			point = tuple(a[k] + (b[k] - a[k]) * r1 + (c[k] - a[k]) * r2 for k in range(3))
			if all(math.dist(point, q) >= apart for q, _ in out):
				break
		out.append((point, n))
	return out
