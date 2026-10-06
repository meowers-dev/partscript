"""Geometry: the part (a mesh being modelled) and every shape PartScript draws.

Pure Python, no Blender. Axes follow Blender: X right, +Y back (north), Z up, metres; a
prop's front faces -Y. The glTF writer (kitlib.gltf) turns that into glTF's Y up / -Z forward.

A ``Part`` collects faces tagged with a material key. UVs default to a box projection in
metres (world-consistent across modular pieces); kitlib.bake later dedupes the faces and bakes
a per-corner tone (grounded darkening + face-facing shade) into vertex colours.
"""

from __future__ import annotations

import itertools
import math
from collections.abc import Mapping
from dataclasses import dataclass

from .maths import Euler, Matrix, Vector


@dataclass
class Mat:
	texture: str
	tile: float = 2.0  # metres per texture repeat for box projection (u, and v unless tile_v is set)
	emission: str = ""  # emission texture name ("" = none)
	emission_strength: float = 0.0
	emission_color: tuple = (0.0, 0.0, 0.0)  # flat emission when no texture
	roughness: float = 1.0
	metallic: float = 0.0
	ao: bool = True  # bake vertex AO
	alpha_clip: bool = False  # texture alpha rounded at 0.5 -> glTF alphaMode MASK
	double_sided: bool = False
	unlit_ao: float = 1.0
	alpha: float = 1.0  # < 1 -> constant Principled alpha, glTF alphaMode BLEND
	tile_v: float = 0.0  # metres per repeat along v (height); 0 = same as tile

	@property
	def tiles(self) -> tuple:
		return self.tile, (self.tile_v or self.tile)


DEFAULT_MAT = Mat("")


@dataclass
class Atlas:
	"""A decal sheet: named cells on a square grid of one texture (the trim statement draws them)."""
	material: str  # material key for its cells
	cells: dict  # cell name -> (column, row, width, height) in grid units, row 0 at the top
	grid: int = 8  # cells across (and down) the sheet
	emissive: frozenset = frozenset()  # cells drawn with emit_material
	emit_material: str = ""


@dataclass
class Face:
	points: list
	material: str
	uv: list | None = None
	shade: float = 1.0
	corner_shade: list | None = None  # optional per-corner tone (Gouraud macro shading)
	group: int | None = None  # faces of one flat surface (a triangulated cap) share one facet tone
	tiled: bool = False  # its UVs are metres over the material's tile, so a part's uv_scale applies to them
	origin: tuple = ()  # the use lines it came through, outermost first: ((file, line), ...)


