"""The compiler: a Program's props into kitlib Parts (faces, materials, snaps), for a Host."""

from __future__ import annotations

import math
import random
import re

from kitlib.geom import Face, Mat, Part
from kitlib.maths import Euler, Matrix, Vector
from kitlib.paths import path_frames, smooth_points
from kitlib.ruin import broken_box
from kitlib.surface import SurfaceIndex, spread

from . import building as pb
from .lang import (_MOD, BUILD_OPS, USE_OPTIONS, Bounds, PartScriptError, Program, Prop, Stmt, _arg_value, _counts, _Skip, _unquote,
	anchor_point, colour_key, colour_material_kwargs, evaluate, expand_points, interpolate, is_dynamic, material_items, pick_options,
	sign_key, split_top)

def finish_parts(prop: Prop, parts: list) -> None:
	"""A prop's px= and ao=auto, both sized from its bounds. px sets how many texels a metre of its
	tiling textures shows (auto: about 80 across its longest side, like a model with its own small
	texture page; the library's base is 32). ao=auto fades the grounded darkening over its own height."""
	px, ao = prop.opts.get("px", ""), prop.opts.get("ao", "")
	parts = [p for p in parts if p.faces]
	if not parts or (not px and ao != "auto"):
		return
	size = [max(p.bounds()[1][k] for p in parts) - min(p.bounds()[0][k] for p in parts) for k in range(3)]
	if px:
		density = max(32.0, min(256.0, 80.0 / max(max(size), 0.001))) if px == "auto" else float(px)
		for part in parts:
			part.uv_scale = density / 32.0
	if ao == "auto":
		for part in parts:
			part.ao_height = max(0.15, min(1.4, size[2] * 1.2))


