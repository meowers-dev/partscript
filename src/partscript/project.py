"""A project: .parts files (plus the std parts) for a host, checked, built and written as .glb.

	project = Project.from_paths(["props/"])
	report = project.check()
	result = project.build("fruit_stall")          # parts, steps, warnings, the .glb bytes
	project.write_all("out/")                      # every prop as out/<id>.glb
"""

from __future__ import annotations

import dataclasses
import json
import os
import time
from dataclasses import dataclass, field
from pathlib import Path

from kitlib.bake import bake
from kitlib.geom import Mat
from kitlib.gltf import glb_bytes
from kitlib.paths import kind_colour, snap_markers

from . import building as pb
from .check import check, prop_budget
from .compiler import _bounds, Compiler, _identity, finish_parts
from .host import Host
from .lang import BUILD_OPS, PartScriptError, Program, Stmt, _arg_value, colour_key, colour_material_kwargs, parse, split_top

STD = Path(__file__).resolve().parent / "std.parts"


def load_program(sources: list[tuple[str, str]], std: bool = True, reader=None) -> Program:
	"""A program from (file name, text) pairs, after the std parts unless std is False, and every file
	their import lines bring in (each once; one imported as NAME has its names as NAME.x). reader(path)
	gives [(path, text)] for a file or a folder's .parts files (default: the file system); a path that is
	already one of the sources is taken from them."""
	program = Program()
	if std:
		parse(STD.read_text(), "std.parts", program)
	program.std = set(program.macros)
	texts = {_norm(name): text for name, text in sources}
	loaded: set = set()
	for name, text in sources:
		loaded.add((_norm(name), None))
		parse(text, name, program)
	paths = {name: name for name, _ in sources}  # file label -> its path
	done = 0
	while done < len(program.imports):
		importer, line, path, alias = program.imports[done]
		done += 1
		outer = program.namespaces.get(importer)
		namespace = f"{outer}.{alias}" if outer and alias else (alias or outer)
		target = _norm(os.path.join(os.path.dirname(paths.get(importer, importer)), path))
		try:
			found = _find(target, texts, reader)
		except (OSError, UnicodeDecodeError) as error:
			program.errors.append(PartScriptError(f"import \"{path}\": {error}", importer, line))
			continue
		if not found:
			program.errors.append(PartScriptError(f"import \"{path}\": no such file or folder ({target})", importer, line))
			continue
		for real, text in found:
			key = (_norm(real), namespace)
			if key in loaded:
				continue
			loaded.add(key)
			label = f"{namespace}:{real}" if namespace else real
			paths[label] = real
			if namespace:
				program.namespaces[label] = namespace
			program.imported.add(label)
			parse(text, label, program)
	return program


def _norm(path: str) -> str:
	return os.path.normpath(path).replace(os.sep, "/")


def _find(target: str, texts: dict, reader) -> list:
	"""[(path, text)] an import names: one of the sources, else what reader (or the file system) finds;
	a path without .parts tries it with."""
	for candidate in (target, target + ".parts"):
		if candidate in texts:
			return [(candidate, texts[candidate])]
		inside = sorted(name for name in texts if name.startswith(candidate.rstrip("/") + "/"))
		if inside:
			return [(name, texts[name]) for name in inside]
	if reader is not None:
		return reader(target) or reader(target + ".parts")
	for candidate in (Path(target), Path(target + ".parts")):
		if candidate.is_dir():
			return [(_norm(str(p)), p.read_text()) for p in sorted(candidate.rglob("*.parts"))]
		if candidate.is_file():
			return [(_norm(str(candidate)), candidate.read_text())]
	return []


def parts_files(paths: list[Path | str]) -> list[Path]:
	"""Every .parts file in paths (files, or directories searched recursively), sorted."""
	out: list[Path] = []
	for path in map(Path, paths):
		out += sorted(path.rglob("*.parts")) if path.is_dir() else [path]
	return out