class Part:
	"""Accumulates faces in a local frame; ``push``/``pop`` nest transforms."""

	def __init__(self, name: str, materials: Mapping[str, Mat] | None = None):
		self.name = name
		self.materials = materials if materials is not None else {}  # key -> Mat, for tiling metric UVs
		self.faces: list[Face] = []
		self._stack = [Matrix.Identity(4)]
		self.smooth = False  # Gouraud (shared normals) instead of flat facets
		self.uv_scale = 1.0  # tiling textures repeat this many times denser (a prop's px= texel density)
		self.ao_height: float | None = None  # overrides the bake's ao_height (a prop's ao=auto)
		# Material slot exported first. Godot's runtime glTF loader leaves vertex-colour-as-albedo
		# off on a mesh's first primitive, so vertex-shaded meshes lead with their least visible one.
		self.lead_material: str | None = None

	def mat(self, key: str) -> Mat:
		return self.materials.get(key, DEFAULT_MAT)

	# ------------------------------------------------------------ transforms
	@property
	def matrix(self) -> Matrix:
		return self._stack[-1]

	def push(self, location=(0, 0, 0), rotation=(0, 0, 0), scale=(1, 1, 1)) -> "Part":
		m = Matrix.Translation(Vector(location)) @ _euler(rotation).to_matrix().to_4x4() @ Matrix.Diagonal(Vector((*scale, 1.0)))
		self._stack.append(self.matrix @ m)
		return self

	def push_matrix(self, m: Matrix) -> "Part":
		self._stack.append(self.matrix @ m)
		return self

	def pop(self) -> "Part":
		self._stack.pop()
		return self

	def xf(self, p) -> Vector:
		return self.matrix @ Vector(p)

	# ------------------------------------------------------------ primitives
	def face(self, points, material: str, uv=None, shade: float = 1.0, corner_shade=None, group=None, tiled: bool = False) -> None:
		self.faces.append(Face([self.xf(p) for p in points], material, uv, shade, corner_shade, group, tiled))

	def box(self, center, size, material: str, rotation=(0, 0, 0), skip=(), shade: float = 1.0,
			taper=(1.0, 1.0), lean=(0.0, 0.0)) -> None:
		"""Axis box; ``skip`` omits faces by name: +x -x +y -y +z -z. ``taper`` scales the top
		face in x and y (0 = a point or ridge) and ``lean`` shifts it, for sloped and trapezoid bodies."""
		self.push(center, rotation)
		hx, hy, hz = (s * 0.5 for s in size)
		c = [(-hx, -hy, -hz), (hx, -hy, -hz), (hx, hy, -hz), (-hx, hy, -hz),
			(-hx, -hy, hz), (hx, -hy, hz), (hx, hy, hz), (-hx, hy, hz)]
		c = [_taper_point(p, hz, taper, lean) for p in c]
		quads = {
			"-z": (0, 3, 2, 1), "+z": (4, 5, 6, 7), "-y": (0, 1, 5, 4),
			"+y": (2, 3, 7, 6), "-x": (3, 0, 4, 7), "+x": (1, 2, 6, 5),
		}
		for key, idx in quads.items():
			if key not in skip:
				self.face([c[i] for i in idx], material, shade=shade)
		self.pop()

	def box_minmax(self, lo, hi, material: str, skip=(), shade: float = 1.0) -> None:
		center = [(a + b) * 0.5 for a, b in zip(lo, hi)]
		size = [abs(b - a) for a, b in zip(lo, hi)]
		self.box(center, size, material, skip=skip, shade=shade)

	def bevel_box(self, center, size, bevel: float, material: str, rotation=(0, 0, 0),
			taper=(1.0, 1.0), lean=(0.0, 0.0)) -> None:
		"""Closed chamfered box: six panels, twelve edge strips, eight corner triangles.

		Nested boxes with skipped faces leave open bevel strips. All faces here share
		the same 24 boundary vertices and have outward winding (44 triangles).
		``taper`` and ``lean`` shape it as they do ``box``.
		"""
		half = [s * 0.5 for s in size]
		b = max(0.0, min(bevel, *(h * 0.9 for h in half)))
		if b == 0:
			self.box(center, size, material, rotation=rotation, taper=taper, lean=lean)
			return
		inner = [h - b for h in half]
		self.push(center, rotation)

		def emit(points) -> None:
			# Orient in local space before face() applies the caller's transform.
			vectors = [Vector(point) for point in points]
			normal = (vectors[1] - vectors[0]).cross(vectors[2] - vectors[0])
			midpoint = sum(vectors, Vector((0, 0, 0))) / len(vectors)
			if normal.dot(midpoint) < 0:
				points.reverse()
			# Taper and lean keep the winding (they only scale and shear by height). The upright
			# chamfers twist when x and y taper differently: those go in as two triangles.
			shaped = [_taper_point(point, half[2], taper, lean) for point in points]
			if len(shaped) == 4 and taper[0] != taper[1]:
				a, b, c, d = (Vector(point) for point in shaped)
				if abs((b - a).cross(c - a).dot(d - a)) > 1e-9:
					group = next(_GROUPS)
					self.face(shaped[:3], material, group=group)
					self.face([shaped[0], shaped[2], shaped[3]], material, group=group)
					return
			self.face(shaped, material)

		for axis in range(3):
			other = [i for i in range(3) if i != axis]
			for sign in (-1, 1):
				points = []
				for u, v in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
					point = [0.0] * 3
					point[axis] = sign * half[axis]
					point[other[0]], point[other[1]] = u * inner[other[0]], v * inner[other[1]]
					points.append(point)
				emit(points)
		for a, c in ((0, 1), (0, 2), (1, 2)):
			free = 3 - a - c
			for sa in (-1, 1):
				for sc in (-1, 1):
					points = []
					for outer_a, sf in ((True, -1), (False, -1), (False, 1), (True, 1)):
						point = [0.0] * 3
						point[a] = sa * (half[a] if outer_a else inner[a])
						point[c] = sc * (inner[c] if outer_a else half[c])
						point[free] = sf * inner[free]
						points.append(point)
					emit(points)
		for sx in (-1, 1):
			for sy in (-1, 1):
				for sz in (-1, 1):
					signs = (sx, sy, sz)
					emit([[signs[i] * (half[i] if i == axis else inner[i]) for i in range(3)] for axis in range(3)])
		self.pop()

	def cylinder(self, center, radius: float, height: float, material: str, sides: int = 10, rotation=(0, 0, 0),
			radius_top: float | None = None, caps: bool = True, cap_material: str | None = None, arc=(0.0, math.tau)) -> None:
		self.push(center, rotation)
		rt = radius if radius_top is None else radius_top
		h = height * 0.5
		full = abs(arc[1] - arc[0] - math.tau) < 1e-6
		steps = sides if full else max(1, sides)
		angles = [arc[0] + (arc[1] - arc[0]) * i / steps for i in range(steps + 1)]
		for i in range(steps):
			a0, a1 = angles[i], angles[i + 1]
			p = [(radius * math.cos(a0), radius * math.sin(a0), -h), (radius * math.cos(a1), radius * math.sin(a1), -h),
				(rt * math.cos(a1), rt * math.sin(a1), h), (rt * math.cos(a0), rt * math.sin(a0), h)]
			u0, u1 = i / steps, (i + 1) / steps
			circumference = max(radius, rt) * (arc[1] - arc[0])
			self.face(p, material, uv=_cyl_uv(u0, u1, circumference, height, self.mat(material)), tiled=True)
		if caps and full:
			top = [(rt * math.cos(a), rt * math.sin(a), h) for a in angles[:-1]]
			bottom = [(radius * math.cos(a), radius * math.sin(a), -h) for a in reversed(angles[:-1])]
			if rt > 0.0005:
				self.face(top, cap_material or material)
			if radius > 0.0005:
				self.face(bottom, cap_material or material)
		self.pop()

	def wrapped_panel(self, center, radius: float, height: float, material: str, sides: int = 12, rotation=(0, 0, 0)) -> None:
		"""A printed sleeve on an opaque cylindrical shell; full normalized UVs, front at u=.5.

		Open ends are intentional: this is a decal, never the structural can body.
		Use the same sides/rotation as the body and a small outward radial offset.
		"""
		if radius <= 0 or height <= 0 or not 3 <= sides <= 48:
			raise ValueError("Wrapped panel needs positive radius/height and 3-48 sides")
		self.push(center, rotation)
		h = height * .5
		# Rotation moves the facets but the wordmark still centres on the prop's -Y front.
		u_offset = -.25 + rotation[2] / 360.0
		for index in range(sides):
			a0, a1 = math.tau * index / sides, math.tau * (index + 1) / sides
			points = [(radius * math.cos(a0), radius * math.sin(a0), -h),
				(radius * math.cos(a1), radius * math.sin(a1), -h),
				(radius * math.cos(a1), radius * math.sin(a1), h),
				(radius * math.cos(a0), radius * math.sin(a0), h)]
			u0, u1 = index / sides + u_offset, (index + 1) / sides + u_offset
			self.face(points, material, uv=[(u0, 0), (u1, 0), (u1, 1), (u0, 1)])
		self.pop()

	def cone(self, center, radius: float, height: float, material: str, sides: int = 8, rotation=(0, 0, 0)) -> None:
		self.cylinder(center, radius, height, material, sides, rotation, radius_top=0.0)

	def extrude(self, center, outline, width: float, material: str, axis: str = "x", taper: float = 1.0,
			rotation=(0, 0, 0)) -> None:
		"""A closed outline pushed ``width`` through, centred on ``center`` along ``axis``.

		The outline is [(u, v), ...] in the plane across the axis: a side profile for x (u = y,
		v = z), a front profile for y (u = x, v = z), a plan for z (u = x, v = y). It may be
		concave (a car's side, an armchair's, a hammer head) and run either way round. ``taper``
		narrows the width toward the outline's highest v (a cabin narrower than its sills).
		4n - 4 triangles for n points.
		"""
		if axis not in ("x", "y", "z"):
			raise ValueError("extrude axis: x, y or z")
		points = [(float(u), float(v)) for u, v in outline]
		if len(points) > 1 and points[0] == points[-1]:
			points.pop()
		if len(points) < 3:
			raise ValueError("an outline needs 3 or more points")
		area = sum(a[0] * b[1] - b[0] * a[1] for a, b in zip(points, points[1:] + points[:1]))
		if abs(area) < 1e-10:
			raise ValueError("the outline encloses no area")
		if area < 0:
			points.reverse()
		if _self_crossing(points):
			raise ValueError("the outline crosses itself")
		triangles = _ear_clip(points)
		low, high = min(v for _, v in points), max(v for _, v in points)

		def place(point, side: int):
			u, v = point
			w = side * width * 0.5 * (1.0 + (taper - 1.0) * ((v - low) / (high - low) if high > low else 0.0))
			return {"x": (w, u, v), "y": (u, w, v), "z": (u, v, w)}[axis]

		# Counter-clockwise (u, v) faces +x and +z, but -y: mirror the winding for a front profile.
		mirrored = axis == "y"
		self.push(center, rotation)
		for a, b in zip(points, points[1:] + points[:1]):
			quad = [place(a, -1), place(b, -1), place(b, 1), place(a, 1)]
			self.face(quad[::-1] if mirrored else quad, material)
		for side in (1, -1):
			group = next(_GROUPS)
			for triangle in triangles:
				corners = [place(points[i], side) for i in triangle]
				self.face(corners[::-1] if (side < 0) != mirrored else corners, material, group=group)
		self.pop()

	def lathe(self, profile, material: str, sides: int = 16, center=(0, 0, 0), arc=(0.0, math.tau), cap=False,
			rotation=(0, 0, 0)) -> None:
		"""Revolve [(radius, z), ...] around local Z. Closed full revolutions share seams."""
		self.push(center, rotation)
		full = abs(arc[1] - arc[0] - math.tau) < 1e-6
		angles = [arc[0] + (arc[1] - arc[0]) * i / sides for i in range(sides + 1)]
		length = 0.0
		lengths = [0.0]
		for (r0, z0), (r1, z1) in zip(profile, profile[1:]):
			length += math.hypot(r1 - r0, z1 - z0)
			lengths.append(length)
		tile, tile_v = self.mat(material).tiles
		for j in range(len(profile) - 1):
			(r0, z0), (r1, z1) = profile[j], profile[j + 1]
			for i in range(sides):
				a0, a1 = angles[i], angles[i + 1]
				p = [(r0 * math.cos(a0), r0 * math.sin(a0), z0), (r0 * math.cos(a1), r0 * math.sin(a1), z0),
					(r1 * math.cos(a1), r1 * math.sin(a1), z1), (r1 * math.cos(a0), r1 * math.sin(a0), z1)]
				circ = max(r0, r1, 0.01) * (arc[1] - arc[0])
				u0 = i / sides * circ / tile
				u1 = (i + 1) / sides * circ / tile
				v0 = lengths[j] / tile_v
				v1 = lengths[j + 1] / tile_v
				self.face(p, material, uv=[(u0, v0), (u1, v0), (u1, v1), (u0, v1)], tiled=True)
		if cap and full:
			r, z = profile[-1]
			if r > 0.001:
				self.face([(r * math.cos(a), r * math.sin(a), z) for a in angles[:-1]], material)
		self.pop()

	def sweep(self, profile, path, material: str, closed_path: bool = False, closed_profile: bool = True,
			up=(0, 0, 1), twist: float = 0.0, scales=None) -> None:
		"""Extrude a 2D profile [(x, y)] along a 3D path. Profile x = side, y = up."""
		frames = []
		count = len(path)
		up_v = Vector(up)
		for i, p in enumerate(path):
			p = Vector(p)
			if closed_path:
				nxt = Vector(path[(i + 1) % count])
				prv = Vector(path[(i - 1) % count])
			else:
				nxt = Vector(path[min(i + 1, count - 1)])
				prv = Vector(path[max(i - 1, 0)])
			tangent = (nxt - prv).normalized()
			side = tangent.cross(up_v)
			if side.length < 1e-5:
				side = tangent.cross(Vector((1, 0, 0)))
			side.normalize()
			normal = side.cross(tangent).normalized()
			angle = twist * i / max(1, count - 1)
			if angle:
				rot = Matrix.Rotation(angle, 3, tangent)
				side = rot @ side
				normal = rot @ normal
			frames.append((p, side, normal))
		rings = []
		for i, (p, side, normal) in enumerate(frames):
			s = scales[i] if scales else 1.0
			rings.append([p + side * (x * s) + normal * (y * s) for x, y in profile])
		segments = count if closed_path else count - 1
		plen = len(profile)
		edges = plen if closed_profile else plen - 1
		tile = self.mat(material).tile
		dist = [0.0]
		for i in range(1, count + (1 if closed_path else 0)):
			dist.append(dist[-1] + (Vector(path[i % count]) - Vector(path[i - 1])).length)
		prof_len = [0.0]
		for k in range(1, plen + 1):
			a = Vector(profile[k % plen])
			b = Vector(profile[k - 1])
			prof_len.append(prof_len[-1] + (a - b).length)
		for i in range(segments):
			ra = rings[i]
			rb = rings[(i + 1) % count]
			for k in range(edges):
				k1 = (k + 1) % plen
				pts = [ra[k], rb[k], rb[k1], ra[k1]]
				uv = [(dist[i] / tile, prof_len[k] / tile), (dist[i + 1] / tile, prof_len[k] / tile),
					(dist[i + 1] / tile, prof_len[k + 1] / tile), (dist[i] / tile, prof_len[k + 1] / tile)]
				self.face([tuple(q) for q in pts], material, uv=uv, tiled=True)
		if not closed_path and closed_profile:
			self.face([tuple(q) for q in reversed(rings[0])], material)
			self.face([tuple(q) for q in rings[-1]], material)

	def panel(self, center, width: float, height: float, material: str, rotation=(0, 0, 0), uv=None,
			double: bool = False) -> None:
		"""Vertical quad facing local -Y (Godot +Z) with 0..1 UVs unless given."""
		self.push(center, rotation)
		w, h = width * 0.5, height * 0.5
		pts = [(-w, 0, -h), (w, 0, -h), (w, 0, h), (-w, 0, h)]
		uv = uv or [(0, 0), (1, 0), (1, 1), (0, 1)]
		self.face(pts, material, uv=uv)
		if double:
			# Nudge the back face 1 mm so the bake's vertex-set dedupe keeps it.
			back = [(x, 0.001, z) for x, _, z in reversed(pts)]
			self.face(back, material, uv=list(reversed(uv)))
		self.pop()

	def trim(self, center, width: float, height: float, cell: str, atlases, rotation=(0, 0, 0), material: str | None = None,
			mirror: bool = False) -> None:
		"""Decal: a panel() facing local -Y with UVs from the first of ``atlases`` (kitlib.Atlas) that
		names the cell. Its emissive cells use the atlas's emit material unless one is given."""
		atlas = next((a for a in atlases if cell in a.cells), None)
		if atlas is None:
			raise ValueError(f"no decal sheet has a cell {cell!r}")
		col, row, w, h = atlas.cells[cell]
		u0, u1 = col / atlas.grid, (col + w) / atlas.grid
		v1, v0 = 1 - row / atlas.grid, 1 - (row + h) / atlas.grid
		if mirror:
			u0, u1 = u1, u0
		mat = material or (atlas.emit_material if cell in atlas.emissive and atlas.emit_material else atlas.material)
		self.panel(center, width, height, mat, rotation=rotation, uv=[(u0, v0), (u1, v0), (u1, v1), (u0, v1)])

	def triangle_count(self) -> int:
		return sum(len(f.points) - 2 for f in self.faces)

	def bounds(self):
		xs = [p.x for f in self.faces for p in f.points]
		ys = [p.y for f in self.faces for p in f.points]
		zs = [p.z for f in self.faces for p in f.points]
		return (min(xs), min(ys), min(zs)), (max(xs), max(ys), max(zs))