class Compiler:
	def __init__(self, program: Program, host):
		self.program = program
		self.host = host
		self.prefix = host.prefix
		self.colours: dict = {}
		self.signs: dict = {}
		self._foreign: dict = {}
		self._building: set = set()
		self.joints: list = []  # where chained pieces met (building.Snap in prop space), for snap markers
		self.links: list = []  # open ends (building.Snap, kind = the link kind) waiting for a join, in this build's space
		self.join_rules: list = []  # (stmt, env) of the join lines of this build
		self._last_links: list = []  # the open ends a finished build left, for whatever places it
		self._foreign_links: dict = {}
		self.snap_sink: list | None = None  # snap statements land here (piece-local Snaps) while it is a list
		self._plans: dict = {}  # id(building prop) -> building.Plan
		self._pieces: dict = {}  # piece name -> (faces, declared snaps, open link ends)
		self.warnings: list[str] = []
		self._path: tuple = ()  # (file, line) of the use lines being run, outermost first (Face.origin)
		self._fallen: list = []  # the chunks a broken box just lost, for its rubble

	def new_part(self, name: str) -> Part:
		return Part(name, self.host.materials)

	# -- buildings and snaps
	def program_prop(self, name: str, file: str = "") -> "Prop | None":
		"""The prop a name used in file means (its own namespace's first), by name or asset id."""
		host = self.host
		return self.program.find_prop(name, file, lambda p: (p.name, host.asset_id(p.name), host.bare(p.name)))

	def _key(self, name: str, file: str) -> str:
		"""What a name used in file resolves to, as one name for caches (furniture.table, or the name itself)."""
		macro = self.program.macro(name, file)
		if macro is not None:
			return macro.name
		prop = self.program_prop(name, file)
		return prop.name if prop is not None else name

	def asset_of(self, name: str) -> str:
		"""The asset a building places for a piece (Host.placed_id decides the form)."""
		if "__" in name:
			return name
		prop = self.program_prop(name)
		return self.host.placed_id(self.host.asset_id(prop.name if prop else name), prop is not None)

	def piece(self, name: str, stmt: "Stmt | None" = None) -> tuple:
		"""(faces in the piece's own space, {snap name: Snap} it declares) for a prop, built piece or pack__id."""
		stmt = stmt or Stmt("use", [name], {}, [], "<building>", 0)
		name = self._key(name, stmt.file)
		if name in self._pieces:
			return self._pieces[name]
		saved, self.snap_sink = self.snap_sink, []
		try:
			prop = self.program_prop(name)
			macro = self.program.macro(name)
			links: list = []
			if prop is not None:
				built = self.builder(prop, self.host.asset_id(prop.name))()
				faces = [f for part in (built if isinstance(built, list) else [built]) for f in part.faces]
				links = self._last_links
			elif macro is not None:
				scratch = [self.new_part(name)]
				scope = self._open_links()
				try:
					home = self.program.env_of(macro.file)
					self._run(macro.body, {**home, **{k: _arg_value(v, home) for k, v in macro.params.items()}},
						scratch, _identity(), 1, Prop(name, name, "", {}, macro.body, macro.file, macro.line))
				finally:
					links = self._close_links(scope, scratch)
				faces = scratch[0].faces
			else:
				faces = self._foreign_faces(name, stmt)
				links = self._foreign_links.get(name, [])
			snaps = {snap.name: snap for snap in self.snap_sink}
		finally:
			self.snap_sink = saved
		self._pieces[name] = (faces, snaps, links)
		return self._pieces[name]

	def piece_snaps(self, name: str, role: str, kit) -> dict:
		"""Every snap of a piece: its bounds' (base, top, front...), its role's in the set (wall ends and
		faces, floor edges), the set's snap lines for it, and its own snap statements, later winning."""
		faces, declared, _ = self.piece(name)
		points = [p for f in faces for p in f.points]
		snaps = {}
		if points:
			snaps.update(pb.bounds_snaps(tuple(min(p[k] for p in points) for k in range(3)), tuple(max(p[k] for p in points) for k in range(3))))
		for kit_role in [role, *kit.roles_of(name)]:
			snaps.update(pb.role_snaps(kit_role, kit))
		env = self.program.env_of(kit.file)
		for entry in kit.snaps.get(name, []):
			snaps.update({snap.name: snap for snap in [self._snap(entry["tokens"], {}, env)]})
		snaps.update(declared)
		return snaps

	def _snap(self, args: list, opts: dict, env: dict, name_suffix: str = ""):
		number = lambda text: evaluate(text, env)  # noqa: E731
		tokens = [t for t in args if "=" not in t]
		opts = {**{t.split("=", 1)[0]: t.split("=", 1)[1] for t in args if "=" in t}, **opts}
		direction = pb.parse_dir(tokens[2], number)
		up = pb.parse_dir(opts["up"], number) if "up" in opts else pb.default_up(direction)
		return pb.Snap(tokens[0] + name_suffix, self._vec(tokens[1], env), direction, up, opts.get("kind", "any"))

	def building_plan(self, prop: Prop):
		"""The plan of a building, with its attachments placed (computed once a compile)."""
		if id(prop) not in self._plans:
			env = self.program.env_of(prop.file)
			try:
				kit = pb.resolve_kit(self.program, prop.opts.get("kit", ""), self.host.base_kit(), file=prop.file)
				result = pb.plan(prop, kit, lambda text: evaluate(text, env))
				if result.errors:
					raise PartScriptError(result.errors[0])
				pb.resolve(result, lambda name, role: self.piece_snaps(name, role, kit))
				self.warnings += result.warnings
			except pb.BuildError as error:
				raise PartScriptError(str(error), prop.file, prop.line)
			self._plans[id(prop)] = result
		return self._plans[id(prop)]

	def place(self, placement, parts: list, frame) -> None:
		"""One placed piece of a building into the current part (a room's fill= runs its def there)."""
		if placement.role == "fill":
			macro = self.program.macro(placement.piece, placement.stmt.file)
			if macro is None:
				raise PartScriptError(f"fill={placement.piece}: no def of that name (a room's fill names a def that furnishes it)",
					placement.stmt.file, placement.stmt.line)
			home = self.program.env_of(macro.file)
			env = {**home, **{k: _arg_value(v, home) for k, v in macro.params.items()}}
			env.update({k: (v if isinstance(v, (int, float)) or k == "__seed__" else _arg_value(v, env)) for k, v in placement.fill["env"].items()})
			saved, self._path = self._path, self._path + ((placement.stmt.file, placement.stmt.line),)
			before = {id(p): len(p.faces) for p in parts}
			room = frame @ Matrix([list(row) for row in placement.matrix])
			try:
				self._run(macro.body, env, parts, room, 1, Prop(macro.name, macro.name, "", {}, macro.body, macro.file, macro.line))
			finally:
				self._path = saved
			self._check_fill(placement, env, _since(parts, before), room)
			return
		faces, _, links = self.piece(placement.piece, placement.stmt)
		where = frame @ Matrix([list(row) for row in placement.matrix])
		_append(parts[-1], faces, where, self._path + ((placement.stmt.file, placement.stmt.line),))
		self.links += _moved(links, where)

	def _check_fill(self, placement, env: dict, faces: list, room) -> None:
		"""Warn when a room's furnishing reaches outside the room: above its ceiling, below its floor, or
		more than a little beyond its walls (a sign on the outside wall is fine; a box on the next floor is not)."""
		if not faces:
			return
		ox, oy, oz = room[0][3], room[1][3], room[2][3]
		points = [(p[0] - ox, p[1] - oy, p[2] - oz) for f in faces for p in f.points]
		w, d, h = env["w"], env["d"], env["h"]
		reach = [("x", min(p[0] for p in points), max(p[0] for p in points), w), ("y", min(p[1] for p in points), max(p[1] for p in points), d),
			("z", min(p[2] for p in points), max(p[2] for p in points), h)]
		out = [f"{axis} {low:.2f}..{high:.2f} m (the room is 0..{size:.2f})" for axis, low, high, size in reach
			if low < -0.35 or high > size + 0.35]
		label = f"{placement.stmt.where()}: fill={placement.piece} ({placement.fill.get('room', '')}, storey {env['level']})"
		if out:
			self.warnings.append(f"{label} reaches outside the room: {', '.join(out)}")
		# The space just inside each door (a metre wide, 0.9 m in, door height) stays clear; rugs and mats may lie in it.
		macro_file = self.program.macro(placement.piece, placement.stmt.file).file
		for side, along in placement.fill.get("doors", ()):
			if side in ("s", "n"):
				zone = (along - .5, along + .5, 0.0 if side == "s" else d - .9, .9 if side == "s" else d)
			else:
				zone = (0.0 if side == "w" else w - .9, .9 if side == "w" else w, along - .5, along + .5)
			blocking = set()
			for f in faces:
				xs, ys, zs = ([p[k] - (ox, oy, oz)[k] for p in f.points] for k in range(3))
				if max(zs) < .05 or min(zs) > 2.0:
					continue
				if min(xs) < zone[1] and max(xs) > zone[0] and min(ys) < zone[3] and max(ys) > zone[2]:
					item = next((o for o in f.origin[len(self._path) + 1:] if o[0] == macro_file), None)
					blocking.add(f"line {item[1]}" if item else "it")
			if blocking:
				self.warnings.append(f"{label}: {', '.join(sorted(blocking))} stands in the doorway on its {side} wall ({along:.2f} m along)")

	def _record_snaps(self, stmt: "Stmt", env: dict, frame, copies: list) -> None:
		for index, matrix in enumerate(copies):
			snap = self._snap(stmt.args, stmt.opts, env, f"_{index + 1}" if len(copies) > 1 else "")
			m = frame @ matrix
			self.snap_sink.append(snap.moved([[m[i][j] for j in range(4)] for i in range(4)]))

	# -- materials
	def register_materials(self) -> None:
		for prop in self.program.props:
			self._scan(prop.body, self.program.env_of(prop.file))
		for macro in self.program.macros.values():
			for value in macro.params.values():
				self._colour(str(value))
			self._scan(macro.body, {**self.program.env_of(macro.file), **macro.params})
		for key, (hex_, finish) in self.colours.items():
			self.host.add_material(key, Mat(key, 2.0, **colour_material_kwargs(hex_, finish)), ("surface", finish, hex_))
		for key, spec in self.signs.items():
			self._register_sign(key, spec)

	def _register_sign(self, key: str, spec: dict) -> None:
		lit = float(spec["lit"])
		self.host.add_material(key, Mat(key, 1.0, emission=key if lit > 0 else "", emission_strength=lit, ao=False), ("sign", spec))

	def _colour(self, token: str) -> None:
		for item in (item for option in pick_options(token) for item in option.split("|")):
			found = colour_key(self.prefix, item) if item.startswith("#") else None
			if found:
				self.colours[found[0]] = (found[1], found[2])

	def _scan(self, body: list, env: dict) -> None:
		for stmt in body:
			if stmt.op == "set":
				env = {**env, **stmt.opts}
			for token in stmt.args + list(stmt.opts.values()):
				for option in pick_options(token):
					self._colour(option)
					for item in option.split("|"):
						value = env.get(item)
						if isinstance(value, str):
							self._colour(value)
			if stmt.op == "sign" and len(stmt.args) >= 4 and not is_dynamic(stmt.args[3] + stmt.opts.get("sub", "")):
				spec = self._sign_spec(stmt)
				self.signs[sign_key(self.prefix, spec)] = spec
			if stmt.block:
				self._scan(stmt.block, env)

	@staticmethod
	def _sign_spec(stmt: Stmt, env: dict | None = None) -> dict:
		def rgb(text: str, default):
			text = str((env or {}).get(text, text)).lstrip("#")  # a colour or a variable holding one
			return tuple(int(text[i:i + 2], 16) for i in (0, 2, 4)) if re.fullmatch(r"[0-9a-fA-F]{6}", text) else default
		# A label and a wrapped sleeve are print on a prop: unlit, on a texture shaped like the panel.
		# A plain sign is a lit shop sign on one line.
		wrapped = "wrap" in stmt.opts
		printed = wrapped or stmt.opts.get("printed") in ("1", "true")
		shape = "256x32"
		if wrapped:
			shape = "256x128"
		elif printed:
			try:
				aspect = float(stmt.args[1]) / max(float(stmt.args[2]), 1e-6)
			except ValueError:
				aspect = 2.0
			shape = "256x32" if aspect >= 6 else "128x32" if aspect >= 3 else "128x64" if aspect >= 1.5 else "64x64"
		tex = stmt.opts.get("tex", shape).split("x")
		text, sub = _unquote(stmt.args[3]), stmt.opts.get("sub", "")
		if env is not None:  # this copy's text: picks chosen, {expressions} worked out
			text, sub = interpolate(text, env), interpolate(sub, env)
		spec = {"text": text, "sub": sub, "bg": rgb(stmt.opts.get("bg", ""), (24, 40, 90)),
			"fg": rgb(stmt.opts.get("fg", ""), (236, 232, 220)), "lit": float(stmt.opts.get("lit", "0" if printed else "1.1")),
			"tex": (int(tex[0]), int(tex[1]))}
		if "wrap" in stmt.opts:
			spec.update(wrapped=True, mark=stmt.opts.get("mark", "bolt"), accent=rgb(stmt.opts.get("accent", ""), spec["fg"]))
		return spec

	def material(self, stmt: Stmt, token: str, env: dict, copy: int) -> str:
		items = material_items(token, env)
		value = items[(copy + int(env.get("__copy__", 0))) % len(items)]
		if value == "none":
			raise _Skip()
		if value.startswith("#"):
			found = colour_key(self.prefix, value)
			if found is None:
				raise PartScriptError(f"colour {value!r}", stmt.file, stmt.line)
			return found[0]
		key = self.host.find_material(value)
		if key is not None:
			return key
		raise PartScriptError(f"unknown material {value!r}", stmt.file, stmt.line)

	# -- build
	def builder(self, prop: Prop, asset_id: str):
		def build():
			if asset_id in self._building:
				raise PartScriptError(f"{asset_id} uses itself (through use)", prop.file, prop.line)
			self._building.add(asset_id)
			saved_path, self._path = self._path, ()  # a used prop's faces start their own origins
			saved_joints, self.joints = self.joints, []  # and its joints stay its own
			scope = self._open_links()
			try:
				parts = [self.new_part(asset_id)]
				if prop.kind == "building":
					self.building_plan(prop)
				self._run(prop.body, {**self.program.env_of(prop.file), "__seed__": prop.opts.get("seed", "")}, parts, _identity(), 0, prop)
			finally:
				self._building.discard(asset_id)
				self._path = saved_path
				self.joints = saved_joints
				self._last_links = self._close_links(scope, parts)
			parts = [p for p in parts if p.faces]
			if not any(p.faces for p in parts):
				raise PartScriptError(f"{asset_id} has no shapes", prop.file, prop.line)
			finish_parts(prop, parts)
			expected = next((s for s in prop.body if s.op == "size" and s.args), None)
			if expected is not None:
				want = self._vec(expected.args[0], self.program.env_of(prop.file))
				lo = [min(p.bounds()[0][k] for p in parts) for k in range(3)]
				hi = [max(p.bounds()[1][k] for p in parts) for k in range(3)]
				got = [h - l for l, h in zip(lo, hi)]
				off = [f"{'xyz'[k]} {got[k]:.2f} m (size says {want[k]:.2f})" for k in range(3) if want[k] > 0 and abs(got[k] - want[k]) > 0.15 * want[k]]
				if off:
					self.warnings.append(f"{prop.file}:{expected.line}: {asset_id}: bounds differ from its size line: {', '.join(off)}")
			return parts[0] if len(parts) == 1 else parts
		return build

	def _run(self, body: list, env: dict, parts: list, frame, depth: int, prop: Prop) -> None:
		if depth > 16:
			raise PartScriptError("use/at nested more than 16 deep (a def that uses itself needs a when= to stop it)", prop.file, prop.line)
		env = dict(env)
		env["__frame__"] = frame  # named shapes (desk.top) are read in this space
		for stmt in body:
			op, a, o = stmt.op, stmt.args, self._placing(stmt)
			try:
				if op == "set":
					for key, value in o.items():
						env[key] = _arg_value(value, env)
					continue
				if op in ("size", "card"):
					continue
				if op in BUILD_OPS:
					if prop.kind != "building":
						raise PartScriptError(f"'{op}' belongs in a building, not a prop", stmt.file, stmt.line)
					if depth == 0:
						for placement in self.building_plan(prop).for_stmt(stmt):
							self.place(placement, parts, frame)
					continue
				if op == "snap":
					if self.snap_sink is not None:
						self._record_snaps(stmt, env, frame, self._copies(stmt, env))
					continue
				if op == "link":
					for index, matrix in enumerate(self._copies(stmt, env)):
						cenv = self._copy_env(stmt, env, index)
						if "when" in o and not evaluate(o["when"], cenv):
							continue
						direction = pb.parse_dir(a[2], lambda text: evaluate(text, cenv))
						end = pb.Snap("link", self._vec(a[1], cenv), direction, pb.default_up(direction), a[0])
						self.links.append(end.moved([[v for v in row] for row in (frame @ matrix)]))
					continue
				if op == "join":
					self.join_rules.append((stmt, dict(env)))
					continue
				if op == "part":
					part = self.new_part(a[0] if a else f"{prop.name}_{len(parts)}")
					part.smooth = o.get("smooth") in ("1", "true")
					parts.append(part)
					continue
				marks = {id(p): len(p.faces) for p in parts} if stmt.name else None
				place = frame @ self._on_matrix(stmt, env) if "on" in o else frame
				copies = self._copies(stmt, env)
				ground = SurfaceIndex([f.points for part in parts for f in part.faces]) if "drop" in o else None
				against = self._against(stmt, env, frame) if "on" in o else None
				for index, matrix in enumerate(copies):
					cenv = self._copy_env(stmt, env, index)
					m = place @ matrix
					cenv["here"] = Bounds("here", (m[0][3], m[1][3], m[2][3]), (m[0][3], m[1][3], m[2][3]), world=True)
					if "when" in o and not evaluate(o["when"], cenv):
						continue
					before = {id(p): len(p.faces) for p in parts}
					self._emit(stmt, cenv, parts, m, depth, prop, index)
					if against is not None:
						_touch(_since(parts, before), *against)
					if ground is not None:
						_drop(_since(parts, before), ground, o["drop"] == "lean",
							self._num(o["sink"], cenv) if "sink" in o else 0.0)
					if self._fallen:
						self._rubble(stmt, cenv, parts, m)
				if stmt.name:
					env[stmt.name] = _bounds(stmt.name, parts, marks)
			except PartScriptError:
				raise
			except (ValueError, KeyError, IndexError, TypeError) as error:
				raise PartScriptError(f"{op}: {error}", stmt.file, stmt.line)

	def _placing(self, stmt: Stmt) -> dict:
		"""A line's options, less any a def it uses takes as its own parameters (use vine drop=.8 hands drop
		to vine rather than dropping it)."""
		if stmt.op != "use" or not stmt.args:
			return stmt.opts
		macro = self.program.macro(stmt.args[0], stmt.file)
		if macro is None or not any(k in macro.params for k in PLACING):
			return stmt.opts
		return {k: v for k, v in stmt.opts.items() if not (k in PLACING and k in macro.params)}

	def _emit(self, stmt: Stmt, env: dict, parts: list, m, depth: int, prop: Prop, index: int) -> None:
		"""One copy of a line placed by m, then reshaped as its options say (twist, bend, wobble, fade...)."""
		o = stmt.opts
		before = {id(p): len(p.faces) for p in parts}
		self._one(stmt, env, parts, m, depth, prop, index)
		if any(k in o for k in ("twist", "bend", "shrink")):
			_deform(_since(parts, before), self._num(o.get("twist", "0"), env),
				self._vec(o["bend"], env, 2) if "bend" in o and "," in o["bend"] else (self._num(o.get("bend", "0"), env), 0.0),
				self._num(o.get("shrink", "1"), env))
		if "wobble" in o:
			_wobble(_since(parts, before), self._vec(o["wobble"], env), env["__seed__"])
		if "fade" in o:
			_fade(_since(parts, before), self._num(o["fade"], env))

	def _on_target(self, stmt: Stmt, env: dict) -> tuple:
		"""on=desk or on=desk.front: (the named shape, the side)."""
		name, _, side = stmt.opts["on"].partition(".")
		target = env.get(name)
		if not isinstance(target, Bounds):
			raise PartScriptError(f"on={stmt.opts['on']}: no shape of that name above this line (name one: {name} = box ...)",
				stmt.file, stmt.line)
		if side not in ("", *SIDES):
			raise PartScriptError(f"on={stmt.opts['on']}: a side is {', '.join(SIDES)} (on={name} is on its top)", stmt.file, stmt.line)
		return target, side or "top"

	def _on_matrix(self, stmt: Stmt, env: dict):
		"""on=desk: the middle of desk's top (or of the side on=desk.front names), where the line's at= is measured from."""
		target, side = self._on_target(stmt, env)
		return Matrix.Translation(target.point(side, env.get("__frame__")))

	def _against(self, stmt: Stmt, env: dict, frame):
		"""on=NAME.SIDE (not the top): the side's plane in prop space, (a point on it, its outward normal)."""
		target, side = self._on_target(stmt, env)
		if side == "top":
			return None
		point = frame @ Vector(target.point(side, env.get("__frame__")))
		normal = Vector(SIDES[side])
		turned = Vector(frame.to_3x3() @ normal).normalized()
		return point, turned

	def _row(self, stmt: Stmt, env: dict, parts: list, m, depth: int, prop: Prop) -> None:
		"""row x|y|z|-x... { lines }: every copy of every line inside set end to end along the axis, gap= apart
		(or spread over= a length), packed from the middle (pack=start / end), and lined up across by align=."""
		a, o = stmt.args, stmt.opts
		axis = a[0] if a else "x"
		if axis.lstrip("+-") not in ("x", "y", "z"):
			raise PartScriptError(f"row {axis}: the axis it runs along, x, y or z (-x runs the other way); stack is row z", stmt.file, stmt.line)
		k, sign = "xyz".index(axis[-1]), (-1.0 if axis.startswith("-") else 1.0)
		local = Matrix.Translation(self._vec(o["at"], env)) if "at" in o else Matrix.Identity(4)
		local = local @ _rot(self._vec(o["r"], env) if "r" in o else (0, 0, 0)).to_4x4()
		frame = m @ local
		inner = {**env, "__frame__": frame}
		items = []
		for child in stmt.block or []:
			if child.op == "set":
				for key, value in child.opts.items():
					inner[key] = _arg_value(value, inner)
				continue
			if child.op in ("part", "snap", "size", "card", "join", "link", "chain") or child.op in BUILD_OPS or child.name:
				what = f"{child.name} = ..." if child.name else f"'{child.op}'"
				raise PartScriptError(f"{what} can't go in a row or stack (name the row itself: books = row x ...)", child.file, child.line)
			place = self._on_matrix(child, inner) if "on" in child.opts else Matrix.Identity(4)
			for index, matrix in enumerate(self._copies(child, inner, still=True)):
				cenv = self._copy_env(child, inner, index)
				if "when" in child.opts and not evaluate(child.opts["when"], cenv):
					continue
				scratch = [self.new_part("row")]
				links, joints = len(self.links), len(self.joints)
				self._emit(child, cenv, scratch, place @ matrix, depth + 1, prop, index)
				faces = [f for p in scratch for f in p.faces]
				if faces:
					points = [q for f in faces for q in f.points]
					lo = tuple(min(q[j] for q in points) for j in range(3))
					hi = tuple(max(q[j] for q in points) for j in range(3))
					items.append((faces, lo, hi, (links, len(self.links)), (joints, len(self.joints))))
		if not items:
			return
		sizes = [hi[k] - lo[k] for _, lo, hi, _, _ in items]
		gap = self._num(o.get("gap", "0"), env)
		if "over" in o and len(items) > 1:
			gap = (self._num(o["over"], env) - sum(sizes)) / (len(items) - 1)
		total = sum(sizes) + gap * (len(items) - 1)
		pack = o.get("pack", "start" if k == 2 else "centre")
		if pack not in ("start", "centre", "center", "end"):
			raise PartScriptError(f"pack={pack}: start, centre or end (where the row sits on its at=)", stmt.file, stmt.line)
		cursor = {"start": 0.0, "end": -total}.get(pack, -total / 2)
		edges = {"left": (0, 0), "right": (0, 1), "front": (1, 0), "back": (1, 1), "bottom": (2, 0), "top": (2, 1)}
		align = [w for w in o.get("align", "").split(",") if w]
		for word in align:
			if word not in edges and word not in ("centre", "center"):
				raise PartScriptError(f"align={word}: left right front back bottom top or centre, joined by commas (align=back,bottom)",
					stmt.file, stmt.line)
		for (faces, lo, hi, links, joints), size in zip(items, sizes):
			shift = [0.0, 0.0, 0.0]
			shift[k] = cursor - lo[k] if sign > 0 else -cursor - hi[k]
			cursor += size + gap
			for word in align:
				for j in ([edges[word][0]] if word in edges else [0, 1, 2]):
					if j != k:
						shift[j] = -(lo[j], hi[j])[edges[word][1]] if word in edges else -(lo[j] + hi[j]) / 2
			where = frame @ Matrix.Translation(shift)
			_append(parts[-1], faces, where, ())
			self.links[links[0]:links[1]] = _moved(self.links[links[0]:links[1]], where)
			self.joints[joints[0]:joints[1]] = _moved(self.joints[joints[0]:joints[1]], where)

	def _copy_env(self, stmt: Stmt, env: dict, index: int) -> dict:
		"""The variables one copy of a line sees: its random seed (the chain of lines and copy numbers
		that led here) and, on a line that makes copies, i = which copy it is."""
		out = {**env, "__seed__": f"{env.get('__seed__', '')}|{stmt.seed()}:{index}"}
		if (stmt.mods or "along" in stmt.opts) and ("i" not in env or env.get("__auto_i__")):
			out["i"], out["__auto_i__"] = index, 1
		if "index" in stmt.opts:
			# as k / as col,row / as col,row,layer: the copy's number by name (a grid's column, row and layer).
			names = stmt.opts["index"].split(",")
			counts = next((_counts(m.group(1), env) for m in map(_MOD.match, stmt.mods) if m and m.group(1)), [index + 1])
			counts = counts + [1] * (3 - len(counts))
			values = [index % counts[0], (index // counts[0]) % counts[1], index // (counts[0] * counts[1])] if len(names) > 1 else [index]
			out.update(zip(names, values))
		return out

	def _one(self, stmt: Stmt, env: dict, parts: list, m, depth: int, prop: Prop, index: int) -> None:
		"""One copy of a shape, use or at line, placed by m."""
		op, a, o = stmt.op, stmt.args, stmt.opts
		if op == "at":
			local = Matrix.Translation(self._vec(a[0], env)) if a else Matrix.Identity(4)
			local = local @ _rot(self._vec(o["r"], env) if "r" in o else (0, 0, 0)).to_4x4()
			if "s" in o:
				local = local @ Matrix.Diagonal((*self._vec(o["s"], env), 1.0))
			self._run(stmt.block or [], env, parts, m @ local, depth + 1, prop)
			return
		if op == "use":
			self._use(stmt, env, parts, m, depth, prop, index)
			return
		if op == "chain":
			self._chain(stmt, env, parts, m)
			return
		if op == "row":
			self._row(stmt, env, parts, m, depth, prop)
			return
		scratch = self.new_part("scratch")
		try:
			self._shape(stmt, env, scratch, index)
		except _Skip:
			return
		_append(parts[-1], scratch.faces, m, self._path)

	def _num(self, text: str, env: dict) -> float:
		return evaluate(text, env)

	def _vec(self, text: str, env: dict, n: int = 3) -> tuple:
		if n == 3 and "," not in text:
			point = anchor_point(text, env)  # desk, desk.top, desk.top_left: a point on a named shape
			if point is not None:
				return point
		parts = split_top(text)
		if len(parts) == 1:
			parts = parts * n
		if len(parts) != n:
			raise ValueError(f"{text!r}: expected {n} numbers")
		return tuple(self._height(p, env) if p.startswith("~") else evaluate(p, env) for p in parts)

	def _height(self, text: str, env: dict) -> float:
		"""on / on(z) / on(NAME) where nothing has a size to sit: the height itself (a group's, a row's)."""
		base = env.get(text[1:])
		if isinstance(base, Bounds):
			return base.number("top", env.get("__frame__"))
		return evaluate(text[1:], env) if text[1:] else 0.0

	def _centre(self, text: str, env: dict, half: tuple) -> tuple:
		"""A position whose ~ components sit the shape on that axis (base + half extent)."""
		if "," not in text:
			point = anchor_point(text, env)
			if point is not None:
				return point
		parts = split_top(text)
		if len(parts) == 1:
			parts = parts * 3
		out = []
		for k, p in enumerate(parts):
			if p.startswith("~"):
				base = env.get(p[1:])
				# on(desk) sits it on desk's top
				low = base.number("top", env.get("__frame__")) if isinstance(base, Bounds) else (evaluate(p[1:], env) if p[1:] else 0.0)
				out.append(low + half[k])
			else:
				out.append(evaluate(p, env))
		return tuple(out)

	def _arc(self, text: str, env: dict) -> list:
		values = [evaluate(v, env) for v in split_top(text)]
		cx, cz, radius, a0, a1 = values[:5]
		steps = int(values[5]) if len(values) > 5 else 12
		return [(cx + radius * math.cos(math.radians(a0 + (a1 - a0) * k / steps)), 0.0,
			cz + radius * math.sin(math.radians(a0 + (a1 - a0) * k / steps))) for k in range(steps + 1)]

	def _path_frames(self, stmt: Stmt, env: dict) -> list:
		"""along="P P P" [every=D] [fit=1] [corners=1] [closed=1]: where copies go along a line of points."""
		o = stmt.opts
		line = str(env.get(o["along"], o["along"]))  # a line of points, or a variable holding one
		points = [self._vec(p, env) if len(split_top(p)) == 3 else (*self._vec(p, env, 2), 0.0) for p in line.split()]
		points = self._smooth(points, o, env)
		flag = lambda key: o.get(key) in ("1", "true")  # noqa: E731
		return path_frames(points, self._num(o["every"], env) if "every" in o else 0.0, flag("fit"), flag("corners"), flag("closed"), flag("joints"))

	def _chain(self, stmt: Stmt, env: dict, parts: list, m) -> None:
		"""chain A B C*3 ...: pieces end to end, each one's start snap on the last one's end snap (a piece
		without them uses its front and back). at= and turn= place the chain; it starts heading +Y."""
		o = stmt.opts
		local = Matrix.Translation(self._vec(o["at"], env)) if "at" in o else Matrix.Identity(4)
		local = local @ _rot(self._vec(o["r"], env) if "r" in o else (0, 0, 0)).to_4x4()
		frame = m @ local
		names = []
		for token in stmt.args:
			name, _, count = token.partition("*")
			names += [name] * (int(self._num(count, env)) if count else 1)
		if not names:
			raise PartScriptError("chain needs pieces: chain path_straight path_curve*2 ...", stmt.file, stmt.line)
		current = pb.Snap("start", (0.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0), "any")
		rows = [[frame[i][j] for j in range(4)] for i in range(4)]
		for name in names:
			faces, declared, links = self.piece(name, stmt)
			snaps = {}
			points = [p for f in faces for p in f.points]
			if points:
				snaps.update(pb.bounds_snaps(tuple(min(p[k] for p in points) for k in range(3)), tuple(max(p[k] for p in points) for k in range(3))))
			snaps.update(declared)
			start, end = snaps.get("start") or snaps["front"], snaps.get("end") or snaps["back"]
			if not pb.kinds_fit(start.kind, current.kind):
				raise PartScriptError(f"chain: {name}'s start is kind {start.kind}, the piece before ends in kind {current.kind}", stmt.file, stmt.line)
			placed = pb.snap_matrix(start, current)
			_append(parts[-1], faces, frame @ Matrix(placed), self._path + ((stmt.file, stmt.line),))
			self.links += _moved(links, frame @ Matrix(placed))
			self.joints.append(current.moved(rows))
			current = end.moved(placed)
		self.joints.append(current.moved(rows))

	# -- links and joins
	def _open_links(self) -> tuple:
		"""Start a build's own links and join rules (the outer build's wait)."""
		scope = (self.links, self.join_rules)
		self.links, self.join_rules = [], []
		return scope

	def _close_links(self, scope: tuple, parts: list) -> list:
		"""Join what this build's join lines can, put the outer build's links back, and return the ends
		left open (in this build's space) for whatever places it."""
		try:
			self._join(parts)
			left = self.links
		finally:
			self.links, self.join_rules = scope
		return left

	def _join(self, parts: list) -> None:
		"""Each join rule bridges pairs of open ends of its kind that nearly meet: nearest pairs first, each
		end once, ends that already touch counted as joined. The bridge curves out of one end and into the
		other (a handrail's return round a stair's turn), or is drawn by with=DEF along +X for length."""
		for stmt, env in self.join_rules:
			kind, o = stmt.args[0], stmt.opts
			reach = self._num(o.get("reach", ".6"), env)
			ends = [k for k, end in enumerate(self.links) if end.kind == kind]
			done: set = set()
			pairs = []
			for n, a in enumerate(ends):
				for b in ends[n + 1:]:
					pa, pb_ = self.links[a].pos, self.links[b].pos
					gap = math.dist(pa, pb_)
					if gap < .005:
						done |= {a, b}
						continue
					towards = [q - p for p, q in zip(pa, pb_)]
					facing = sum(d * t for d, t in zip(self.links[a].dir, towards)) >= -.1 * gap and \
						sum(-d * t for d, t in zip(self.links[b].dir, towards)) >= -.1 * gap
					if gap <= reach and facing:
						pairs.append((gap, a, b))
			for gap, a, b in sorted(pairs):
				if a in done or b in done:
					continue
				done |= {a, b}
				self._bridge(stmt, env, parts, self.links[a], self.links[b], gap)
				self.joints += [self.links[a], self.links[b]]
			self.links = [end for k, end in enumerate(self.links) if k not in done]

	def _bridge(self, stmt: Stmt, env: dict, parts: list, a, b, gap: float) -> None:
		o = stmt.opts
		origin = self._path + ((stmt.file, stmt.line),)
		if "with" in o:
			macro = self.program.macro(o["with"], stmt.file)
			if macro is None:
				raise PartScriptError(f"join with={o['with']}: no def of that name (it draws the bridge along +X, length metres long)",
					stmt.file, stmt.line)
			dx, dy, dz = (q - p for p, q in zip(a.pos, b.pos))
			heading, pitch = math.atan2(dy, dx), math.atan2(dz, math.hypot(dx, dy))
			frame = Matrix.Translation(a.pos) @ Matrix.Rotation(heading, 4, "Z") @ Matrix.Rotation(-pitch, 4, "Y")
			env = {**env, **self.program.env_of(macro.file)}
			inner = {**env, **{k: _arg_value(v, env) for k, v in macro.params.items()}, "length": gap}
			saved, self._path = self._path, origin
			try:
				self._run(macro.body, inner, parts, frame, 1, Prop(macro.name, macro.name, "", {}, macro.body, macro.file, macro.line))
			finally:
				self._path = saved
			return
		radius = self._num(o.get("radius", ".025"), env)
		sides = int(self._num(o.get("s", "6"), env))
		bulge = self._num(o["bulge"], env) if "bulge" in o else max(.06, min(.2, gap * .5))
		material = self.material(stmt, o.get("mat", "steel_dark"), env, 0)
		path = [a.pos, tuple(p + d * bulge for p, d in zip(a.pos, a.dir)), tuple(p + d * bulge for p, d in zip(b.pos, b.dir)), b.pos]
		profile = [(radius * math.cos(math.tau * k / sides), radius * math.sin(math.tau * k / sides)) for k in range(sides)]
		scratch = self.new_part("bridge")
		scratch.sweep(profile, smooth_points(path, 5), material)
		_append(parts[-1], scratch.faces, _identity(), origin)

	def _scatter(self, stmt: Stmt, env: dict, count: int, spec: str) -> list:
		"""count offsets scattered over an area: *N~R (a disc), *N~W,D (a rectangle), *N~W,D,GAP
		(at least GAP apart where it can; R,0,GAP for a disc). Seeded like rand()."""
		values = [self._num(v, env) for v in split_top(spec)]
		width, depth = values[0], (values[1] if len(values) > 1 else 0.0)
		gap = values[2] if len(values) > 2 else 0.0
		disc = depth <= 0
		rng = random.Random(f"{env.get('__seed__', '')}|{stmt.seed()}:scatter")
		out: list = []
		for _ in range(count):
			for _attempt in range(30):
				if disc:
					angle, radius = rng.uniform(0, math.tau), width * math.sqrt(rng.random())
					point = (radius * math.cos(angle), radius * math.sin(angle), 0.0)
				else:
					point = (rng.uniform(-width / 2, width / 2), rng.uniform(-depth / 2, depth / 2), 0.0)
				if all(math.hypot(point[0] - q[0], point[1] - q[1]) >= gap for q in out):
					break
			out.append(point)
		return out

	def _spread(self, stmt: Stmt, env: dict, count: int, spec: str) -> list:
		"""scatter N on NAME [facing up|side|down] [apart G]: N places over the faces of the shape named NAME,
		each turned so its +Z stands out of the face (moss on tops, ivy up walls, rivets on a hull)."""
		name, _, gap = spec.partition(",")
		target = env.get(name)
		if not isinstance(target, Bounds) or not target.faces:
			raise PartScriptError(f"scatter on {name}: no shape of that name above this line (name one: {name} = box ...)", stmt.file, stmt.line)
		facing = stmt.opts.get("facing", "any")
		if facing not in ("up", "down", "side", "any"):
			raise PartScriptError(f"facing={facing}: up, side, down or any (which faces of {name} it grows on)", stmt.file, stmt.line)
		rng = random.Random(f"{env.get('__seed__', '')}|{stmt.seed()}:on")
		found = spread([f.points for f in target.faces], count, rng, facing, self._num(gap, env) if gap else 0.0)
		frame = env.get("__frame__")
		inverse = frame.inverted() if frame is not None else Matrix.Identity(4)
		out = []
		for point, normal in found:
			n = Vector(inverse.to_3x3() @ Vector(normal)).normalized()
			axis = Vector((0.0, 0.0, 1.0)).cross(n)
			if axis.length > 1e-6:
				turn = Matrix.Rotation(math.acos(max(-1.0, min(1.0, n[2]))), 4, axis.normalized())
			else:
				turn = Matrix.Identity(4) if n[2] > 0 else Matrix.Rotation(math.pi, 4, "X")
			out.append(Matrix.Translation(inverse @ Vector(point)) @ turn)
		return out

	def _rubble(self, stmt: Stmt, env: dict, parts: list, m) -> None:
		"""The chunks a broken box lost, some of them (rubble=share) as rubble fallen round its foot: each a
		lump of the core material, tumbled, dropped onto whatever is there (and onto each other)."""
		(local, size, fallen), self._fallen = self._fallen[0], []
		share = self._num(stmt.opts.get("rubble", "0"), env)
		if share <= 0 or not fallen:
			return
		o = stmt.opts
		m = m @ local  # the box's own space: its middle at the origin
		rng = random.Random(f"{env.get('__seed__', '')}|rubble")
		pieces = rng.sample(fallen, min(len(fallen), max(1, round(len(fallen) * min(share, 1.0))), 120))
		material = self.material(stmt, o.get("core", stmt.args[2]), env, 0)
		ground = SurfaceIndex([f.points for part in parts for f in part.faces])
		for (x, y, z), cell in pieces:
			side = rng.choice((-1, 1))
			spot = (x + rng.uniform(-.3, .3) * size[2], side * (size[1] / 2 + rng.uniform(.05, .3 + size[2] * .35)), -size[2] / 2 + .4)
			lump = self.new_part("rubble")
			lump.push(spot, (rng.uniform(0, math.tau), rng.uniform(0, math.tau), rng.uniform(0, math.tau)))
			lump.box((0, 0, 0), tuple(c * rng.uniform(.45, .8) for c in cell), material)
			lump.pop()
			_wobble(lump.faces, tuple(c * .12 for c in cell), f"{env.get('__seed__', '')}|{x},{y},{z}")
			faces = [Face([m @ q for q in f.points], f.material, f.uv, f.shade, f.corner_shade, f.group, f.tiled, self._path) for f in lump.faces]
			_drop(faces, ground, True)
			parts[-1].faces.extend(faces)

	def _copies(self, stmt: Stmt, env: dict, still: bool = False) -> list:
		"""Where each copy goes. still: repeat N without every= makes N copies in one place (a row lays them out)."""
		mats = [Matrix.Identity(4)]
		for mod in stmt.mods:
			match = _MOD.match(mod)
			if match and match.group(1):
				counts = _counts(match.group(1), env)
				if len(counts) == 1:
					step = self._vec(match.group(2), env) if match.group(2) else ((0, 0, 0) if still else (1, 0, 0))
					offsets = [tuple(s * k for s in step) for k in range(counts[0])]
					# hang=S: the row sags S at its middle, like things strung on a wire.
					hang = self._num(stmt.opts["hang"], env) if "hang" in stmt.opts else 0.0
					if hang and counts[0] > 1:
						n = counts[0]
						offsets = [(x, y, z - hang * (1.0 - (2.0 * k / (n - 1) - 1.0) ** 2)) for k, (x, y, z) in enumerate(offsets)]
				else:
					step = self._vec(match.group(2), env, 3) if match.group(2) and len(split_top(match.group(2))) == 3 else (
						*self._vec(match.group(2), env, 2), 0.0) if match.group(2) else ((0, 0, 0) if still else (1, 1, 1))
					counts = counts + [1] * (3 - len(counts))
					offsets = [(i * step[0], j * step[1], k * step[2]) for k in range(counts[2]) for j in range(counts[1]) for i in range(counts[0])]
				mats = [Matrix.Translation(off) @ m for off in offsets for m in mats]
			elif match and match.group(3):
				n, deg = _counts(match.group(3), env)[0], self._num(match.group(4), env)
				mats = [Matrix.Rotation(math.radians(deg * k), 4, "Z") @ m for k in range(n) for m in mats]
			elif match and match.group(5):
				mats = [Matrix.Translation(off) @ m for off in self._scatter(stmt, env, _counts(match.group(5), env)[0], match.group(6)) for m in mats]
			elif match and match.group(7):
				mats = [spot @ m for spot in self._spread(stmt, env, _counts(match.group(7), env)[0], match.group(8)) for m in mats]
			elif mod in ("mx", "my", "mz"):
				axis = "xyz".index(mod[1])
				scale = [1.0, 1.0, 1.0]
				scale[axis] = -1.0
				mirror = Matrix.Diagonal((*scale, 1.0))
				mats = mats + [mirror @ m for m in mats]
		if "along" in stmt.opts:
			placed = []
			for frame in self._path_frames(stmt, env):
				move = Matrix.Translation(frame.pos) @ Matrix.Rotation(math.radians(frame.yaw), 4, "Z")
				if frame.stretch != 1.0:
					move = move @ Matrix.Diagonal((frame.stretch, 1.0, 1.0, 1.0))
				placed += [move @ m for m in mats]
			mats = placed
		if "jit" in stmt.opts:
			values = [self._num(v, env) for v in split_top(stmt.opts["jit"])]
			amount, spin = values[0], (values[1] if len(values) > 1 else 0.0)
			rng = random.Random(f"{env.get('__seed__', '')}{stmt.seed()}")
			mats = [Matrix.Translation((rng.uniform(-amount, amount), rng.uniform(-amount, amount), 0.0)) @ m
				@ Matrix.Rotation(math.radians(rng.uniform(-spin, spin)), 4, "Z") for m in mats]
		return mats

	def _shape(self, stmt: Stmt, env: dict, p, copy: int) -> None:
		op, a, o = stmt.op, stmt.args, stmt.opts
		rot = tuple(math.radians(v) for v in self._vec(o["r"], env)) if "r" in o else (0, 0, 0)
		mat = lambda token: self.material(stmt, token, env, copy)  # noqa: E731
		taper = self._vec(o["taper"], env, 2) if "taper" in o and op in ("b", "bb") else (1.0, 1.0)
		lean = self._vec(o["lean"], env, 2) if "lean" in o and op in ("b", "bb") else (0.0, 0.0)
		if op in ("b", "bb", "c", "cone") and "from" in o and "to" in o:
			self._between(stmt, env, p, mat, rot)
		elif op == "b" and "break" in o:
			# a box with chunks knocked out of it (break= the share gone, chunk= their size, core= what shows inside)
			size = self._vec(a[1], env)
			centre = self._centre(a[0], env, tuple(v / 2 for v in size))
			p.push(centre, rot)
			chunk = self._num(o["chunk"], env) if "chunk" in o else max(.15, min(size), max(size) / 14)
			fallen = broken_box(p, size, mat(a[2]), self._num(o["break"], env), chunk, env.get("__seed__", ""),
				mat(o["core"]) if "core" in o else None)
			p.pop()
			self._fallen = [(Matrix.Translation(centre) @ _rot(tuple(math.degrees(v) for v in rot)).to_4x4(), size, fallen)]
		elif op == "b":
			size = self._vec(a[1], env)
			skip = tuple(s for s in o.get("skip", "").split(",") if s)
			p.box(self._centre(a[0], env, tuple(s / 2 for s in size)), size, mat(a[2]), rotation=rot, skip=skip,
				shade=float(o.get("shade", 1.0)), taper=taper, lean=lean)
		elif op == "bx":
			p.box_minmax(self._vec(a[0], env), self._vec(a[1], env), mat(a[2]))
		elif op == "bb":
			size = self._vec(a[1], env)
			p.bevel_box(self._centre(a[0], env, tuple(s / 2 for s in size)), size, self._num(a[2], env), mat(a[3]), rotation=rot,
				taper=taper, lean=lean)
		elif op == "ext":
			outline = [tuple(evaluate(v, env) for v in point.split(":")) for point in a[3:]]
			p.extrude(self._vec(a[0], env), outline, self._num(a[1], env), mat(a[2]), axis=o.get("ax", "x"),
				taper=self._num(o["taper"], env) if "taper" in o else 1.0, rotation=rot)
		elif op in ("c", "cone", "tube"):
			axis = o.get("ax", "z")
			radius = self._num(a[1], env)
			if op == "tube":
				inner, height, material = self._num(a[2], env), self._num(a[3], env), mat(a[4])
			else:
				height, material = self._num(a[2], env), mat(a[3])
			extent = {"x": (height / 2, radius, radius), "y": (radius, height / 2, radius), "z": (radius, radius, height / 2)}[axis]
			centre = self._centre(a[0], env, extent)
			axis_rot = {"x": (0, math.pi / 2, 0), "y": (math.pi / 2, 0, 0), "z": (0, 0, 0)}[axis]
			p.push(centre, rot)
			p.push((0, 0, 0), axis_rot)
			sides = int(self._num(o.get("s", "12" if op == "tube" else "10" if op == "c" else "8"), env))
			if op == "tube":
				h = height / 2
				p.lathe([(inner, -h), (radius, -h), (radius, h), (inner, h), (inner, -h)], material, sides)
			elif op == "cone":
				p.cone((0, 0, 0), radius, height, material, sides)
			else:
				arc = tuple(math.radians(v) for v in self._vec(o["arc"], env, 2)) if "arc" in o else (0.0, math.tau)
				p.cylinder((0, 0, 0), radius, height, material, sides, radius_top=self._num(o["rt"], env) if "rt" in o else None,
					caps=o.get("caps") != "0", cap_material=mat(o["capm"]) if "capm" in o else None, arc=arc)
			p.pop()
			p.pop()
		elif op == "sph":
			radius = self._num(a[1], env)
			rings = int(self._num(o.get("rings", "6"), env))
			profile = [(radius * math.sin(math.pi * k / rings), -radius * math.cos(math.pi * k / rings)) for k in range(rings + 1)]
			p.lathe(profile, mat(a[2]), int(self._num(o.get("s", "10"), env)), center=self._centre(a[0], env, (radius,) * 3), rotation=rot)
		elif op == "wedge":
			w, d, h = self._vec(a[1], env)
			cx, cy, cz = self._centre(a[0], env, (w / 2, d / 2, h / 2))
			material = mat(a[2])
			p.push((cx, cy, cz), rot)
			x, y, z = w / 2, d / 2, h / 2
			p.face([(-x, -y, -z), (x, -y, -z), (x, y, -z), (-x, y, -z)][::-1], material)  # bottom
			p.face([(-x, y, -z), (-x, y, z), (x, y, z), (x, y, -z)], material)  # back (+Y)
			p.face([(-x, -y, -z), (x, -y, -z), (x, y, z), (-x, y, z)], material)  # slope, facing front and up
			p.face([(-x, -y, -z), (-x, y, z), (-x, y, -z)], material)  # -X side
			p.face([(x, -y, -z), (x, y, -z), (x, y, z)], material)  # +X side
			p.pop()
		elif op == "lathe":
			profile = [tuple(evaluate(v, env) for v in point.split(":")) for point in a[2:]]
			arc = tuple(math.radians(v) for v in self._vec(o["arc"], env, 2)) if "arc" in o else (0.0, math.tau)
			p.lathe(profile, mat(a[1]), int(self._num(o.get("s", "16"), env)), center=self._vec(a[0], env), arc=arc,
				cap=o.get("cap") in ("1", "true"), rotation=rot)
		elif op == "pipe":
			radius = self._num(a[1], env)
			sides = int(self._num(o.get("s", "6"), env))
			profile = [(radius * math.cos(math.tau * k / sides), radius * math.sin(math.tau * k / sides)) for k in range(sides)]
			points = self._arc(o["arc"], env) if "arc" in o else [self._vec(t, env) for t in expand_points(a[2:], env)]
			points = self._smooth(points, o, env)
			p.sweep(profile, points, mat(a[0]), closed_path=o.get("closed") in ("1", "true"), up=(0, 1, 0) if "arc" in o else (0, 0, 1),
				scales=_taper(points, self._num(o["taper"], env)) if "taper" in o else None)
		elif op == "sweep":
			profile = [tuple(evaluate(v, env) for v in point.split(":")) for point in o["prof"].split(",")]
			points = self._arc(o["arc"], env) if "arc" in o else [self._vec(t, env) for t in expand_points(a[1:], env)]
			points = self._smooth(points, o, env)
			p.sweep(profile, points, mat(a[0]), closed_path=o.get("closed") in ("1", "true"),
				closed_profile=o.get("open") not in ("1", "true"), up=(0, 1, 0) if "arc" in o else (0, 0, 1),
				scales=_taper(points, self._num(o["taper"], env)) if "taper" in o else None)
		elif op == "terrain":
			self._terrain(stmt, env, p, mat)
		elif op == "archwall":
			cx, cy, cz = self._vec(a[0], env)
			width, height, thick = self._num(a[1], env), self._num(a[2], env), self._num(a[3], env)
			material = mat(a[4])
			span = self._num(o["open"], env) if "open" in o else width
			spring = self._num(o["spring"], env) if "spring" in o else height - span / 2
			steps = int(self._num(o.get("s", "12"), env))
			hw, ho = width / 2, span / 2
			y0, y1 = cy, cy + thick
			arc = [(cx - ho * math.cos(math.pi * k / steps), cz + spring + ho * math.sin(math.pi * k / steps)) for k in range(steps + 1)]
			top = cz + height
			for y, flip in ((y0, False), (y1, True)):
				quads = []
				if hw - ho > 0.001:
					quads.append([(cx - hw, cz), (cx - ho, cz), (cx - ho, top), (cx - hw, top)])
					quads.append([(cx + ho, cz), (cx + hw, cz), (cx + hw, top), (cx + ho, top)])
				for (x0, z0), (x1, z1) in zip(arc, arc[1:]):
					quads.append([(x0, z0), (x1, z1), (x1, top), (x0, top)])
				for quad in quads:
					points = [(x, y, z) for x, z in quad]
					# The front (y0) faces -Y; the back faces +Y.
					p.face(points[::-1] if flip else points, material)
			for (x0, z0), (x1, z1) in zip(arc, arc[1:]):
				p.face([(x0, y0, z0), (x0, y1, z0), (x1, y1, z1), (x1, y0, z1)], material)  # the soffit
			for x, sign in ((cx - ho, 1), (cx + ho, -1)):
				jamb = [(x, y0, cz), (x, y1, cz), (x, y1, cz + spring), (x, y0, cz + spring)]
				p.face(jamb if sign > 0 else jamb[::-1], material)
			p.face([(cx - hw, y0, top), (cx + hw, y0, top), (cx + hw, y1, top), (cx - hw, y1, top)], material)
			for x, sign in ((cx - hw, -1), (cx + hw, 1)):
				end = [(x, y0, cz), (x, y0, top), (x, y1, top), (x, y1, cz)]
				p.face(end if sign < 0 else end[::-1], material)
		elif op == "vault":
			cx, cy, cz = self._vec(a[0], env)
			span, depth, rise = self._num(a[1], env), self._num(a[2], env), self._num(a[3], env)
			material = mat(a[4])
			steps = int(self._num(o.get("s", "12"), env))
			half, y0, y1 = span / 2, cy - depth / 2, cy + depth / 2
			ring = [(cx - half * math.cos(math.pi * k / steps), cz + rise * math.sin(math.pi * k / steps)) for k in range(steps + 1)]
			for (x0, z0), (x1, z1) in zip(ring, ring[1:]):
				# Wound to face down and in: the underside is what shows.
				p.face([(x0, y0, z0), (x0, y1, z0), (x1, y1, z1), (x1, y0, z1)], material)
		elif op == "face":
			points = [self._vec(t, env) for t in a[1:]]
			p.face(points, mat(a[0]))
			if o.get("double") in ("1", "true"):
				# The back, nudged 1 mm along its normal so to_object's dedupe keeps both.
				v = [Vector(q) for q in points]
				n = (v[1] - v[0]).cross(v[2] - v[0])
				n = n.normalized() * 0.001 if n.length > 1e-9 else Vector((0, 0, 0))
				p.face([tuple(q - n) for q in reversed(v)], mat(a[0]))
		elif op == "pan":
			p.panel(self._vec(a[0], env), self._num(a[1], env), self._num(a[2], env), mat(a[3]), rotation=rot,
				double=o.get("double") in ("1", "true"))
		elif op == "trim":
			p.trim(self._vec(a[0], env), self._num(a[1], env), self._num(a[2], env), a[3], self.host.atlases(), rotation=rot,
				material=mat(o["m"]) if "m" in o else None)
		elif op == "sign":
			spec = self._sign_spec(stmt, env)
			key = sign_key(self.prefix, spec)
			if key not in self.host.materials:
				self._register_sign(key, spec)
			if "wrap" in o:
				p.wrapped_panel(self._vec(a[0], env), self._num(o["wrap"], env), self._num(a[2], env), key,
					sides=int(self._num(o.get("sides", "12"), env)), rotation=rot)
			else:
				p.panel(self._vec(a[0], env), self._num(a[1], env), self._num(a[2], env), key, rotation=rot, double=o.get("double") in ("1", "true"))
		elif op == "torus":
			# A ring: a tube of radius thick bent round a circle of radius radius, lying flat (axis=x/y stands it up).
			radius, thick = self._num(a[1], env), self._num(a[2], env)
			sides, rings = int(self._num(o.get("s", "16"), env)), int(self._num(o.get("rings", "6"), env))
			profile = [(thick * math.cos(math.tau * k / rings), thick * math.sin(math.tau * k / rings)) for k in range(rings)]
			ring = [(radius * math.cos(math.tau * k / sides), radius * math.sin(math.tau * k / sides), 0.0) for k in range(sides)]
			axis_rot = {"x": (0, math.pi / 2, 0), "y": (math.pi / 2, 0, 0), "z": (0, 0, 0)}[o.get("ax", "z")]
			outer = radius + thick
			half = {"z": (outer, outer, thick), "x": (thick, outer, outer), "y": (outer, thick, outer)}[o.get("ax", "z")]
			p.push(self._centre(a[0], env, half), rot)
			p.push((0, 0, 0), axis_rot)
			p.sweep(profile, ring, mat(a[3]), closed_path=True)
			p.pop()
			p.pop()
		elif op == "frame":
			# A panel with a hole: width x height facing the front (-Y), depth thick, the hole hole=w,h
			# (default 70 %) centred, or moved by hole_at=x,z; picture frames, window and door frames, hatches.
			width, height, depth = self._num(a[1], env), self._num(a[2], env), self._num(a[3], env)
			material = mat(a[4])
			hw, hh = self._vec(o["hole"], env, 2) if "hole" in o else (width * .7, height * .7)
			hx, hz = self._vec(o["hole_at"], env, 2) if "hole_at" in o else (0.0, 0.0)
			p.push(self._centre(a[0], env, (width / 2, depth / 2, height / 2)), rot)
			left, right, bottom, top = -width / 2, width / 2, -height / 2, height / 2
			h0, h1, v0, v1 = hx - hw / 2, hx + hw / 2, hz - hh / 2, hz + hh / 2
			# Side bars full height; the bars above and below the hole fit between them (no hidden end faces).
			for (x0, x1, z0, z1, skip) in ((left, h0, bottom, top, ()), (h1, right, bottom, top, ()), (h0, h1, bottom, v0, ("-x", "+x")),
					(h0, h1, v1, top, ("-x", "+x"))):
				if x1 - x0 > 1e-6 and z1 - z0 > 1e-6:
					p.box(((x0 + x1) / 2, 0, (z0 + z1) / 2), (x1 - x0, depth, z1 - z0), material,
						skip=skip if x0 > left + 1e-6 and x1 < right - 1e-6 else ())
			p.pop()
		else:
			raise PartScriptError(f"unknown shape '{op}'", stmt.file, stmt.line)

	def _terrain(self, stmt: Stmt, env: dict, p, mat) -> None:
		"""terrain at=C size=W,D mat=M height=EXPR [cells=N[,M]] [steep=M slope=35] [skirt=0]: ground W x D whose
		height at each corner of a grid is EXPR (x and y are the corner's place, from the middle), in flat
		triangles; faces steeper than slope degrees take steep=, and the sides go down to C's height."""
		a, o = stmt.args, stmt.opts
		cx, cy, cz = self._vec(a[0], env)
		width, depth = self._vec(a[1], env, 2) if "," in a[1] else (self._num(a[1], env),) * 2
		counts = [int(self._num(v, env)) for v in split_top(o.get("cells", "16"))]
		nx, ny = (counts[0], counts[-1])
		if not (1 <= nx <= 96 and 1 <= ny <= 96):
			raise PartScriptError("terrain cells=N or N,M: 1-96 a side", stmt.file, stmt.line)
		ground, steep = mat(a[2]), mat(o["steep"]) if "steep" in o else None
		limit = math.cos(math.radians(self._num(o.get("slope", "35"), env)))
		height = o.get("height", "0")
		grid = []
		for j in range(ny + 1):
			row = []
			for i in range(nx + 1):
				x, y = -width / 2 + width * i / nx, -depth / 2 + depth * j / ny
				row.append((cx + x, cy + y, cz + evaluate(height, {**env, "x": x, "y": y})))
			grid.append(row)
		for j in range(ny):
			for i in range(nx):
				a00, a10, a11, a01 = grid[j][i], grid[j][i + 1], grid[j + 1][i + 1], grid[j + 1][i]
				pair = ((a00, a10, a11), (a00, a11, a01)) if (i + j) % 2 == 0 else ((a00, a10, a01), (a10, a11, a01))
				for tri in pair:
					u = [tri[1][k] - tri[0][k] for k in range(3)]
					v = [tri[2][k] - tri[0][k] for k in range(3)]
					nz = u[0] * v[1] - u[1] * v[0]
					length = math.sqrt((u[1] * v[2] - u[2] * v[1]) ** 2 + (u[2] * v[0] - u[0] * v[2]) ** 2 + nz ** 2) or 1.0
					p.face(list(tri), steep if steep and nz / length < limit else ground)
		if o.get("skirt", "1") not in ("0", "false"):
			side = steep or ground
			edges = [grid[0], [row[-1] for row in grid], grid[-1][::-1], [row[0] for row in grid][::-1]]
			for edge in edges:
				for q0, q1 in zip(edge, edge[1:]):
					p.face([(q0[0], q0[1], cz), (q1[0], q1[1], cz), q1, q0], side)
			p.face([(cx - width / 2, cy - depth / 2, cz), (cx - width / 2, cy + depth / 2, cz), (cx + width / 2, cy + depth / 2, cz),
				(cx + width / 2, cy - depth / 2, cz)], side)

	def _between(self, stmt: Stmt, env: dict, p, mat, rot: tuple) -> None:
		"""box/bevel_box/cylinder/cone from=A to=B: a beam or rod from one point to the other. A box's size
		is across it (y,z: its top stays up), a cylinder's radius= too; a cone points at B."""
		op, a, o = stmt.op, stmt.args, stmt.opts
		start, end = self._vec(o["from"], env), self._vec(o["to"], env)
		length = math.dist(start, end)
		if length < 1e-6:
			raise PartScriptError(f"{op} from= and to= are the same point", stmt.file, stmt.line)
		if op in ("b", "bb"):
			size = self._vec(a[1], env)
			p.push_matrix(_aim(start, end, "x"))
			if op == "b":
				p.box((0, 0, 0), (length, size[1], size[2]), mat(a[2]), rotation=rot, taper=self._vec(o["taper"], env, 2) if "taper" in o else (1, 1))
			else:
				p.bevel_box((0, 0, 0), (length, size[1], size[2]), self._num(a[2], env), mat(a[3]), rotation=rot)
			p.pop()
			return
		radius = self._num(a[1], env)
		p.push_matrix(_aim(start, end, "z"))
		p.push((0, 0, 0), rot)
		if op == "cone":
			p.cone((0, 0, 0), radius, length, mat(a[3]), int(self._num(o.get("s", "8"), env)))
		else:
			p.cylinder((0, 0, 0), radius, length, mat(a[3]), int(self._num(o.get("s", "10"), env)),
				radius_top=self._num(o["rt"], env) if "rt" in o else None, caps=o.get("caps") != "0",
				cap_material=mat(o["capm"]) if "capm" in o else None)
		p.pop()
		p.pop()

	def _smooth(self, points: list, o: dict, env: dict) -> list:
		"""smooth=N: a curve through the points, N pieces between each pair."""
		if "smooth" not in o:
			return points
		return smooth_points(points, int(self._num(o["smooth"], env)), o.get("closed") in ("1", "true"))

	def _use(self, stmt: Stmt, env: dict, parts: list, matrix, depth: int, prop: Prop, copy: int) -> None:
		a, o = stmt.args, stmt.opts
		name = a[0]
		local = Matrix.Translation(self._vec(a[1], env)) if len(a) > 1 else Matrix.Identity(4)
		length = None
		placing = self._placing(stmt)
		if "from" in placing and "to" in placing:
			# use NAME from=A to=B: its +X runs from A toward B, and a def gets length (how far that is).
			start, end = self._vec(o["from"], env), self._vec(o["to"], env)
			length = math.dist(start, end)
			aim = _aim(start, end, "x") if length > 1e-9 else Matrix.Translation(start)
			local = Matrix.Translation(start) @ Matrix(aim.to_3x3()).to_4x4()
		local = local @ _rot(self._vec(o["r"], env) if "r" in o else (0, 0, 0)).to_4x4()
		if "s" in o:
			local = local @ Matrix.Diagonal((*self._vec(o["s"], env), 1.0))
		macro = self.program.macro(name, stmt.file)
		if macro is not None:
			env = {**env, **self.program.env_of(macro.file)}  # a def sees its own file's variables
			inner = {**env, **{k: _arg_value(v, env) for k, v in macro.params.items()}, "__copy__": copy + int(env.get("__copy__", 0))}
			if length is not None:
				inner["length"] = length
			for key, value in o.items():
				if key not in USE_OPTIONS and (key not in PLACING or key in macro.params):
					inner[key] = _arg_value(value, env)
			saved, self._path = self._path, self._path + ((stmt.file, stmt.line),)
			try:
				self._run(macro.body, inner, parts, matrix @ local, depth + 1, prop)
			finally:
				self._path = saved
			return
		faces = self._foreign_faces(name, stmt)
		_append(parts[-1], faces, matrix @ local, self._path + ((stmt.file, stmt.line),))
		self.links += _moved(self._foreign_links.get(self._key(name, stmt.file), []), matrix @ local)

	def _foreign_faces(self, name: str, stmt: Stmt) -> list:
		"""Faces of another prop: one of the program's (built whole), else the host's (Host.foreign_parts)."""
		used, name = name, self._key(name, stmt.file)
		if name in self._foreign:
			return self._foreign[name]
		saved, self.snap_sink = self.snap_sink, None  # a used prop's snaps are its own, not the user's
		try:
			prop = self.program_prop(name)
			if prop is not None and prop.kind == "prop":
				built = self.builder(prop, self.host.asset_id(prop.name))()
				result = built if isinstance(built, list) else [built]
				self._foreign_links[name] = self._last_links
			else:
				result = self.host.foreign_parts(name)
		finally:
			self.snap_sink = saved
		if result is None:
			if stmt.called:
				raise PartScriptError(f"unknown statement '{used}' (not a shape, a def, a std part or a prop; partscript ref lists the shapes)",
					stmt.file, stmt.line)
			raise PartScriptError(f"'use {used}': no def, std part or prop of that name", stmt.file, stmt.line)
		faces = [f for part in result for f in part.faces]
		self._foreign[name] = faces
		return faces



def _moved(links: list, matrix) -> list:
	"""Link ends carried into the space a matrix places them in."""
	rows = [[matrix[i][j] for j in range(4)] for i in range(4)]
	return [end.moved(rows) for end in links]


PLACING = ("on", "drop", "sink", "from", "to", "facing")  # options that place a line, unless a def it uses takes them itself


def _identity():
	return Matrix.Identity(4)


SIDES = {"top": (0, 0, 1), "bottom": (0, 0, -1), "front": (0, -1, 0), "back": (0, 1, 0), "left": (-1, 0, 0), "right": (1, 0, 0)}


def _touch(faces: list, point, normal) -> None:
	"""Move what one copy made along normal until its nearest point lies on the plane (point, normal):
	it stands against that side, outside it."""
	points = [q for f in faces for q in f.points]
	if not points:
		return
	nearest = min((Vector(q) - point).dot(normal) for q in points)
	move = Matrix.Translation(tuple(-nearest * v for v in normal))
	for f in faces:
		f.points = [move @ q for q in f.points]


def _since(parts: list, marks: dict) -> list:
	"""The faces added to parts since marks (part id -> face count); parts new since then count whole."""
	return [f for part in parts for f in part.faces[marks.get(id(part), 0):]]


def _bounds(name: str, parts: list, marks: dict) -> Bounds:
	"""The box the faces added since marks (part id -> face count) fill, as the shape named name."""
	faces = [f for part in parts for f in part.faces[marks.get(id(part), 0):]]
	points = [q for f in faces for q in f.points]
	if not points:
		return Bounds(name, None, None)
	return Bounds(name, tuple(min(q[k] for q in points) for k in range(3)), tuple(max(q[k] for q in points) for k in range(3)), faces)


def _drop(faces: list, ground: SurfaceIndex, lean: bool, sink: float = 0.0) -> None:
	"""Let what one copy made fall (or rise, if it is set inside the ground) onto the surfaces under it,
	resting on the highest point it touches, then sink that far into it; lean tilts it to the ground
	there. It is then ground itself, for what falls after it."""
	points = [q for f in faces for q in f.points]
	if not points:
		return
	bottom, top = min(q[2] for q in points), max(q[2] for q in points)
	low = [q for q in points if q[2] <= bottom + max(.02, (top - bottom) * .1)]
	cx, cy = sum(q[0] for q in low) / len(low), sum(q[1] for q in low) / len(low)
	samples = {(round(q[0], 4), round(q[1], 4)) for q in low} | {(cx, cy)}
	hits = [hit for hit in (ground.below(x, y, top) for x, y in samples) if hit is not None]
	if not hits:
		hits = [(0.0, (0.0, 0.0, 1.0))]  # nothing there: the prop's floor
	if hits:
		rest = max(z for z, _ in hits) - sink
		move = Matrix.Translation((0.0, 0.0, rest - bottom))
		centre = ground.below(cx, cy, top)
		if lean and centre is not None:
			n = Vector(centre[1])
			axis = Vector((0.0, 0.0, 1.0)).cross(n)
			if axis.length > 1e-6:
				pivot = (cx, cy, rest)
				turn = Matrix.Rotation(math.acos(max(-1.0, min(1.0, n[2]))), 4, axis.normalized())
				move = Matrix.Translation(pivot) @ turn @ Matrix.Translation(tuple(-v for v in pivot)) @ move
		for f in faces:
			f.points = [move @ q for q in f.points]
	for f in faces:
		ground.add(f.points)


def _taper(points: list, end: float) -> list:
	"""Sizes along a line of points, 1 at the first and end at the last, by distance along it."""
	run = [0.0]
	for a, b in zip(points, points[1:]):
		run.append(run[-1] + math.dist(a, b))
	total = run[-1] or 1.0
	return [1.0 + (end - 1.0) * d / total for d in run]


def _aim(start, end, axis: str):
	"""A frame at the middle of start-end whose axis (x or z) runs from start to end, its other axes
	kept level (a beam's top faces up; straight up, its top faces +Y)."""
	d = [e - s for s, e in zip(start, end)]
	length = math.sqrt(sum(v * v for v in d))
	d = [v / length for v in d]
	up = (0.0, 1.0, 0.0) if abs(d[2]) > .999 else (0.0, 0.0, 1.0)
	y = [up[1] * d[2] - up[2] * d[1], up[2] * d[0] - up[0] * d[2], up[0] * d[1] - up[1] * d[0]]
	n = math.sqrt(sum(v * v for v in y))
	y = [v / n for v in y]
	z = [d[1] * y[2] - d[2] * y[1], d[2] * y[0] - d[0] * y[2], d[0] * y[1] - d[1] * y[0]]
	columns = (d, y, z) if axis == "x" else (y, z, d)
	mid = [(s + e) / 2 for s, e in zip(start, end)]
	return Matrix([[columns[0][r], columns[1][r], columns[2][r], mid[r]] for r in range(3)] + [[0.0, 0.0, 0.0, 1.0]])


def _rot(degrees):
	return Euler(tuple(math.radians(v) for v in degrees), "XYZ").to_matrix()


def _deform(faces: list, twist: float, bend, shrink: float) -> None:
	"""Bend what a line made as a whole, by height from its lowest point to its highest, about its middle:
	twist turns it (degrees at the top), shrink scales it across (1 = not at all, .5 = half size at the
	top), bend curves it over (degrees at the top, toward +Y, or bend=deg,heading to lean another way)."""
	points = [p for f in faces for p in f.points]
	if not points:
		return
	xs, ys, zs = ([p[k] for p in points] for k in range(3))
	cx, cy, bottom = (min(xs) + max(xs)) / 2, (min(ys) + max(ys)) / 2, min(zs)
	height = max(max(zs) - bottom, 1e-6)
	angle, heading = math.radians(bend[0]), math.radians(bend[1])
	radius = height / angle if abs(angle) > 1e-9 else None
	moved: dict = {}
	for f in faces:
		out = []
		for p in f.points:
			key = (round(p[0], 6), round(p[1], 6), round(p[2], 6))
			if key not in moved:
				t = (p[2] - bottom) / height
				x, y, z = p[0] - cx, p[1] - cy, p[2]
				if twist:
					a = math.radians(twist * t)
					x, y = x * math.cos(a) - y * math.sin(a), x * math.sin(a) + y * math.cos(a)
				if shrink != 1.0:
					k = 1.0 + (shrink - 1.0) * t
					x, y = x * k, y * k
				if radius is not None:
					# Into the bend's frame (its lean is along +Y there), curve round a circle, and back.
					u, v = x * math.cos(-heading) - y * math.sin(-heading), x * math.sin(-heading) + y * math.cos(-heading)
					theta = angle * t
					v, z = radius - (radius - v) * math.cos(theta), bottom + (radius - v) * math.sin(theta)
					x, y = u * math.cos(heading) - v * math.sin(heading), u * math.sin(heading) + v * math.cos(heading)
				moved[key] = Vector((x + cx, y + cy, z))
			out.append(moved[key])
		f.points = out


def _wobble(faces: list, amount, seed: str) -> None:
	"""Move every corner by a random offset up to amount (x, y, z), drawn for this copy from where the
	corner is, so corners that faces share move together and the surface stays closed."""
	moved: dict = {}
	for f in faces:
		points = []
		for p in f.points:
			key = (round(p[0], 4), round(p[1], 4), round(p[2], 4))
			if key not in moved:
				rng = random.Random(f"{seed}~{key}")
				moved[key] = Vector([p[k] + rng.uniform(-amount[k], amount[k]) for k in range(3)])
			points.append(moved[key])
		f.points = points


def _fade(faces: list, low: float) -> None:
	"""Darken faces toward the base of what they make up: tone low at the lowest point, 1 at the highest."""
	zs = [p[2] for f in faces for p in f.points]
	if not zs:
		return
	bottom, span = min(zs), max(max(zs) - min(zs), 1e-6)
	for f in faces:
		shades = f.corner_shade if f.corner_shade and len(f.corner_shade) == len(f.points) else [1.0] * len(f.points)
		f.corner_shade = [s * (low + (1.0 - low) * (p[2] - bottom) / span) for s, p in zip(shades, f.points)]


def _append(part, faces: list, matrix, origin: tuple = ()) -> None:
	"""faces through matrix into part; origin (the use lines they came through) goes before their own."""
	flip = matrix.determinant() < 0
	for f in faces:
		points = [(matrix @ pt) for pt in f.points]
		uv, shade = f.uv, f.corner_shade
		if flip:
			points = points[::-1]
			uv = uv[::-1] if uv else uv
			shade = shade[::-1] if shade else shade
		part.faces.append(Face(points, f.material, uv, f.shade, shade, f.group, f.tiled, origin + f.origin))