@dataclass
class Built:
	asset_id: str
	parts: list  # kitlib Parts with faces
	steps: list  # per top-level statement: {"stmt", "faces": [(part, first, end)], "note"?}
	snaps: list  # the prop's own snap statements (building.Snap, prop space)
	joints: list = field(default_factory=list)  # where chained pieces meet, each chain's open end, and joined link ends
	links: list = field(default_factory=list)  # link ends nothing joined (open: shown by the snap markers)
	warnings: list = field(default_factory=list)
	baked: list = field(default_factory=list)
	origins: list = field(default_factory=list)  # every face's use chain ((file, line), ...); a face's TEXCOORD_1.y indexes it
	glb: bytes = b""
	seconds: float = 0.0

	@property
	def triangles(self) -> int:
		"""The model's triangles (snap markers not counted)."""
		return sum(len(p["ids"]) - 2 for b in self.baked if b["name"] != "snaps" for p in b["polygons"])


class Tracer(Compiler):
	"""The compiler, noting which faces each top-level statement of a prop adds (its steps)."""

	def __init__(self, program, host):
		super().__init__(program, host)
		self._trace_parts: list | None = None
		self._steps: list = []

	def trace(self, prop, asset_id: str) -> tuple[list, list]:
		if asset_id in self._building:
			raise PartScriptError(f"{asset_id} uses itself (through use)", prop.file, prop.line)
		self._building.add(asset_id)
		outer = (self._trace_parts, self._steps, self._path)
		self._path = ()
		parts = [self.new_part(asset_id)]
		self._trace_parts, self._steps = parts, []
		scope = self._open_links()
		try:
			if prop.kind == "building":
				self.building_plan(prop)
			self._run(prop.body, {**self.program.env_of(prop.file), "__seed__": prop.opts.get("seed", "")}, parts, _identity(), 0, prop)
			steps = self._steps
		finally:
			self._building.discard(asset_id)
			self._trace_parts, self._steps, self._path = outer
			self._last_links = self._close_links(scope, parts)
		if not any(part.faces for part in parts):
			raise PartScriptError(f"{asset_id} has no shapes", prop.file, prop.line)
		return parts, steps

	def builder(self, prop, asset_id: str):
		# Another prop pulled in with `use`: built whole, not traced; its joints stay its own.
		def build():
			saved, self.joints = self.joints, []
			try:
				parts, _ = self.trace(prop, asset_id)
			finally:
				self.joints = saved
			parts = [part for part in parts if part.faces]
			return parts[0] if len(parts) == 1 else parts
		return build

	def _run(self, body, env, parts, frame, depth, prop) -> None:
		if depth != 0 or parts is not self._trace_parts:
			super()._run(body, env, parts, frame, depth, prop)
			return
		env = dict(env)
		for stmt in body:
			if stmt.op == "set":
				for key, value in stmt.opts.items():
					env[key] = _arg_value(value, env)
				continue
			if stmt.op in BUILD_OPS and prop.kind == "building":
				# One step a placed piece: a building goes up wall by wall.
				for placement in self.building_plan(prop).for_stmt(stmt):
					before = {id(part): len(part.faces) for part in parts}
					self.place(placement, parts, frame)
					label = Stmt("use", [placement.piece, placement.role], {}, [], stmt.file, stmt.line)
					self._steps.append({"stmt": label, "note": f"{placement.piece} ({placement.role.replace('wall:', 'wall ')})",
						"faces": [(part, before.get(id(part), 0), len(part.faces)) for part in parts if len(part.faces) > before.get(id(part), 0)]})
				continue
			before = {id(part): len(part.faces) for part in parts}
			super()._run([stmt], env, parts, frame, depth, prop)
			if stmt.name:
				env[stmt.name] = _bounds(stmt.name, parts, before)
			if stmt.op in ("size", "card"):
				continue
			added = [(part, before.get(id(part), 0), len(part.faces)) for part in parts if len(part.faces) > before.get(id(part), 0)]
			self._steps.append({"stmt": stmt, "faces": added})