def _euler(rotation):
	return Euler(rotation, "XYZ")


_GROUPS = itertools.count(1)


def _taper_point(point, half_height: float, taper, lean) -> tuple:
	"""A local point scaled and shifted by its height: untouched at the bottom, by taper and lean at the top."""
	if half_height <= 0 or (tuple(taper) == (1.0, 1.0) and tuple(lean) == (0.0, 0.0)):
		return tuple(point)
	t = (point[2] + half_height) / (2.0 * half_height)
	return (point[0] * (1.0 + (taper[0] - 1.0) * t) + lean[0] * t, point[1] * (1.0 + (taper[1] - 1.0) * t) + lean[1] * t, point[2])


def _self_crossing(points: list) -> bool:
	"""True when two edges of a closed outline that do not share a corner cross."""
	def side(o, a, b) -> float:
		return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])

	count = len(points)
	edges = [(points[i], points[(i + 1) % count]) for i in range(count)]
	for i in range(count):
		for j in range(i + 2, count):
			if i == 0 and j == count - 1:
				continue
			(a, b), (c, d) = edges[i], edges[j]
			if side(a, b, c) * side(a, b, d) < -1e-12 and side(c, d, a) * side(c, d, b) < -1e-12:
				return True
	return False


def _ear_clip(points: list) -> list[tuple]:
	"""Index triples covering a simple counter-clockwise polygon, concave or not."""
	def cross(o, a, b) -> float:
		return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])

	def inside(q, a, b, c) -> bool:
		return cross(a, b, q) >= -1e-12 and cross(b, c, q) >= -1e-12 and cross(c, a, q) >= -1e-12

	left = list(range(len(points)))
	out: list[tuple] = []
	while len(left) > 3:
		count = len(left)
		for k in range(count):
			i0, i1, i2 = left[k - 1], left[k], left[(k + 1) % count]
			a, b, c = points[i0], points[i1], points[i2]
			if cross(a, b, c) <= 1e-12:
				continue
			if any(inside(points[j], a, b, c) for j in left if j not in (i0, i1, i2) and points[j] not in (a, b, c)):
				continue
			out.append((i0, i1, i2))
			left.pop(k)
			break
		else:
			# No ear: a straight run of points adds nothing, anything else crosses itself.
			flat = next((k for k in range(count) if abs(cross(points[left[k - 1]], points[left[k]], points[left[(k + 1) % count]])) <= 1e-12), None)
			if flat is None:
				raise ValueError("the outline crosses itself")
			left.pop(flat)
	if len(left) == 3 and abs(cross(*(points[i] for i in left))) > 1e-12:
		out.append(tuple(left))
	return out


