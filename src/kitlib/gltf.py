"""A small glTF 2.0 binary (.glb) writer for baked parts: no Blender, no dependencies.

One mesh per part and a primitive per material; corners are not shared between faces (flat
shading); nearest-filtered textures; baked tone in COLOR_0. Positions convert from the authoring
space (Z up, +Y back) to glTF's (Y up, -Z forward).
"""

from __future__ import annotations

import json
import struct
import zlib
from pathlib import Path

from .bake import _smooth_normals

# glTF constants.
_FLOAT, _USHORT, _UINT = 5126, 5123, 5125
_ARRAY_BUFFER, _ELEMENT_ARRAY_BUFFER = 34962, 34963
_NEAREST, _NEAREST_MIPMAP_NEAREST = 9728, 9984


def _colour_word(value: float) -> int:
	"""A baked tone as Blender's exporter writes it. The layer is BYTE_COLOR, which Blender keeps
	as 8-bit sRGB, and the glTF holds that byte back in linear at 16 bits; so tones land on the
	256 values a byte can hold, not on the float that was baked."""
	value = max(0.0, min(1.0, value))
	encoded = 12.92 * value if value <= 0.0031308 else 1.055 * value ** (1.0 / 2.4) - 0.055
	stored = int(255.0 * encoded + 0.5) / 255.0
	linear = stored / 12.92 if stored <= 0.04045 else ((stored + 0.055) / 1.055) ** 2.4
	return int(65535.0 * linear + 0.5)


def _emission(spec) -> tuple:
	"""(emissiveFactor or None, emissiveStrength or None) as Blender's exporter splits them."""
	if spec.emission:
		factor = [spec.emission_strength] * 3
	elif spec.emission_strength > 0:
		factor = [c * spec.emission_strength for c in spec.emission_color]
	else:
		return None, None
	peak = max(factor)
	if peak > 1.0:
		return [c / peak for c in factor], peak
	return factor, None