class Project:
	def __init__(self, sources: list[tuple[str, str]] | None = None, host: Host | None = None, std: bool = True, reader=None):
		self.host = host or Host()
		self.sources = list(sources or [])
		self.program = load_program(self.sources, std, reader)
		self._compiler: Tracer | None = None

	@classmethod
	def from_paths(cls, paths: list[Path | str], host: Host | None = None, std: bool = True) -> "Project":
		return cls([(str(p), p.read_text()) for p in parts_files(paths)], host, std)

	@classmethod
	def from_text(cls, text: str, name: str = "<text>", host: Host | None = None) -> "Project":
		return cls([(name, text)], host)

	# ------------------------------------------------------------ queries
	@property
	def errors(self) -> list[str]:
		return [str(e) for e in self.program.errors]

	def props(self) -> list[dict]:
		return [{"id": self.host.asset_id(p.name), "title": p.title, "subcategory": p.subcategory, "kind": p.kind, "file": p.file,
			"line": p.line, "imported": p.file in self.program.imported} for p in self.program.props]

	def prop(self, name: str):
		for p in self.program.props:
			if name in (p.name, self.host.asset_id(p.name)):
				return p
		raise KeyError(f"no prop {name!r}")

	def uses(self, name: str) -> dict:
		"""What a prop or def is built from, all the way down: {"name", "kind", "file", "line", "count",
		"at", "children"}: at is the line of the use that placed it (0 at the root). kind is prop, def, std (a std part), host (the host's asset) or missing; count is
		how many copies one use line makes (*N, mirrors), when the line says it in plain numbers."""
		def node(target: str, count: int, at: int, stack: tuple, file: str = "") -> dict:
			macro = self.program.macro(target, file)
			prop = None if macro else self.program.find_prop(target, file, lambda p: (p.name, self.host.asset_id(p.name)))
			if macro is not None:
				kind = "std" if macro.file == "std.parts" else "def"
				body, file, line = macro.body, macro.file, macro.line
			elif prop is not None:
				kind, body, file, line = "prop", prop.body, prop.file, prop.line
			else:
				kind = "host" if self.host.foreign_parts(target) is not None else "missing"
				body, file, line = [], "", 0
			name = macro.name if macro is not None else prop.name if prop is not None else target  # furniture.leg, not leg
			out = {"name": name, "kind": kind, "file": file, "line": line, "count": count, "at": at, "children": []}
			if target in stack:
				return out  # a cycle; check() reports it
			for stmt in _walk(body):
				if stmt.op == "use" and stmt.args:
					out["children"].append(node(stmt.args[0], _copies(stmt), stmt.line, (*stack, target), stmt.file))
			return out

		prop = self.prop(name) if name not in self.program.macros else None
		return node(prop.name if prop else name, 1, 0, ())

	def check(self) -> dict:
		return check(self.program, self.host)

	# ------------------------------------------------------------ build
	@property
	def compiler(self) -> Tracer:
		if self._compiler is None:
			if self.program.errors:
				raise PartScriptError("; ".join(str(e) for e in self.program.errors[:10]))
			self._compiler = Tracer(self.program, self.host)
			self._compiler.register_materials()
		return self._compiler

	def build(self, name: str, glb: bool = True, steps: bool = False, snaps: bool = False, seed: str | int | None = None) -> Built:
		"""One prop: its parts and steps, baked, and (glb) written to .glb bytes. steps tags each
		face with its source statement (TEXCOORD_1.x) and its use chain (TEXCOORD_1.y, an index into
		Built.origins), for line-by-line previews and for showing which part made which faces.
		seed= deals the prop's draws again, as if it said seed=...: one more variant, the same every time."""
		started = time.perf_counter()
		prop = self.prop(name)
		if seed is not None:
			prop = dataclasses.replace(prop, opts={**prop.opts, "seed": str(seed)})
		asset_id = self.host.asset_id(prop.name)
		compiler = self.compiler
		compiler.snap_sink = []
		compiler.joints = []
		warnings_before = len(compiler.warnings)
		try:
			parts, raw_steps = compiler.trace(prop, asset_id)
			declared = list(compiler.snap_sink)
		finally:
			compiler.snap_sink = None
		face_steps = {id(part): [-1] * len(part.faces) for part in parts}
		for index, step in enumerate(raw_steps):
			for part, first, end in step["faces"]:
				face_steps[id(part)][first:end] = [index] * (end - first)
		parts = [part for part in parts if part.faces]
		finish_parts(prop, parts)
		ao_height = float(prop.opts["ao"]) if prop.opts.get("ao") not in (None, "", "auto") else 1.4
		result = Built(asset_id, parts, raw_steps, declared, warnings=compiler.warnings[warnings_before:])
		result.joints = list(compiler.joints)
		result.links = list(compiler._last_links)
		result.baked = [bake(part, ao_height, face_steps[id(part)]) for part in parts]
		interned: dict = {}
		for baked in result.baked:
			for polygon in baked["polygons"]:
				polygon["origin"] = interned.setdefault(polygon["face"].origin, len(interned))
		result.origins = list(interned)
		budget = prop_budget(prop)
		if result.triangles > budget:
			result.warnings.append(f"{asset_id}: {result.triangles} triangles, over the {budget} budget")
		if snaps and (result.snaps or result.joints or result.links):
			result.baked.append(bake(snap_markers(result.snaps + result.joints + result.links, self._marker_material), 0.0))
		if glb:
			data, missing = glb_bytes(asset_id, result.baked, self.host.materials, self.host.textures, steps)
			result.glb = data
			result.warnings += [f"{asset_id}: texture {name} could not be made; shown grey" for name in missing]
		result.seconds = time.perf_counter() - started
		return result

	def _marker_material(self, kind: str) -> str:
		"""A lit colour material for snap markers of a kind (matching kinds share a colour)."""
		key, hex_, finish = colour_key(self.host.prefix, f"#{kind_colour(kind)}/glow")
		if key not in self.host.materials:
			self.host.add_material(key, Mat(key, 2.0, **colour_material_kwargs(hex_, finish)), ("surface", finish, hex_))
		return key

	def write(self, name: str, out_dir: Path | str, steps: bool = False, seed: str | int | None = None) -> Built:
		result = self.build(name, steps=steps, seed=seed)
		path = Path(out_dir) / f"{result.asset_id}.glb"
		path.parent.mkdir(parents=True, exist_ok=True)
		path.write_bytes(result.glb)
		return result

	def write_all(self, out_dir: Path | str, only: list[str] | None = None, seed: str | int | None = None) -> dict:
		"""Every prop (not buildings) as out_dir/<id>.glb. Returns {"built": [...], "errors": [...], "warnings": [...]}."""
		out: dict = {"built": [], "errors": [], "warnings": []}
		for prop in self.program.props:
			if prop.kind == "building" or prop.file in self.program.imported or (only and prop.name not in only and self.host.asset_id(prop.name) not in only):
				continue
			try:
				result = self.write(prop.name, out_dir, seed=seed)
			except PartScriptError as error:
				out["errors"].append(str(error))
				continue
			out["built"].append({"id": result.asset_id, "triangles": result.triangles, "seconds": round(result.seconds, 4)})
			out["warnings"] += result.warnings
		return out

	def building(self, name: str) -> dict:
		"""A building as placement data (building.export_building), in Y-up space."""
		prop = self.prop(name)
		if prop.kind != "building":
			raise PartScriptError(f"{name} is a prop, not a building", prop.file, prop.line)
		compiler = self.compiler
		return pb.export_building(compiler.building_plan(prop), prop.title, compiler.asset_of)


def _walk(body: list):
	for stmt in body:
		yield stmt
		if stmt.block:
			yield from _walk(stmt.block)


def _copies(stmt: Stmt) -> int:
	"""Copies a line makes when its counts are plain numbers (*3, *2x4, *6%60, mx), else 1 per unknown count."""
	count = 1
	for mod in stmt.mods:
		if mod in ("mx", "my", "mz"):
			count *= 2
			continue
		head = mod[1:].split("@")[0].split("%")[0].split("~")[0].split("^")[0]
		for item in split_top(head, "x"):
			count *= int(item) if item.isdigit() else 1
	return count


def report_json(data) -> str:
	return json.dumps(data, indent=1, default=str)
