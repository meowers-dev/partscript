"""Static check: names, materials, uses, copies and an estimate of triangles, without building."""

from __future__ import annotations

import re

from . import building as pb
from .lang import (_MOD, _SIDED, BUILD_OPS, STYLE_LAYERS, USE_OPTIONS, Bounds, PartScriptError, Program, Prop, Stmt, _arg_value, _counts,
	anchor_point, colour_key, evaluate, expand_points, interpolate, pick_options, split_top)
from .reference import REFERENCE

# The options each shape takes (besides its arguments), and those any line takes. Anything else is a typo.
COMMON_OPTIONS = frozenset({"r", "when", "fade", "wobble", "twist", "bend", "shrink", "index", "jit", "hang", "along", "every", "fit", "closed",
	"joints", "corners", "smooth", "shade", "on", "drop", "facing", "sink"})
SHAPE_OPTIONS = {
	"b": {"skip", "taper", "lean", "from", "to", "break", "chunk", "core", "rubble"}, "bb": {"taper", "lean", "from", "to"}, "bx": set(),
	"c": {"s", "rt", "ax", "arc", "caps", "capm", "from", "to"}, "cone": {"s", "ax", "from", "to"},
	"sph": {"s", "rings"}, "tube": {"s", "ax"}, "wedge": set(), "lathe": {"s", "arc", "cap"}, "ext": {"ax", "taper"}, "pipe": {"s", "arc", "taper"},
	"sweep": {"prof", "arc", "open", "taper"}, "face": {"double"}, "pan": {"double"}, "trim": {"m"}, "vault": {"s"}, "archwall": {"open", "spring", "s"},
	"sign": {"sub", "bg", "fg", "lit", "tex", "wrap", "sides", "mark", "accent", "printed", "double"}, "torus": {"s", "rings", "ax"},
	"frame": {"hole", "hole_at"}, "at": {"s"}, "chain": {"at"}, "link": set(), "join": {"reach", "radius", "mat", "s", "with", "bulge"},
	"row": {"at", "gap", "over", "pack", "align"}, "terrain": {"cells", "height", "steep", "slope", "skirt"},
}
SHAPE_OPTIONS = {op: frozenset(keys) for op, keys in SHAPE_OPTIONS.items()}
_LONG_NAME = {"b": "box", "bb": "bevel_box", "bx": "box_between", "c": "cylinder", "sph": "sphere", "pan": "panel", "ext": "extrude",
	"at": "group", "trim": "decal", "archwall": "arch_wall", "sign": "label", "terrain": "terrain"}

_PLACING = ("on", "drop", "sink", "from", "to", "facing")  # placing options, unless a used def takes them as parameters
TRIANGLE_BUDGET = 6000
PROP_BUDGET = 2500


def prop_budget(prop) -> int:
	"""A prop's triangle budget: budget=N, else a building's, a hero's (hero=1) or a prop's."""
	if prop.opts.get("budget", "").isdigit():
		return int(prop.opts["budget"])
	if prop.kind == "building":
		return pb.BUILDING_BUDGET
	return TRIANGLE_BUDGET if prop.opts.get("hero") else PROP_BUDGET


