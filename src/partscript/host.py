"""The host: what PartScript builds into. Materials, textures, and anything outside the .parts files.

The language and compiler never look at the file system or a game's content directly; they ask
the host. The default Host is self-contained (a material library and textures from a texture
provider). A game embeds PartScript by subclassing it: its own materials and texture provider,
its existing assets for `use`, its prefix for asset ids, its base kit for buildings.
"""

from __future__ import annotations

from kitlib.geom import Atlas, Mat, Part

from .textures import BasicProvider, TextureProvider, TextureStore


class Host:
	def __init__(self, provider: TextureProvider | None = None, prefix: str = "", cache_dir=None):
		self.provider = provider or BasicProvider()
		self.prefix = prefix  # asset ids are <prefix>_<name> when set
		self.textures = TextureStore(self.provider, cache_dir)
		self.materials: dict[str, Mat] = {}
		for key, (mat, recipe) in self.provider.library().items():
			self.add_material(key, mat, recipe)

	# ------------------------------------------------------------ names
	def asset_id(self, name: str) -> str:
		"""The asset id of a prop name: <prefix>_<name>, or the name when there is no prefix."""
		if not self.prefix or name.startswith(self.prefix + "_"):
			return name
		return f"{self.prefix}_{name}"

	def bare(self, asset_id: str) -> str:
		return asset_id.removeprefix(self.prefix + "_") if self.prefix else asset_id

	def placed_id(self, asset_id: str, own: bool) -> str:
		"""The id a building's placement names for a piece (own: a prop of these files)."""
		return asset_id

	# ------------------------------------------------------------ materials and textures
	def add_material(self, key: str, mat: Mat, recipe: tuple | None = None) -> None:
		"""Registers a material; recipe (see textures.TextureStore) makes its texture when asked."""
		self.materials[key] = mat
		if recipe is not None:
			self.textures.add(mat.texture, recipe)

	def find_material(self, value: str) -> str | None:
		"""The material key a word in a .parts file names: <prefix>_<word>, then the word."""
		for key in (self.asset_id(value) if self.prefix else value, value):
			if key in self.materials:
				return key
		return None

	def known_materials(self) -> set | None:
		"""Material words the check accepts (None: accept any; they fail at build)."""
		return set(self.materials)

	def atlases(self) -> list[Atlas]:
		"""Decal sheets for the trim statement, first match wins."""
		return list(self.provider.atlases())

	# ------------------------------------------------------------ the world outside the .parts files
	def known_assets(self) -> dict:
		"""asset id -> {"triangles": n, ...} for built assets `use` may name (check counts them)."""
		return {}

	def native_props(self) -> set:
		"""Prop names the host defines outside PartScript (a .parts prop of that name needs replace=1)."""
		return set()

	def foreign_parts(self, name: str) -> list[Part] | None:
		"""Parts of an asset outside the program that `use` and buildings may name, or None."""
		return None

	def base_kit(self) -> dict:
		"""Wall sets and pieces a kit can start from (kit NAME walls=SET): {"walls": {...}, "pieces": {...}}."""
		return {}