def _smooth(a: float, b: float, x: float) -> float:
	t = max(0.0, min(1.0, (x - a) / max(b - a, 1e-6)))
	return t * t * (3 - 2 * t)


def face_uvs(face, coords, normal, mat, scale: float = 1.0):
	"""A face's UVs: its own (scaled when they are metric), else the box projection at the part's density."""
	if face.uv and len(face.uv) == len(coords):
		return [(u * scale, v * scale) for u, v in face.uv] if face.tiled and scale != 1.0 else face.uv
	tile, tile_v = mat.tiles
	return _box_uv(coords, normal, tile / scale, tile_v / scale)


def _box_uv(coords, normal, tile: float, tile_v: float | None = None):
	tv = tile_v or tile
	axis = max(range(3), key=lambda i: abs(normal[i]))
	if axis == 0:
		return [(c.y / tile * (1 if normal.x > 0 else -1), c.z / tv) for c in coords]
	if axis == 1:
		return [(c.x / tile * (-1 if normal.y > 0 else 1), c.z / tv) for c in coords]
	return [(c.x / tile, c.y / tv) for c in coords]


def _cyl_uv(u0, u1, circumference, height, mat: Mat):
	tile, tile_v = mat.tiles
	uu0 = u0 * circumference / tile
	uu1 = u1 * circumference / tile
	vv = height / tile_v
	return [(uu0, 0), (uu1, 0), (uu1, vv), (uu0, vv)]


def geometry_stats(parts: list) -> dict:
	"""What the prop library scores detail from: faces, materials, how much of the surface is
	axis-aligned (a model made only of boxes reads as blocky) and how many distinct face
	directions it has (curves, bevels and slopes add them)."""
	faces = [f for part in parts if isinstance(part, Part) for f in part.faces]
	materials = sorted({f.material for f in faces})
	area_total = 0.0
	area_axis = 0.0
	directions = set()
	for f in faces:
		pts = [Vector(p) for p in f.points]
		if len(pts) < 3:
			continue
		normal = Vector((0.0, 0.0, 0.0))
		for i in range(1, len(pts) - 1):
			normal += (pts[i] - pts[0]).cross(pts[i + 1] - pts[0])
		area = normal.length * 0.5
		if area < 1e-6:
			continue
		n = normal.normalized()
		area_total += area
		if max(abs(n.x), abs(n.y), abs(n.z)) > 0.999:
			area_axis += area
		directions.add((round(n.x, 1), round(n.y, 1), round(n.z, 1)))
	return {"faces": len(faces), "materials": materials, "axis_share": round(area_axis / area_total, 3) if area_total else 1.0,
		"directions": len(directions)}
