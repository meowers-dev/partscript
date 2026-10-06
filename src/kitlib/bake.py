"""Bake: a Part's faces into a mesh ready to write (deduped corners, UVs, baked vertex tone).

The tone is the house shading: a grounded darkening that fades over ``ao_height`` metres, a
face-facing shade, a small seeded per-face jitter, and any per-corner tone the face carries.
Triangulation and normals follow Blender's, so a model looks the same imported there.
"""

from __future__ import annotations

import math
import random
import zlib

from .geom import _smooth, face_uvs
from .maths import Vector


def _polygon_normal(points: list):
	"""The face normal as Blender computes it: the cross product of two edges for a triangle, of
	the diagonals for a quad, Newell's method beyond; +Z when the face has no area or folds over
	itself (a bow-tie quad's diagonals are parallel)."""
	if len(points) == 4:
		normal = (points[0] - points[2]).cross(points[1] - points[3])
	elif len(points) == 3:
		normal = (points[0] - points[1]).cross(points[1] - points[2])
	else:
		x = y = z = 0.0
		prev = points[-1]
		for curr in points:
			x += (prev.y - curr.y) * (prev.z + curr.z)
			y += (prev.z - curr.z) * (prev.x + curr.x)
			z += (prev.x - curr.x) * (prev.y + curr.y)
			prev = curr
		normal = Vector((x, y, z))
	squared = normal.length_squared
	if squared > 1.0e-35:
		return normal * (1.0 / math.sqrt(squared))
	return Vector((0.0, 0.0, 1.0))


def _triangles(points: list) -> list[tuple]:
	"""Corner indices of a polygon's triangles as Blender tessellates it: a fan from the first
	corner, except a quad that would fold along 0-2 is split along 1-3. (Blender fills polygons
	of five corners and more its own way; they are flat caps here, where a fan shows the same.)"""
	if len(points) == 4:
		d12, d13, d14 = points[1] - points[0], points[2] - points[0], points[3] - points[0]
		if d12.cross(d13).dot(d14.cross(d13)) > 0.0:
			return [(0, 1, 3), (1, 2, 3)]
	return [(0, k, k + 1) for k in range(1, len(points) - 1)]


def bake(part, ao_height: float = 1.4, face_steps: list | None = None) -> dict:
	"""A Part ready to write: duplicate and degenerate faces dropped, materials in slot order, and
	per kept face its corners with UVs and the baked vertex tone. face_steps is the step (source
	statement) of each face in part.faces, for step-tagged traces."""
	if face_steps is None:
		face_steps = [0] * len(part.faces)
	verts: list[tuple] = []
	index: dict[tuple, int] = {}
	polygons: list[dict] = []
	seen: set = set()
	for face, step in zip(part.faces, face_steps, strict=True):
		ids: list[int] = []
		for point in face.points:
			key = (round(point.x, 5), round(point.y, 5), round(point.z, 5))
			if key not in index:
				index[key] = len(verts)
				verts.append(key)
			if not ids or ids[-1] != index[key]:
				ids.append(index[key])
		if len(ids) > 1 and ids[0] == ids[-1]:
			ids.pop()
		signature = tuple(sorted(ids))
		if len(set(ids)) < 3 or len(set(ids)) != len(ids) or signature in seen:
			continue
		seen.add(signature)
		polygons.append({"face": face, "ids": ids, "step": step})
	material_names = sorted({polygon["face"].material for polygon in polygons})
	if part.lead_material in material_names:
		material_names.remove(part.lead_material)
		material_names.insert(0, part.lead_material)
	zmin = min(v[2] for v in verts) if verts else 0.0
	if part.ao_height is not None:
		ao_height = part.ao_height
	rng = random.Random(zlib.crc32(part.name.encode()))
	tones: dict = {}
	for polygon in polygons:
		face = polygon["face"]
		mat = part.mat(face.material)
		coords = [Vector(verts[i]) for i in polygon["ids"]]
		normal = _polygon_normal(coords)
		uvs = face_uvs(face, coords, normal, mat, part.uv_scale)
		if face.group is not None and face.group in tones:
			tone = tones[face.group]
		else:
			tone = face.shade * (1.0 + rng.uniform(-0.04, 0.04))
			if face.group is not None:
				tones[face.group] = tone
		corners = face.corner_shade if face.corner_shade and len(face.corner_shade) == len(coords) else [1.0] * len(coords)
		colours = []
		for co, corner in zip(coords, corners):
			value = 1.0
			if mat.ao:
				ground = 0.62 + 0.38 * _smooth(0.0, ao_height, co.z - zmin) if ao_height > 0 else 1.0
				facing = 0.86 + 0.14 * max(0.0, normal.z) if normal.z > -0.5 else 0.74
				value = max(0.0, min(1.0, ground * facing * tone * corner))
			colours.append(value)
		triangles = _triangles(coords)
		folded = len(coords) == 4 and triangles[0] == (0, 1, 3)
		polygon.update({"coords": coords, "normal": normal, "uvs": [tuple(uv) for uv in uvs], "colours": colours, "triangles": triangles,
			"folded": folded})
	return {"name": part.name, "smooth": bool(part.smooth), "materials": material_names, "polygons": polygons}


def _smooth_normals(polygons: list) -> dict:
	"""Vertex normals for a smooth part: face normals weighted by the corner angle, as Blender does."""
	totals: dict[int, Vector] = {}
	for polygon in polygons:
		coords, ids = polygon["coords"], polygon["ids"]
		for k, vertex in enumerate(ids):
			a = (coords[k - 1] - coords[k]).normalized()
			b = (coords[(k + 1) % len(coords)] - coords[k]).normalized()
			angle = math.acos(max(-1.0, min(1.0, a.dot(b))))
			totals[vertex] = totals.get(vertex, Vector((0.0, 0.0, 0.0))) + polygon["normal"] * angle
	return {vertex: normal.normalized() for vertex, normal in totals.items()}