class _Glb:
	def __init__(self, materials, textures, material_prefix: str = ""):
		self.materials = materials  # key -> kitlib.Mat
		self.textures = textures  # texture name -> PNG bytes, or None when it cannot be made
		self.material_prefix = material_prefix
		self.json: dict = {"asset": {"generator": "partscript", "version": "2.0"}, "scene": 0, "scenes": [{"name": "Scene", "nodes": []}],
			"nodes": [], "materials": [], "meshes": [], "textures": [], "images": [], "samplers": [{"magFilter": _NEAREST, "minFilter": _NEAREST_MIPMAP_NEAREST}],
			"accessors": [], "bufferViews": [], "buffers": []}
		self.blob = bytearray()
		self._materials: dict[str, int] = {}
		self._images: dict[str, int] = {}
		self.missing_textures: list[str] = []

	def view(self, data: bytes, target: int | None = None) -> int:
		while len(self.blob) % 4:
			self.blob.append(0)
		view = {"buffer": 0, "byteLength": len(data), "byteOffset": len(self.blob)}
		if target is not None:
			view["target"] = target
		self.blob += data
		self.json["bufferViews"].append(view)
		return len(self.json["bufferViews"]) - 1

	def accessor(self, values: list, component: int, kind: str, target: int, normalized: bool = False, bounds: bool = False) -> int:
		width = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[kind]
		code = {_FLOAT: "f", _USHORT: "H", _UINT: "I"}[component]
		flat = [v for item in values for v in item] if width > 1 else list(values)
		entry = {"bufferView": self.view(struct.pack(f"<{len(flat)}{code}", *flat), target), "componentType": component,
			"count": len(values), "type": kind}
		if normalized:
			entry["normalized"] = True
		if bounds and values:
			# glTF wants POSITION bounds as the stored 32-bit values.
			as_float = lambda v: struct.unpack("<f", struct.pack("<f", v))[0]  # noqa: E731
			entry["min"] = [as_float(min(item[k] for item in values)) for k in range(width)]
			entry["max"] = [as_float(max(item[k] for item in values)) for k in range(width)]
		self.json["accessors"].append(entry)
		return len(self.json["accessors"]) - 1

	def texture(self, name: str) -> int:
		if name not in self._images:
			data = self.textures(name)
			if data is None:
				self.missing_textures.append(name)
				data = _grey_png()
			self.json["images"].append({"bufferView": self.view(data), "mimeType": "image/png", "name": name.rsplit("/", 1)[-1]})
			self.json["textures"].append({"sampler": 0, "source": len(self.json["images"]) - 1})
			self._images[name] = len(self.json["textures"]) - 1
		return self._images[name]

	def material(self, key: str) -> int:
		if key in self._materials:
			return self._materials[key]
		spec = self.materials[key]
		pbr: dict = {"baseColorTexture": {"index": self.texture(spec.texture)}, "metallicFactor": spec.metallic}
		if spec.roughness != 1.0:
			pbr["roughnessFactor"] = spec.roughness
		entry: dict = {"name": self.material_prefix + key, "pbrMetallicRoughness": pbr}
		factor, strength = _emission(spec)
		if factor is not None:
			entry["emissiveFactor"] = factor
			if spec.emission:
				entry["emissiveTexture"] = {"index": self.texture(spec.emission)}
			if strength is not None:
				entry["extensions"] = {"KHR_materials_emissive_strength": {"emissiveStrength": strength}}
				self.json["extensionsUsed"] = ["KHR_materials_emissive_strength"]
		if spec.alpha_clip:
			entry["alphaMode"] = "MASK"
		elif spec.alpha < 1.0:
			entry["alphaMode"] = "BLEND"
			pbr["baseColorFactor"] = [1.0, 1.0, 1.0, spec.alpha]
		if spec.double_sided:
			entry["doubleSided"] = True
		self.json["materials"].append(entry)
		self._materials[key] = len(self.json["materials"]) - 1
		return self._materials[key]

	def mesh(self, baked: dict, steps: bool) -> int:
		"""One mesh per part, a primitive per material in slot order (the exporter's). Corners are
		not shared between faces, as flat shading wants; a face is a fan from its first corner."""
		smooth = _smooth_normals(baked["polygons"]) if baked["smooth"] else {}
		primitives = []
		for material in baked["materials"]:
			positions, normals, uvs, colours, tags, indices = [], [], [], [], [], []
			for polygon in baked["polygons"]:
				if polygon["face"].material != material:
					continue
				base = len(positions)
				flat = polygon["normal"]
				if not smooth and polygon["folded"]:
					# No one normal fits a folded face; light it by its first triangle's.
					a, b, c = (polygon["coords"][i] for i in polygon["triangles"][0])
					flat = (b - a).cross(c - a).normalized()
				for k, co in enumerate(polygon["coords"]):
					normal = smooth[polygon["ids"][k]] if smooth else flat
					# Blender is Z up with +Y north; glTF and Godot are Y up with -Z north.
					positions.append((co.x, co.z, -co.y))
					normals.append((normal.x, normal.z, -normal.y))
					uvs.append((polygon["uvs"][k][0], 1.0 - polygon["uvs"][k][1]))
					word = _colour_word(polygon["colours"][k])
					colours.append((word, word, word, 65535))
					tags.append((polygon["step"] + 0.5, polygon.get("origin", 0) + 0.5))
				for triangle in polygon["triangles"]:
					indices += [base + corner for corner in triangle]
			if not positions:
				continue
			attributes = {
				"POSITION": self.accessor(positions, _FLOAT, "VEC3", _ARRAY_BUFFER, bounds=True),
				"NORMAL": self.accessor(normals, _FLOAT, "VEC3", _ARRAY_BUFFER),
				"TEXCOORD_0": self.accessor(uvs, _FLOAT, "VEC2", _ARRAY_BUFFER),
				"COLOR_0": self.accessor(colours, _USHORT, "VEC4", _ARRAY_BUFFER, normalized=True),
			}
			if steps:
				attributes["TEXCOORD_1"] = self.accessor(tags, _FLOAT, "VEC2", _ARRAY_BUFFER)
			wide = len(positions) > 65535
			primitives.append({"attributes": attributes, "material": self.material(material),
				"indices": self.accessor(indices, _UINT if wide else _USHORT, "SCALAR", _ELEMENT_ARRAY_BUFFER)})
		self.json["meshes"].append({"name": baked["name"], "primitives": primitives})
		return len(self.json["meshes"]) - 1

	def to_bytes(self) -> bytes:
		while len(self.blob) % 4:
			self.blob.append(0)
		self.json["buffers"] = [{"byteLength": len(self.blob)}]
		document = {key: value for key, value in self.json.items() if value not in ([], {})}
		text = json.dumps(document, separators=(",", ":")).encode()
		text += b" " * (-len(text) % 4)
		return (struct.pack("<4sII", b"glTF", 2, 12 + 8 + len(text) + 8 + len(self.blob)) + struct.pack("<I4s", len(text), b"JSON") + text
			+ struct.pack("<I4s", len(self.blob), b"BIN\x00") + bytes(self.blob))


def _grey_png() -> bytes:
	"""A 2x2 mid-grey PNG standing in for a texture that could not be made."""
	def chunk(kind: bytes, data: bytes) -> bytes:
		return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
	rows = b"".join(b"\x00" + bytes([128, 128, 128] * 2) for _ in range(2))
	return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", 2, 2, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")


def glb_bytes(asset_id: str, baked_parts: list, materials, textures, steps: bool = False, material_prefix: str = "") -> tuple[bytes, list[str]]:
	"""(the .glb, textures it could not find): one mesh node, or a node named after the asset over
	one node per part. baked_parts come from kitlib.bake.bake; textures(name) gives PNG bytes.
	steps adds TEXCOORD_1.x = the source step of each face."""
	glb = _Glb(materials, textures, material_prefix)
	nodes = glb.json["nodes"]
	for baked in baked_parts:
		nodes.append({"mesh": glb.mesh(baked, steps), "name": baked["name"]})
	if len(nodes) == 1:
		nodes[0]["name"] = asset_id
		root = 0
	else:
		nodes.append({"name": asset_id, "children": list(range(len(nodes)))})
		root = len(nodes) - 1
	glb.json["scenes"][0]["nodes"] = [root]
	return glb.to_bytes(), glb.missing_textures


def write_glb(path: Path, asset_id: str, baked_parts: list, materials, textures, steps: bool = False, material_prefix: str = "") -> list[str]:
	"""Writes glb_bytes to path. Returns the textures it could not find."""
	data, missing = glb_bytes(asset_id, baked_parts, materials, textures, steps, material_prefix)
	path.parent.mkdir(parents=True, exist_ok=True)
	path.write_bytes(data)
	return missing