def check(program: Program, host) -> dict:
	"""Static check: names, materials, uses, copies, estimated triangles. Returns
	{"props": [{id, file, line, triangles, warnings}], "errors": [...], "warnings": [...]}."""
	prefix = host.prefix
	assets = host.known_assets()
	materials = host.known_materials()
	py_props = host.native_props()
	errors = [str(e) for e in program.errors]
	warnings: list[str] = []
	seen: dict = {}
	defined = {p.name for p in program.props}
	props_out = []
	for name, macro in program.macros.items():
		if macro.file != "std.parts" and name in program.std:
			warnings.append(f"{macro.file}:{macro.line}: def {name} replaces the std part {name}")
	for prop in program.props:
		asset_id = host.asset_id(prop.name)
		if asset_id in seen:
			errors.append(f"{prop.file}:{prop.line}: {asset_id} defined twice (also {seen[asset_id]})")
		seen[asset_id] = f"{prop.file}:{prop.line}"
		bare = host.bare(asset_id)
		if program.macro(prop.name, prop.file) is not None:
			kind = "std part" if prop.name in getattr(program, "std", ()) else "def"
			warnings.append(f"{prop.file}:{prop.line}: prop {prop.name} has the name of a {kind}; 'use {prop.name}' draws the {kind}, not this prop")
		if bare in py_props and prop.opts.get("replace") not in ("1", "true"):
			errors.append(f"{prop.file}:{prop.line}: {asset_id} is already one of the host's props; rename it or add replace=1 to remodel it")
		for key, low, high in (("px", 16.0, 512.0), ("ao", 0.0, 10.0)):
			value = prop.opts.get(key, "")
			if value and value != "auto":
				try:
					if not low <= float(value) <= high:
						raise ValueError
				except ValueError:
					errors.append(f"{prop.file}:{prop.line}: {key}={value}: auto or a number from {low:g} to {high:g}")
		context = _Ctx(program, host, assets, materials, defined)
		context.building = prop.kind == "building"
		try:
			tris = context.estimate(prop.body, {**program.env_of(prop.file), "i": 0, "here": Bounds("here", (0.0,) * 3, (0.0,) * 3, world=True)}, depth=0)
		except PartScriptError as error:
			errors.append(str(error))
			tris = 0
		if prop.kind == "building":
			tris += _check_building(program, prop, host, context)
		prop_warnings = list(context.warnings)
		budget = prop_budget(prop)
		if tris > budget:
			prop_warnings.append(f"about {tris} triangles, over the {budget} prop budget (hero=1 allows {TRIANGLE_BUDGET}, budget=N sets it)")
		errors.extend(context.errors)
		props_out.append({"id": asset_id, "title": prop.title, "subcategory": prop.subcategory, "file": prop.file, "line": prop.line,
			"triangles": tris, "warnings": prop_warnings, "kind": prop.kind})
		warnings.extend(f"{asset_id}: {w}" for w in prop_warnings)
	known = set(assets) | {p["id"] for p in props_out} | {host.asset_id(n) for n in py_props}

	def piece_ok(name: str) -> bool:
		return name in known or host.asset_id(name) in known

	for name, style in program.styles.items():
		file, line = program.style_lines.get(name, ("", 0))
		for layer, entries in style.items():
			if layer not in STYLE_LAYERS or layer == "decals":
				continue
			for entry in entries:
				pieces = [entry] if isinstance(entry, str) else [entry.get(k) for k in ("piece", "front", "around", "beside", "above", "facing")
					if isinstance(entry.get(k), str)]
				for piece in pieces:
					if not piece_ok(piece):
						errors.append(f"{file}:{line}: style {name}: no piece {piece!r} ({layer})")
	for bucket, items in program.dressing.items():
		for item in (items.values() if isinstance(items, dict) else items):
			piece = item[0] if isinstance(item, list) else item
			if not piece_ok(piece):
				errors.append(f"dressing {bucket}: no piece {piece!r}")
	return {"prefix": prefix, "props": props_out, "errors": errors, "warnings": warnings,
		"materials_checked": materials is not None, "styles": sorted(program.styles), "dressing": sorted(program.dressing)}


