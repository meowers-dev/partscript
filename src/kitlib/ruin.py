"""Broken boxes: a wall or a block with chunks knocked out of it, crumbling edges and the rubble that fell.

broken_box() cuts a box into a lattice of chunks, knocks out a share of them (most from the top and the
ends, in noisy bites), lets go of any that no longer reach the ground through the rest, and draws what
is left: the box's own faces where they survive (merged into big quads where nothing nearby broke) and
rough broken faces, in a core material, where chunks came away. The lattice points round the damage
are nudged, so a break reads as crumbled stone rather than steps. It returns the chunks that fell.
"""

from __future__ import annotations

import random

from .noise import rough

# The six sides of a cell: direction, and its quad's corners as lattice offsets, anticlockwise from outside.
_SIDES = (
	((1, 0, 0), ((1, 0, 0), (1, 1, 0), (1, 1, 1), (1, 0, 1))),
	((-1, 0, 0), ((0, 0, 0), (0, 0, 1), (0, 1, 1), (0, 1, 0))),
	((0, 1, 0), ((0, 1, 0), (0, 1, 1), (1, 1, 1), (1, 1, 0))),
	((0, -1, 0), ((0, 0, 0), (1, 0, 0), (1, 0, 1), (0, 0, 1))),
	((0, 0, 1), ((0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1))),
	((0, 0, -1), ((0, 0, 0), (0, 1, 0), (1, 1, 0), (1, 0, 0))),
)


def broken_box(part, size: tuple, material: str, amount: float, chunk: float, seed: str, core: str | None = None,
		skip_bottom: bool = False) -> list:
	"""A box of size centred on the part's origin with amount (0-1) of it knocked out in chunks about
	chunk across. Draws into part; returns the fallen chunks as [(centre, size)] in the same space."""
	core = core or material
	n = [max(1, round(s / chunk)) for s in size]
	cell = [s / k for s, k in zip(size, n)]
	half = [s / 2 for s in size]
	rng = random.Random(f"ruin#{seed}")
	cells = [(i, j, k) for k in range(n[2]) for j in range(n[1]) for i in range(n[0])]

	def centre(i: int, j: int, k: int) -> tuple:
		return (-half[0] + (i + .5) * cell[0], -half[1] + (j + .5) * cell[1], -half[2] + (k + .5) * cell[2])

	# Which chunks go: noisy bites, most likely at the top and toward the ends.
	scale = 1.0 / max(chunk * 4.5, 1e-6)
	scores = {}
	for c in cells:
		x, y, z = centre(*c)
		top = (c[2] + .5) / n[2]
		end = abs((c[0] + .5) / n[0] * 2 - 1)
		scores[c] = rough(x * scale, y * scale * .5, z * scale * .8, seed) * .8 + top * .28 + end * .14 + rng.random() * .1
	order = sorted(cells, key=lambda c: -scores[c])
	gone = set(order[:round(max(0.0, min(1.0, amount)) * len(cells))])
	# Anything no longer joined to the bottom through the rest falls too.
	kept = set(cells) - gone
	reached, todo = set(), [c for c in kept if c[2] == 0]
	while todo:
		c = todo.pop()
		if c in reached:
			continue
		reached.add(c)
		for (dx, dy, dz), _ in _SIDES:
			nb = (c[0] + dx, c[1] + dy, c[2] + dz)
			if nb in kept and nb not in reached:
				todo.append(nb)
	gone |= kept - reached
	kept = reached

	# Lattice points touching a missing chunk are nudged (along the axes they are free to move on), so
	# breaks crumble; points away from the damage stay put and their faces merge.
	def inside(i: int, j: int, k: int) -> bool:
		return 0 <= i < n[0] and 0 <= j < n[1] and 0 <= k < n[2]

	damaged: set = set()
	for (i, j, k) in gone:
		for di in (0, 1):
			for dj in (0, 1):
				for dk in (0, 1):
					damaged.add((i + di, j + dj, k + dk))
	moved: dict = {}

	def point(v: tuple) -> tuple:
		if v in moved:
			return moved[v]
		base = [-half[a] + v[a] * cell[a] for a in range(3)]
		if v in damaged:
			jitter = random.Random(f"ruin#{seed}#{v}")
			for a in range(3):
				if 0 < v[a] < n[a]:
					base[a] += jitter.uniform(-.44, .44) * cell[a]
		moved[v] = tuple(base)
		return moved[v]

	def corners(c: tuple, offsets: tuple) -> list:
		return [point((c[0] + o[0], c[1] + o[1], c[2] + o[2])) for o in offsets]

	for direction, offsets in _SIDES:
		axis = [a for a in range(3) if direction[a]][0]
		if skip_bottom and direction == (0, 0, -1):
			offsets_bottom = True
		else:
			offsets_bottom = False
		# The box's own faces on this side: clean cells merged greedily, the rest one by one.
		other = [a for a in range(3) if a != axis]
		layer = n[axis] - 1 if direction[axis] > 0 else 0
		clean = {}
		for c in kept:
			if c[axis] != layer:
				continue
			quad = [(c[0] + o[0], c[1] + o[1], c[2] + o[2]) for o in offsets]
			if all(v not in damaged for v in quad):
				clean[(c[other[0]], c[other[1]])] = c
			elif not offsets_bottom:
				_quad(part, corners(c, offsets), material)
		if not offsets_bottom:
			for u0, v0, u1, v1 in _merge(set(clean), n[other[0]], n[other[1]]):
				first, last = clean[(u0, v0)], clean[(u1, v1)]
				box_lo = [min(first[a], last[a]) for a in range(3)]
				box_hi = [max(first[a], last[a]) + 1 for a in range(3)]
				quad = []
				for o in offsets:
					v = [box_lo[a] if o[a] == 0 else box_hi[a] for a in range(3)]
					v[axis] = layer + (1 if direction[axis] > 0 else 0)
					quad.append(tuple(-half[a] + v[a] * cell[a] for a in range(3)))
				part.face(quad, material)
		# Broken faces: a kept chunk beside a missing one.
		for c in kept:
			nb = (c[0] + direction[0], c[1] + direction[1], c[2] + direction[2])
			if inside(*nb) and nb in gone:
				_quad(part, corners(c, offsets), core)
	return [(centre(*c), tuple(cell)) for c in sorted(gone)]


def _quad(part, points: list, material: str) -> None:
	"""A quad, as two triangles when nudged corners have bent it out of flat."""
	a, b, c, d = points
	if _flat(points):
		part.face(points, material)
	else:
		part.face([a, b, c], material)
		part.face([a, c, d], material)


def _flat(points: list) -> bool:
	a, b, c, d = points
	u = [b[k] - a[k] for k in range(3)]
	v = [c[k] - a[k] for k in range(3)]
	w = [d[k] - a[k] for k in range(3)]
	n = (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])
	return abs(n[0] * w[0] + n[1] * w[1] + n[2] * w[2]) < 1e-9


def _merge(cells: set, nu: int, nv: int) -> list:
	"""Greedy rectangles covering cells (u, v): [(u0, v0, u1, v1)] inclusive."""
	left = set(cells)
	out = []
	for v in range(nv):
		for u in range(nu):
			if (u, v) not in left:
				continue
			u1 = u
			while (u1 + 1, v) in left:
				u1 += 1
			v1 = v
			while all((x, v1 + 1) in left for x in range(u, u1 + 1)):
				v1 += 1
			for x in range(u, u1 + 1):
				for y in range(v, v1 + 1):
					left.discard((x, y))
			out.append((u, v, u1, v1))
	return out
