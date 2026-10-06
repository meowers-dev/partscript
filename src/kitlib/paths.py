"""Paths: placing things along a line of points, and markers that show where pieces snap together.

path_frames() is what a fence, a hedge or a row of lamp posts needs: where each piece goes along a
polyline, which way it turns to follow it, and how much to stretch it so the pieces fill each straight
exactly. snap_markers() draws snap points (and the joints between chained pieces) as small arrows, so
a model can carry its own connection points for a viewer to show.
"""

from __future__ import annotations

import math
from dataclasses import dataclass

from .geom import Part


@dataclass
class Frame:
	pos: tuple  # x, y, z
	yaw: float  # degrees about Z; 0 = the piece's +X runs along +X
	stretch: float = 1.0  # along the piece's X (fit)
	segment: int = 0  # which straight it is on (or which corner)


def path_frames(points: list, every: float = 0.0, fit: bool = False, corners: bool = False, closed: bool = False,
		joints: bool = False) -> list[Frame]:
	"""Frames along a polyline. corners: one at each point, turned halfway between the straights that
	meet there. every: one each `every` metres, centred in its slot; with fit, each straight gets a
	whole number of slots and the pieces stretch to fill them (a fence panel always meets a post).
	joints (with every): one at each end of those slots instead, where the pieces meet (the posts)."""
	pts = [tuple(float(v) for v in p) + (0.0,) * (3 - len(p)) for p in points]
	if closed and len(pts) > 2 and pts[0] != pts[-1]:
		pts.append(pts[0])
	if len(pts) < 2:
		raise ValueError("a path needs two or more points")
	segments = [(a, b) for a, b in zip(pts, pts[1:])]
	headings = [math.degrees(math.atan2(b[1] - a[1], b[0] - a[0])) for a, b in segments]
	lengths = [math.dist(a, b) for a, b in segments]
	if corners:
		out = []
		count = len(pts) - (1 if closed else 0)
		for k in range(count):
			before = headings[k - 1] if (k > 0 or closed) else None
			after = headings[k] if k < len(headings) else None
			if before is None or after is None:
				yaw = after if after is not None else before
			else:
				yaw = before + ((after - before + 180.0) % 360.0 - 180.0) / 2.0
			out.append(Frame(pts[k], yaw, 1.0, k))
		return out
	if every <= 0:
		raise ValueError("along a path: every=D (a piece each D metres) or corners=1 (one at each point)")
	out = []
	if joints:
		for k, ((a, b), length, yaw) in enumerate(zip(segments, lengths, headings)):
			count = max(1, round(length / every)) if fit else max(1, int(length // every))
			step = length / count if fit else every
			last = count if (k == len(segments) - 1 and not closed) else count - 1
			for n in range(0, last + 1):
				t = n * step / length if length else 0.0
				out.append(Frame(tuple(a[j] + (b[j] - a[j]) * t for j in range(3)), yaw, 1.0, k))
		return out
	if fit:
		for k, ((a, b), length, yaw) in enumerate(zip(segments, lengths, headings)):
			count = max(1, round(length / every))
			step = length / count
			for n in range(count):
				t = (n + 0.5) * step / length if length else 0.0
				out.append(Frame(tuple(a[j] + (b[j] - a[j]) * t for j in range(3)), yaw, step / every, k))
		return out
	total = sum(lengths)
	distance = every / 2.0
	while distance <= total + 1e-9:
		walked = distance
		for k, ((a, b), length, yaw) in enumerate(zip(segments, lengths, headings)):
			if walked <= length + 1e-9 or k == len(segments) - 1:
				t = min(1.0, walked / length) if length else 0.0
				out.append(Frame(tuple(a[j] + (b[j] - a[j]) * t for j in range(3)), yaw, 1.0, k))
				break
			walked -= length
		distance += every
	return out


# A colour for each snap kind, so matching kinds read as matching: kind -> #rrggbb.
_KIND_COLOURS = ["ffd84a", "4ad8ff", "ff6a4a", "8aff6a", "d86aff", "ffffff"]


def kind_colour(kind: str) -> str:
	if kind in ("any", ""):
		return "f0f0f0"
	return _KIND_COLOURS[sum(map(ord, kind)) % (len(_KIND_COLOURS) - 1)]


def snap_markers(snaps: list, material_of, size: float = 0.06) -> Part:
	"""A part with a marker per snap: a small cube at the point and a pointer along its direction.
	snaps are objects with pos, dir and kind; material_of(kind) gives the material key to draw it in."""
	part = Part("snaps")
	for snap in snaps:
		mat = material_of(snap.kind)
		x, y, z = snap.pos
		dx, dy, dz = snap.dir
		part.box((x, y, z), (size, size, size), mat)
		length = size * 4
		# The pointer: a thin box from the point along dir, turned to face it.
		yaw = math.degrees(math.atan2(dy, dx))
		pitch = math.degrees(math.atan2(dz, math.hypot(dx, dy)))
		part.push((x + dx * length / 2, y + dy * length / 2, z + dz * length / 2), (0.0, math.radians(-pitch), math.radians(yaw)))
		part.box((0, 0, 0), (length, size * 0.35, size * 0.35), mat)
		part.box((length / 2, 0, 0), (size * 0.9, size * 0.9, size * 0.9), mat, taper=(0.0, 0.0), rotation=(0, math.radians(90), 0))
		part.pop()
	return part


def smooth_points(points: list, steps: int, closed: bool = False) -> list[tuple]:
	"""A curve through the points (Catmull-Rom): `steps` pieces between each pair, passing through every
	point, so a few points make a cable, a vine or a rail that bends smoothly instead of kinking."""
	pts = [tuple(float(v) for v in p) for p in points]
	if steps < 2 or len(pts) < 3:
		return pts
	count = len(pts)
	segments = count if closed else count - 1
	out = []
	for k in range(segments):
		p0 = pts[(k - 1) % count] if closed or k > 0 else tuple(2 * a - b for a, b in zip(pts[0], pts[1]))
		p1, p2 = pts[k], pts[(k + 1) % count]
		p3 = pts[(k + 2) % count] if closed or k + 2 < count else tuple(2 * a - b for a, b in zip(pts[-1], pts[-2]))
		for n in range(steps):
			t = n / steps
			t2, t3 = t * t, t * t * t
			out.append(tuple(0.5 * (2 * b + (-a + c) * t + (2 * a - 5 * b + 4 * c - d) * t2 + (-a + 3 * b - 3 * c + d) * t3)
				for a, b, c, d in zip(p0, p1, p2, p3)))
	if not closed:
		out.append(pts[-1])
	return out