class _Ctx:
	"""Walks statements without building to count triangles and find bad names."""

	def __init__(self, program, host, assets, materials, defined):
		self.program = program
		self.host = host
		self.prefix = host.prefix
		self.assets = assets
		self.materials = materials
		self.defined = defined
		self.errors: list[str] = []
		self.warnings: list[str] = []
		self.building = False

	def fail(self, stmt: Stmt, message: str) -> None:
		self.errors.append(f"{stmt.where()}: {message}")

	def material(self, stmt: Stmt, token: str, env: dict) -> None:
		for value in _every_material(token, env):
			if value == "none":
				continue
			if value.startswith("#"):
				try:
					if colour_key(self.prefix, value) is None:
						self.fail(stmt, f"colour {value!r}: #rrggbb or #rrggbb/finish")
				except ValueError as error:
					self.fail(stmt, str(error))
				continue
			if self.materials is not None and value not in self.materials and self.host.asset_id(value) not in self.materials:
				self.fail(stmt, f"unknown material {value!r} (partscript materials lists them; or use #rrggbb/finish)")

	def number(self, stmt: Stmt, text: str, env: dict) -> float:
		if text.startswith("~") and isinstance(env.get(text[1:]), Bounds):
			return 0.0  # on(desk)
		try:
			return evaluate(text.lstrip("~") or "0", env)
		except ValueError as error:
			self.fail(stmt, str(error))
			return 0.0

	def vec(self, stmt: Stmt, text: str, env: dict, n: int = 3) -> list:
		if n == 3 and "," not in text:
			try:
				point = anchor_point(text, env)
			except ValueError as error:
				self.fail(stmt, str(error))
				return [0.0] * 3
			if point is not None:
				return list(point)
		parts = split_top(text)
		if len(parts) == 1:
			parts = parts * n
		if len(parts) != n:
			self.fail(stmt, f"{text!r}: expected {n} numbers")
			return [0.0] * n
		return [self.number(stmt, p, env) for p in parts]

	def copies(self, stmt: Stmt, env: dict) -> int:
		count = 1
		for mod in stmt.mods:
			match = _MOD.match(mod)
			if match and match.group(1):
				try:
					counts = _counts(match.group(1), env)
				except ValueError as error:
					self.fail(stmt, str(error))
					continue
				for n in counts:
					count *= n
				if match.group(2):
					self.vec(stmt, match.group(2), env, len(counts) if len(counts) > 1 else 3)
				if "hang" in stmt.opts:
					if len(counts) > 1:
						self.fail(stmt, "hang= bends a single *N@ row (not a grid)")
					self.number(stmt, stmt.opts["hang"], env)
			elif match and match.group(3):
				try:
					count *= _counts(match.group(3), env)[0]
				except ValueError as error:
					self.fail(stmt, str(error))
				self.number(stmt, match.group(4), env)
			elif match and match.group(5):
				try:
					count *= _counts(match.group(5), env)[0]
				except ValueError as error:
					self.fail(stmt, str(error))
				values = split_top(match.group(6))
				if not 1 <= len(values) <= 3:
					self.fail(stmt, f"scatter {mod}: *N~R (a disc), *N~W,D (an area) or *N~W,D,GAP")
				for value in values:
					self.number(stmt, value, env)
			elif match and match.group(7):
				try:
					count *= _counts(match.group(7), env)[0]
				except ValueError as error:
					self.fail(stmt, str(error))
				name, _, gap = match.group(8).partition(",")
				if not isinstance(env.get(name), Bounds):
					self.fail(stmt, f"scatter on {name}: no shape of that name above this line (name one: {name} = box ...)")
				if gap:
					self.number(stmt, gap, env)
				if stmt.opts.get("facing", "any") not in ("up", "down", "side", "any"):
					self.fail(stmt, f"facing={stmt.opts['facing']}: up, side, down or any")
			elif mod in ("mx", "my", "mz"):
				count *= 2
		if "facing" in stmt.opts and not any("^" in m for m in stmt.mods):
			self.fail(stmt, "facing= goes with scatter N on NAME (which of NAME's faces the copies grow on)")
		if "along" in stmt.opts:
			count *= self.along(stmt, env)
		if count > 400:
			self.fail(stmt, f"{count} copies; keep arrays under 400")
		return count

	def along(self, stmt: Stmt, env: dict) -> int:
		"""How many copies along="P P P" makes (and its mistakes)."""
		from kitlib.paths import path_frames
		o = stmt.opts
		line = str(env.get(o["along"], o["along"]))
		points = [self.vec(stmt, p, env, len(split_top(p))) if len(split_top(p)) in (2, 3) else self.fail(stmt, f"along point {p!r}: x,y or x,y,z")
			for p in line.split()]
		if any(p is None for p in points):
			return 1
		try:
			flag = lambda key: o.get(key) in ("1", "true")  # noqa: E731
			return len(path_frames(points, self.number(stmt, o["every"], env) if "every" in o else 0.0, flag("fit"), flag("corners"),
				flag("closed"), flag("joints")))
		except ValueError as error:
			self.fail(stmt, str(error))
			return 1

	def estimate(self, body: list, env: dict, depth: int) -> int:
		if depth > 8:
			raise PartScriptError("use/at nested more than 8 deep (a def using itself?)")
		total = 0
		env = dict(env)
		env.setdefault("here", Bounds("here", (0.0,) * 3, (0.0,) * 3, world=True))
		for stmt in body:
			op, a, o = stmt.op, stmt.args, stmt.opts
			macro = self.program.macro(a[0], stmt.file) if op == "use" and a else None
			if macro is not None and any(k in macro.params for k in _PLACING):
				o = {k: v for k, v in o.items() if not (k in _PLACING and k in macro.params)}
			try:
				if op == "set":
					env.update(o)
					continue
				if op == "snap":
					if len(a) < 3:
						self.fail(stmt, f"'snap' needs a name, a position and a direction: {pb.SNAP_USAGE}")
						continue
					self.vec(stmt, a[1], env)
					for key, text in (("direction", a[2]), ("up", o.get("up"))):
						if text is not None:
							try:
								pb.parse_dir(text, lambda v: self.number(stmt, v, env))
							except ValueError as error:
								self.fail(stmt, f"snap {key}: {error}")
					if not re.fullmatch(r"[a-z][a-z0-9_]{0,30}", a[0]) or ("kind" in o and not re.fullmatch(r"[a-z][a-z0-9_]{0,30}", o["kind"])):
						self.fail(stmt, f"snap names and kinds are lower_snake_case words: {pb.SNAP_USAGE}")
					continue
				if op in BUILD_OPS:
					if not self.building:
						self.fail(stmt, f"'{op}' belongs in a building (building NAME \"Title\" kit=SET), not a prop")
					continue
				if op == "link":
					if len(a) < 3:
						self.fail(stmt, "link KIND at=P toward=DIR: an open end of kind KIND at P, pointing DIR (+x -y up ... or x,y,z)")
						continue
					if not re.fullmatch(r"[a-z][a-z0-9_]{0,30}", a[0]):
						self.fail(stmt, f"link kind {a[0]!r}: a lower_snake_case word (rail, pipe, cable...)")
					self.copies(stmt, env)
					self.vec(stmt, a[1], env)
					try:
						pb.parse_dir(a[2], lambda v: self.number(stmt, v, env))
					except ValueError as error:
						self.fail(stmt, f"link toward: {error}")
					continue
				if op == "join":
					if not a:
						self.fail(stmt, "join KIND [reach=.6] [radius=.025] [mat=M] [sides=6] [bulge=B] [with=DEF]")
						continue
					for key in ("reach", "radius", "bulge"):
						if key in o:
							self.number(stmt, o[key], env)
					if "mat" in o:
						self.material(stmt, o["mat"], env)
					if "with" in o and self.program.macro(o["with"], stmt.file) is None:
						self.fail(stmt, f"join with={o['with']}: no def of that name (it draws the bridge along +X, length long)")
					continue
				if op == "chain":
					names = []
					for token in a:
						name, _, count = token.partition("*")
						names += [name] * (int(self.number(stmt, count, env)) if count else 1)
					if not names:
						self.fail(stmt, "chain needs pieces: chain path_straight path_curve*2 ...")
					total += sum(self.use_triangles(stmt, name, env, depth) for name in names) * self.copies(stmt, env)
					continue
				if op in ("part", "size", "card"):
					if op == "card":
						unknown = set(o) - {"what", "where", "pairs", "look", "avoid", "notes"}
						if unknown:
							self.fail(stmt, f"card fields: what where pairs look avoid notes (not {', '.join(sorted(unknown))})")
					continue
				n = self.copies(stmt, env)
				if "sink" in o and "drop" not in o:
					self.fail(stmt, "sink= goes with drop= (how far a dropped thing settles into the ground)")
				elif "sink" in o:
					self.number(stmt, o["sink"], env)
				if "drop" in o and o["drop"] not in ("1", "true", "lean"):
					self.fail(stmt, f"drop={o['drop']}: 1 (fall onto what is below) or lean (and tilt with the ground)")
				if "taper" in o and op in ("pipe", "sweep") and not 0 <= self.number(stmt, o["taper"], env) <= 20:
					self.fail(stmt, f"taper={o['taper']}: the size at the end of the line, 1 = unchanged (0-20)")
				if op == "b" and "break" in o:
					if not 0 <= self.number(stmt, o["break"], env) <= 1:
						self.fail(stmt, f"break={o['break']}: the share knocked out, 0-1")
					if "chunk" in o and self.number(stmt, o["chunk"], env) <= 0.02:
						self.fail(stmt, f"chunk={o['chunk']}: the size of the pieces it breaks into, in metres (over 2 cm)")
					if "core" in o:
						self.material(stmt, o["core"], env)
					if "rubble" in o:
						self.number(stmt, o["rubble"], env)
				elif any(k in o for k in ("chunk", "core", "rubble")) and op == "b":
					self.fail(stmt, "chunk=, core= and rubble= go with break= (box ... break=.3 chunk=.25 core=brick rubble=.5)")
				if "on" in o and not isinstance(env.get(o["on"].partition(".")[0]), Bounds):
					self.fail(stmt, f"on={o['on']}: no shape of that name above this line (name one: {o['on']} = box ...)")
				if stmt.name:
					# A named line: later lines read it (desk.top); checked with a stand-in box a metre each way.
					env[stmt.name] = Bounds(stmt.name, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
				for key in ("from", "to"):
					if key in o and op != "bx":
						self.vec(stmt, o[key], env)
				if op in SHAPE_OPTIONS:
					unknown = [k for k in o if k not in SHAPE_OPTIONS[op] and k not in COMMON_OPTIONS]
					if unknown:
						long = {"s": "scale" if op == "at" else "sides", "ax": "axis", "rt": "top_radius", "capm": "cap_mat", "r": "turn", "jit": "jitter"}
						takes = sorted(long.get(k, k) for k in SHAPE_OPTIONS[op] - {"printed"})
						common = sorted(long.get(k, k) for k in COMMON_OPTIONS - {"index"})
						self.fail(stmt, f"{_LONG_NAME.get(op, op)}: no option {unknown[0]}= (it takes {', '.join(takes) or 'none of its own'}; "
							f"any line takes {', '.join(common)})")
				if "index" in o:
					names = o["index"].split(",")
					if not all(re.fullmatch(r"[a-z_]\w*", name) for name in names) or len(names) > 3:
						self.fail(stmt, f"as {o['index']}: one name (as k) or a grid's column,row[,layer] (as col,row)")
					env = {**env, **{name: 0 for name in names}}
				for key, low, high in (("twist", -3600, 3600), ("shrink", 0, 10)):
					if key in o and not low <= self.number(stmt, o[key], {**env, "i": 0}) <= high:
						self.fail(stmt, f"{key}={o[key]}: {'degrees of turn at the top' if key == 'twist' else 'the size at the top, 1 = unchanged'}")
				if "bend" in o:
					self.vec(stmt, o["bend"], {**env, "i": 0}, 2 if "," in o["bend"] else 1)
				if "smooth" in o and not 1 <= self.number(stmt, o["smooth"], env) <= 32:
					self.fail(stmt, f"smooth={o['smooth']}: pieces between each pair of points, 1-32")
				if "when" in o:
					# Counted unless it is false whatever the copy (it names no i and draws no rand).
					if not re.search(r"\bi\b|rand\(|\bhere\b", o["when"]) and self.number(stmt, o["when"], env) == 0:
						continue
					self.number(stmt, o["when"], {**env, "i": 0})
				if "wobble" in o:
					self.vec(stmt, o["wobble"], {**env, "i": 0})
				if "fade" in o and not 0.0 <= self.number(stmt, o["fade"], {**env, "i": 0}) <= 1.0:
					self.fail(stmt, f"fade={o['fade']}: the tone at the base, 0-1 (1 = no fade)")
				tris = 0
				# Placement options are expressions too: check them here, not first at build.
				# (s= is a side count on round shapes and a scale on use and at.)
				for key in ("r", "s"):
					if key in o and not (key == "s" and op in _SIDED):
						self.vec(stmt, o[key], env)
				need = {"b": 3, "bb": 4, "bx": 3, "c": 4, "cone": 4, "sph": 3, "tube": 5, "wedge": 3, "pan": 4, "trim": 4, "sign": 4,
					"torus": 4, "frame": 5}
				if op in need and len(a) < need[op]:
					self.fail(stmt, f"'{op}' needs {need[op]} arguments, got {len(a)}: {self.usage(op)}")
					continue
				if op in ("b", "bb"):
					if "taper" in o and not all(0.0 <= v <= 4.0 for v in self.vec(stmt, o["taper"], env, 2)):
						self.fail(stmt, f"taper={o['taper']}: the top face's scale in x,y, each 0-4 (1 = straight, 0 = a ridge or point)")
					if "lean" in o:
						if "," not in o["lean"]:
							self.fail(stmt, f"lean={o['lean']}: dx,dy, how far the top face shifts in metres (lean=0,-.1 leans toward the front)")
						else:
							self.vec(stmt, o["lean"], env, 2)
				if op == "b":
					self.vec(stmt, a[0], env), self.vec(stmt, a[1], env), self.material(stmt, a[2], env)
					tris = 12 - 2 * len([s for s in o.get("skip", "").split(",") if s])
				elif op == "bx":
					self.vec(stmt, a[0], env), self.vec(stmt, a[1], env), self.material(stmt, a[2], env)
					tris = 12
				elif op == "bb":
					self.vec(stmt, a[0], env), self.vec(stmt, a[1], env), self.material(stmt, a[3], env)
					tris = 44 if self.number(stmt, a[2], env) > 0 else 12
				elif op in ("c", "cone"):
					self.vec(stmt, a[0], env), self.number(stmt, a[1], env), self.number(stmt, a[2], env), self.material(stmt, a[3], env)
					sides = int(self.number(stmt, o.get("s", "10" if op == "c" else "8"), env))
					if sides < 3 or sides > 48:
						self.fail(stmt, f"s={sides}: 3-48 sides (6-16 is the house style)")
					tris = sides * 2 + (0 if o.get("caps") == "0" else (sides - 2) * 2)
				elif op == "torus":
					self.vec(stmt, a[0], env), self.number(stmt, a[1], env), self.number(stmt, a[2], env), self.material(stmt, a[3], env)
					tris = int(self.number(stmt, o.get("s", "16"), env)) * int(self.number(stmt, o.get("rings", "6"), env)) * 2
				elif op == "frame":
					self.vec(stmt, a[0], env), self.number(stmt, a[1], env), self.number(stmt, a[2], env), self.number(stmt, a[3], env)
					self.material(stmt, a[4], env)
					for key in ("hole", "hole_at"):
						if key in o:
							self.vec(stmt, o[key], env, 2)
					tris = 48
				elif op == "sph":
					self.vec(stmt, a[0], env), self.number(stmt, a[1], env), self.material(stmt, a[2], env)
					tris = int(self.number(stmt, o.get("s", "10"), env)) * int(self.number(stmt, o.get("rings", "6"), env)) * 2
				elif op == "tube":
					self.vec(stmt, a[0], env), self.number(stmt, a[1], env), self.number(stmt, a[2], env), self.number(stmt, a[3], env)
					self.material(stmt, a[4], env)
					tris = int(self.number(stmt, o.get("s", "12"), env)) * 8
				elif op == "wedge":
					self.vec(stmt, a[0], env), self.vec(stmt, a[1], env), self.material(stmt, a[2], env)
					tris = 8
				elif op == "lathe":
					if len(a) < 4:
						self.fail(stmt, f"'lathe' needs a centre, a material and 2+ r:z points: {self.usage(op)}")
						continue
					self.vec(stmt, a[0], env), self.material(stmt, a[1], env)
					for point in a[2:]:
						if ":" not in point:
							self.fail(stmt, f"lathe point {point!r}: r:z")
					tris = (len(a) - 3) * int(self.number(stmt, o.get("s", "16"), env)) * 2
				elif op == "ext":
					if len(a) < 6:
						self.fail(stmt, f"'ext' needs a centre, a width, a material and 3+ u:v outline points: {self.usage(op)}")
						continue
					self.vec(stmt, a[0], env), self.number(stmt, a[1], env), self.material(stmt, a[2], env)
					for point in a[3:]:
						pair = point.split(":")
						if len(pair) != 2:
							self.fail(stmt, f"outline point {point!r}: u:v (two numbers joined by a colon, such as -.4:.2)")
						else:
							self.number(stmt, pair[0], env), self.number(stmt, pair[1], env)
					if o.get("ax", "x") not in ("x", "y", "z"):
						self.fail(stmt, f"ax={o['ax']}: x (side profile y:z), y (front profile x:z) or z (plan x:y)")
					if "taper" in o and not 0.0 <= self.number(stmt, o["taper"], env) <= 4.0:
						self.fail(stmt, f"taper={o['taper']}: the width's scale at the top of the outline, 0-4")
					tris = 4 * (len(a) - 3) - 4
				elif op == "vault":
					if len(a) < 5:
						self.fail(stmt, f"'vault' needs C W D RISE M: {self.usage(op)}")
						continue
					self.vec(stmt, a[0], env), self.number(stmt, a[1], env), self.number(stmt, a[2], env), self.number(stmt, a[3], env)
					self.material(stmt, a[4], env)
					tris = int(self.number(stmt, o.get("s", "12"), env)) * 2
				elif op == "archwall":
					if len(a) < 5:
						self.fail(stmt, f"'archwall' needs C W H T M: {self.usage(op)}")
						continue
					self.vec(stmt, a[0], env), self.number(stmt, a[1], env), self.number(stmt, a[2], env), self.number(stmt, a[3], env)
					self.material(stmt, a[4], env)
					tris = int(self.number(stmt, o.get("s", "12"), env)) * 6 + 28
				elif op in ("pipe", "sweep") and "arc" in o:
					self.material(stmt, a[0], env)
					arc = [self.number(stmt, v, env) for v in split_top(o["arc"])]
					if len(arc) not in (5, 6):
						self.fail(stmt, "arc=cx,cz,r,a0,a1[,n]")
					steps = int(arc[5]) if len(arc) == 6 else 12
					profile = len(o["prof"].split(",")) if op == "sweep" and "prof" in o else int(self.number(stmt, o.get("s", "6"), env))
					tris = steps * profile * 2
				elif op == "pipe":
					a = a[:2] + expand_points(a[2:], env)
					if len(a) < 4 and "arc" not in o:
						self.fail(stmt, f"'pipe' needs a material, a radius and 2+ points: {self.usage(op)}")
						continue
					self.material(stmt, a[0], env), self.number(stmt, a[1], env)
					for point in a[2:]:
						self.vec(stmt, point, env)
					tris = (len(a) - 3) * int(self.number(stmt, o.get("smooth", "1"), env)) * int(self.number(stmt, o.get("s", "6"), env)) * 2 + 8
				elif op == "sweep":
					if (len(a) < 3 and "arc" not in o) or "prof" not in o:
						self.fail(stmt, f"'sweep' needs a material, prof= and 2+ points: {self.usage(op)}")
						continue
					self.material(stmt, a[0], env)
					profile = o["prof"].split(",")
					tris = (len(a) - 2) * int(self.number(stmt, o.get("smooth", "1"), env)) * len(profile) * 2
				elif op == "face":
					if len(a) < 4:
						self.fail(stmt, f"'face' needs a material and 3+ points: {self.usage(op)}")
						continue
					self.material(stmt, a[0], env)
					for point in a[1:]:
						self.vec(stmt, point, env)
					tris = (len(a) - 3) * (2 if o.get("double") in ("1", "true") else 1)
				elif op in ("pan", "trim"):
					self.vec(stmt, a[0], env), self.number(stmt, a[1], env), self.number(stmt, a[2], env)
					if op == "pan":
						self.material(stmt, a[3], env)
					else:
						cells = {cell for atlas in self.host.atlases() for cell in atlas.cells}
						if a[3] not in cells:
							self.fail(stmt, f"trim cell {a[3]!r}: one of {', '.join(sorted(cells))}" if cells else
								"trim: the texture provider has no decal sheets (use pan with a material instead)")
					tris = 4 if o.get("double") else 2
				elif op == "sign":
					self.vec(stmt, a[0], env), self.number(stmt, a[1], env), self.number(stmt, a[2], env)
					if not a[3].startswith('"'):
						self.fail(stmt, 'sign text goes in quotes: sign 0,-.05,2 1.6 .3 "THE HOPE & ANCHOR"')
					for text in (a[3], o.get("sub", "")):
						try:
							interpolate(text, {**env, "i": 0})
						except ValueError as error:
							self.fail(stmt, f"label text: {error}")
					tex = o.get("tex", "256x32").split("x")
					if len(tex) != 2 or not all(t.isdigit() and int(t) in (16, 32, 64, 128, 256) for t in tex):
						self.fail(stmt, f"tex={o.get('tex')}: WxH, each 16, 32, 64, 128 or 256 (textures are powers of two)")
					tris = 2
					if "wrap" in o:
						radius = self.number(stmt, o["wrap"], env)
						sides = int(self.number(stmt, o.get("sides", "12"), env))
						if radius <= 0 or self.number(stmt, a[2], env) <= 0 or not 3 <= sides <= 48:
							self.fail(stmt, "wrapped sign needs positive radius/height and 3-48 sides")
						if o.get("mark", "bolt") not in ("bolt", "orbit", "star", "wave"):
							self.fail(stmt, "wrapped sign mark: bolt, orbit, star or wave")
						if "accent" in o and not re.fullmatch(r"#[0-9a-fA-F]{6}", o["accent"]):
							self.fail(stmt, "wrapped sign accent must be #rrggbb")
						tris = sides * 2
				elif op == "use":
					if not a:
						self.fail(stmt, f"'use' needs a name: {self.usage(op)}")
						continue
					if len(a) > 1:
						self.vec(stmt, a[1], env)
					tris = self.use_triangles(stmt, a[0], env, depth)
				elif op == "at":
					if a:
						self.vec(stmt, a[0], env)
					tris = self.estimate(stmt.block or [], env, depth + 1)
				elif op == "terrain":
					self.vec(stmt, a[0], env), self.material(stmt, a[2], env)
					size = split_top(a[1])
					for value in size:
						self.number(stmt, value, env)
					if len(size) not in (1, 2):
						self.fail(stmt, f"terrain size={a[1]}: W,D (metres across and back)")
					cells = [int(self.number(stmt, v, env)) for v in split_top(o.get("cells", "16"))]
					if not all(1 <= c <= 96 for c in cells) or len(cells) > 2:
						self.fail(stmt, f"cells={o.get('cells')}: N or N,M corners a side, 1-96")
					self.number(stmt, o.get("height", "0"), {**env, "x": 0.0, "y": 0.0})
					if "steep" in o:
						self.material(stmt, o["steep"], env)
					if "slope" in o and not 0 < self.number(stmt, o["slope"], env) < 90:
						self.fail(stmt, f"slope={o['slope']}: degrees, 0-90 (faces steeper than this take steep=)")
					tris = cells[0] * cells[-1] * 2 + (0 if o.get("skirt") in ("0", "false") else (cells[0] + cells[-1]) * 4 + 2)
				elif op == "row":
					if a and a[0].lstrip("+-") not in ("x", "y", "z"):
						self.fail(stmt, f"row {a[0]}: the axis it runs along, x, y or z (-x runs the other way); stack is row z")
					for key in ("gap", "over"):
						if key in o:
							self.number(stmt, o[key], env)
					if "at" in o:
						self.vec(stmt, o["at"], env)
					if o.get("pack", "start") not in ("start", "centre", "center", "end"):
						self.fail(stmt, f"pack={o['pack']}: start, centre or end")
					for word in (w for w in o.get("align", "").split(",") if w):
						if word not in ("left", "right", "front", "back", "bottom", "top", "centre", "center"):
							self.fail(stmt, f"align={word}: left right front back bottom top or centre (align=back,bottom)")
					for child in stmt.block or []:
						if child.name or child.op in ("part", "snap", "join", "link", "chain") or child.op in BUILD_OPS:
							self.fail(child, f"{child.name + ' = ...' if child.name else repr(child.op)} can't go in a row or stack "
								"(name the row itself: books = row x ...)")
					tris = self.estimate(stmt.block or [], env, depth + 1)
				total += tris * n
			except PartScriptError as error:
				self.errors.append(str(error))
		return total

	def use_triangles(self, stmt: Stmt, name: str, env: dict, depth: int) -> int:
		macro = self.program.macro(name, stmt.file)
		if macro is not None:
			local = {**env, **self.program.env_of(macro.file), **macro.params, **({"length": 1.0} if "from" in stmt.opts else {})}
			for key, value in stmt.opts.items():
				if key in USE_OPTIONS or (key in _PLACING and key not in macro.params):
					continue
				if key not in macro.params:
					self.fail(stmt, f"'{name}' has no parameter {key!r} (it takes {', '.join(macro.params) or 'none'})")
				local[key] = _arg_value(value, env)
			return self.estimate(macro.body, local, depth + 1)
		asset = name if "__" in name else self.host.asset_id(name)
		if asset in self.assets or name in self.assets:
			return int((self.assets.get(asset) or self.assets.get(name))["triangles"])
		own = self.program.find_prop(name, stmt.file, lambda p: (p.name, self.host.asset_id(p.name)))
		if own is not None:
			return self.estimate(own.body, {**self.program.env_of(own.file), "i": 0}, depth + 1)
		if self.host.bare(asset) in self.host.native_props():
			self.warnings.append(f"{stmt.where()}: {asset} is not built yet; counting 0 triangles")
			return 0
		if self.host.foreign_parts(name) is not None:
			return sum(len(f.points) - 2 for part in self.host.foreign_parts(name) for f in part.faces)
		if stmt.called:
			self.fail(stmt, f"unknown statement '{name}' (not a shape, a def, a std part or a prop; partscript ref lists the shapes)")
		else:
			self.fail(stmt, f"'use {name}': no def, std part or prop of that name (partscript ref lists the std parts)")
		return 0

	@staticmethod
	def usage(op: str) -> str:
		for line in REFERENCE.splitlines():
			stripped = line.strip()
			if stripped.startswith(op + " ") or f"  {op} " in line:
				return stripped
		return op

def _every_material(token: str, env: dict) -> list[str]:
	"""Every material a token can give: each item of its cycle, parameters looked up, each pick() option."""
	out = []
	for item in token.split("|"):
		value = str(env.get(item, item)) if not item.startswith("#") else item
		for option in pick_options(value):
			for picked in option.split("|"):
				held = env.get(picked) if not picked.startswith("#") else None
				out.extend(o for found in pick_options(held) for o in found.split("|")) if isinstance(held, str) else out.append(picked)
	return out


def _check_building(program: Program, prop: Prop, host, context: "_Ctx") -> int:
	"""A building's set and room plan, without building: errors into context, triangles of the pieces it places."""
	env = {**program.env_of(prop.file), "i": 0}
	try:
		kit = pb.resolve_kit(program, prop.opts.get("kit", ""), host.base_kit(), file=prop.file)
	except ValueError as error:
		context.errors.append(f"{prop.file}:{prop.line}: {error}")
		return 0
	result = pb.plan(prop, kit, lambda text: evaluate(text, env))
	context.errors.extend(result.errors)
	context.warnings.extend(result.warnings)
	total = 0
	counted: dict = {}
	for placement in result.placements:
		piece, stmt = placement.piece, placement.stmt
		if placement.role == "fill":
			macro = program.macro(piece, stmt.file)
			if macro is None:
				context.errors.append(f"{stmt.where()}: fill={piece}: no def of that name (a room's fill names a def that furnishes it)")
				continue
			local = {**env, **program.env_of(macro.file), **macro.params, "i": 0}
			local.update({k: v for k, v in placement.fill["env"].items() if k != "__seed__"})
			unknown = [k for k in placement.fill["env"] if k not in macro.params and k not in pb.FILL_VARIABLES and k != "__seed__"]
			if unknown:
				context.errors.append(f"{stmt.where()}: fill={piece}: no parameter {unknown[0]!r} (it takes {', '.join(macro.params) or 'none'})")
			total += context.estimate(macro.body, local, 1)
			continue
		if piece not in counted:
			own = program.find_prop(piece, stmt.file, lambda p: (p.name, host.asset_id(p.name), host.bare(p.name)))
			if own is not None:
				counted[piece] = context.estimate(own.body, {**program.env_of(own.file), "i": 0}, 1)
			elif program.macro(piece, stmt.file) is not None or any(p.kind == "building" and p.name == piece for p in program.props):
				context.errors.append(f"{stmt.where()}: {piece!r} ({placement.role}) is a def or a building; a building places props")
				counted[piece] = 0
			else:
				before = len(context.errors)
				counted[piece] = context.use_triangles(Stmt("use", [piece], {}, [], stmt.file, stmt.line), piece, env, 1)
				if len(context.errors) > before:
					what = f"kit {kit.name} piece" if placement.role not in ("attach", "place") else placement.role
					context.errors[before:] = [f"{stmt.where()}: {what} {piece!r}: no prop or built piece of that name"]
		total += counted[piece]
	for name, entries in kit.snaps.items():
		for entry in entries:
			if len(entry["tokens"]) < 3:
				context.errors.append(f"{entry['file']}:{entry['line']}: {pb.SNAP_USAGE}")
	return total
