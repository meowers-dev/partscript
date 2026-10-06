"""Sets, snap points and buildings for PartScript.

A kit block is a set of pieces by role: wall kinds (solid, window, door...), floor, roof,
parapet, partition, corner and stair, on a grid of cells (4 m) and storeys (4 m). A building
block lays rooms out on that grid: the set's floors, walls, roofs and parapets go where the
rooms say, openings swap wall kinds, and attach snaps any other piece onto a placed one by
named snap points. Everything here is plain arithmetic; geometry (a piece's faces and declared
snaps) comes from the compiler through the callbacks resolve() is given.

Coordinates are the prop's authoring space: X right, Y back, Z up, metres. A building's
origin is the front-left corner of cell 0,0; its front (side s) faces -Y. Cell x,y spans
x..x+1 and y..y+1 grid units; storey s stands at s storeys.
"""

from __future__ import annotations

import math
import re
from dataclasses import dataclass, field


BUILD_OPS = frozenset({"room", "open", "walls", "stair", "roof", "attach", "place"})
ROOM_OPTS = frozenset({"storeys", "storey", "floor", "roof", "walls", "theme", "as", "fill"})
FILL_VARIABLES = frozenset({"w", "d", "h", "level", "door_s", "door_n", "door_e", "door_w"})
WALL_KINDS = ("solid", "window", "window_broken", "door", "wide_door", "damaged", "upper")
ROLES = ("floor", "roof", "parapet", "partition", "corner", "stair")
OPENINGS = ("window", "window_broken", "door", "wide_door", "damaged")
SIDES = {"s": (0, -1), "n": (0, 1), "w": (-1, 0), "e": (1, 0)}
# Wall rotation (degrees about Z) whose interior face (-Y, the prop front) looks along the inward direction.
WALL_TURN = {(0, 1): 180.0, (0, -1): 0.0, (1, 0): 90.0, (-1, 0): -90.0}
DIRS = {"+x": (1.0, 0.0, 0.0), "-x": (-1.0, 0.0, 0.0), "+y": (0.0, 1.0, 0.0), "-y": (0.0, -1.0, 0.0),
	"+z": (0.0, 0.0, 1.0), "-z": (0.0, 0.0, -1.0), "right": (1.0, 0.0, 0.0), "left": (-1.0, 0.0, 0.0),
	"back": (0.0, 1.0, 0.0), "front": (0.0, -1.0, 0.0), "up": (0.0, 0.0, 1.0), "down": (0.0, 0.0, -1.0)}
KIT_OPTS = {"grid": 4.0, "storey": 4.0, "wall": 0.3, "parapet_lift": 0.3, "stair_cells": 2}
BUILDING_BUDGET = 200000

SNAP_USAGE = "snap NAME C DIR [up=DIR] [kind=WORD]   DIR: +x -x +y -y +z -z (or front back left right up down)"
ATTACH_USAGE = "attach PIECE to=TARGET [at=SNAP] [via=SNAP] [spin=DEG] [slide=A,B] [as=NAME]"


class BuildError(ValueError):
	pass


# ------------------------------------------------------------------ small vector maths (plain floats)
def _sub(a, b):
	return tuple(x - y for x, y in zip(a, b))


def _add(a, b):
	return tuple(x + y for x, y in zip(a, b))


def _scale(a, k):
	return tuple(x * k for x in a)


def _dot(a, b):
	return sum(x * y for x, y in zip(a, b))


def _cross(a, b):
	return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def _unit(a):
	length = math.sqrt(_dot(a, a))
	if length < 1e-9:
		raise BuildError("a snap direction has no length")
	return _scale(a, 1.0 / length)


def _frame(direction, up):
	"""Orthonormal columns (direction, up, direction x up); up is made perpendicular to direction."""
	d = _unit(direction)
	u = _sub(up, _scale(d, _dot(up, d)))
	if _dot(u, u) < 1e-9:
		u = _sub((0.0, 0.0, 1.0) if abs(d[2]) < 0.9 else (0.0, 1.0, 0.0), _scale(d, d[2] if abs(d[2]) < 0.9 else d[1]))
	u = _unit(u)
	return d, u, _cross(d, u)


def mat_identity():
	return [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]


def mat_mul(a, b):
	return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def mat_point(m, p):
	return tuple(m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3] for i in range(3))


def mat_vector(m, v):
	return tuple(m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2] for i in range(3))


def mat_place(position, degrees: float = 0.0):
	"""Translation then a turn of degrees about +Z."""
	c, s = math.cos(math.radians(degrees)), math.sin(math.radians(degrees))
	c, s = (round(c, 12), round(s, 12))
	return [[c, -s, 0.0, position[0]], [s, c, 0.0, position[1]], [0.0, 0.0, 1.0, position[2]], [0.0, 0.0, 0.0, 1.0]]


def mat_rotation(columns):
	"""Matrix whose 3x3 part has these columns."""
	return [[columns[0][i], columns[1][i], columns[2][i], 0.0] for i in range(3)] + [[0.0, 0.0, 0.0, 1.0]]


def mat_axis_turn(axis, degrees: float):
	"""Rotation by degrees about a unit axis through the origin (Rodrigues)."""
	x, y, z = _unit(axis)
	c, s = math.cos(math.radians(degrees)), math.sin(math.radians(degrees))
	t = 1.0 - c
	return [[t * x * x + c, t * x * y - s * z, t * x * z + s * y, 0.0], [t * x * y + s * z, t * y * y + c, t * y * z - s * x, 0.0],
		[t * x * z - s * y, t * y * z + s * x, t * z * z + c, 0.0], [0.0, 0.0, 0.0, 1.0]]


def mat_translate(v):
	m = mat_identity()
	m[0][3], m[1][3], m[2][3] = v
	return m


# ------------------------------------------------------------------ snaps
@dataclass
class Snap:
	name: str
	pos: tuple
	dir: tuple
	up: tuple = (0.0, 0.0, 1.0)
	kind: str = "any"

	def moved(self, m) -> "Snap":
		return Snap(self.name, mat_point(m, self.pos), _unit(mat_vector(m, self.dir)), _unit(mat_vector(m, self.up)), self.kind)

	def as_dict(self) -> dict:
		return {"name": self.name, "pos": [round(v, 4) for v in self.pos], "dir": [round(v, 4) for v in self.dir],
			"up": [round(v, 4) for v in self.up], "kind": self.kind}


def parse_dir(text: str, number) -> tuple:
	"""A direction word (+x, back, up...) or an x,y,z vector evaluated by number()."""
	if text in DIRS:
		return DIRS[text]
	parts = text.split(",")
	if len(parts) != 3:
		raise BuildError(f"direction {text!r}: one of {' '.join(k for k in DIRS if k[0] in '+-')} (or front back left right up down) or x,y,z")
	return _unit(tuple(number(p) for p in parts))


def default_up(direction) -> tuple:
	return (0.0, 1.0, 0.0) if abs(direction[2]) > 0.9 else (0.0, 0.0, 1.0)


def bounds_snaps(low, high) -> dict:
	"""Snaps every piece has from its bounds: base, top, front, back, left, right (kind any)."""
	cx, cy, cz = ((a + b) / 2 for a, b in zip(low, high))
	return {name: Snap(name, pos, d, default_up(d)) for name, pos, d in (
		("base", (cx, cy, low[2]), (0.0, 0.0, -1.0)), ("top", (cx, cy, high[2]), (0.0, 0.0, 1.0)),
		("front", (cx, low[1], cz), (0.0, -1.0, 0.0)), ("back", (cx, high[1], cz), (0.0, 1.0, 0.0)),
		("left", (low[0], cy, cz), (-1.0, 0.0, 0.0)), ("right", (high[0], cy, cz), (1.0, 0.0, 0.0)))}


def role_snaps(role: str, kit: "Kit") -> dict:
	"""Grid snaps a set's pieces get from their role: wall ends and faces, floor and roof edges."""
	g, h, t = kit.grid, kit.storey, kit.wall
	if role.startswith("wall") or role == "partition":
		return {s.name: s for s in (
			Snap("left", (-g / 2, 0.0, 0.0), (-1.0, 0.0, 0.0), kind="wall_end"), Snap("right", (g / 2, 0.0, 0.0), (1.0, 0.0, 0.0), kind="wall_end"),
			# On the wall top a piece's back goes to the interior (-Y), so its front faces the street.
			Snap("top", (0.0, 0.0, h), (0.0, 0.0, 1.0), (0.0, -1.0, 0.0), "wall_top"),
			Snap("outside", (0.0, t / 2, h / 2), (0.0, 1.0, 0.0), kind="wall_face"),
			Snap("inside", (0.0, -t / 2, h / 2), (0.0, -1.0, 0.0), kind="wall_face"))}
	if role in ("floor", "roof"):
		kind = role + "_edge"
		return {s.name: s for s in (
			Snap("top", (0.0, 0.0, 0.0), (0.0, 0.0, 1.0), (0.0, 1.0, 0.0), role + "_top"),
			Snap("n", (0.0, g / 2, 0.0), (0.0, 1.0, 0.0), kind=kind), Snap("s", (0.0, -g / 2, 0.0), (0.0, -1.0, 0.0), kind=kind),
			Snap("e", (g / 2, 0.0, 0.0), (1.0, 0.0, 0.0), kind=kind), Snap("w", (-g / 2, 0.0, 0.0), (-1.0, 0.0, 0.0), kind=kind))}
	return {}


def snap_matrix(source: Snap, target: Snap, spin: float = 0.0, slide=(0.0, 0.0)):
	"""Placement that puts source (piece-local) on target (building space): same point, facing
	each other (source dir against target dir), ups together; then spin about the target's
	direction and slide along its face (A to the right as you face it, B up)."""
	sd, su, sx = _frame(source.dir, source.up)
	td, tu, _ = _frame(target.dir, target.up)
	nd = _scale(td, -1.0)
	nd, nu, nx = _frame(nd, tu)
	rotation = mat_mul(mat_rotation((nd, nu, nx)), [[sd[0], sd[1], sd[2], 0.0], [su[0], su[1], su[2], 0.0], [sx[0], sx[1], sx[2], 0.0], [0.0, 0.0, 0.0, 1.0]])
	if spin:
		rotation = mat_mul(mat_axis_turn(td, spin), rotation)
	right = _cross(_scale(td, -1.0), tu)
	point = _add(target.pos, _add(_scale(right, slide[0]), _scale(tu, slide[1])))
	return mat_mul(mat_translate(point), mat_mul(rotation, mat_translate(_scale(source.pos, -1.0))))


def kinds_fit(a: str, b: str) -> bool:
	return a == "any" or b == "any" or a == b


# ------------------------------------------------------------------ kits (sets)
@dataclass
class Kit:
	name: str
	grid: float = 4.0
	storey: float = 4.0
	wall: float = 0.3
	parapet_lift: float = 0.3
	stair_cells: int = 2
	stair_exit: str = "ahead"  # where a stair lets you off: ahead (a straight flight) or back (a dog-leg, beside its foot)
	walls: dict = field(default_factory=dict)  # kind -> piece name
	pieces: dict = field(default_factory=dict)  # role -> piece name
	snaps: dict = field(default_factory=dict)  # piece name -> {snap name: Snap spec (unevaluated tokens)}
	file: str = ""
	line: int = 0

	def piece(self, role: str) -> str:
		return self.walls.get(role.removeprefix("wall:"), "") if role.startswith("wall:") else self.pieces.get(role, "")

	def roles_of(self, name: str) -> list[str]:
		return [f"wall:{k}" for k, v in self.walls.items() if v == name] + [r for r, v in self.pieces.items() if v == name]


def parse_kit(raw: list, file: str, program) -> None:
	"""kit NAME [walls=SET] [grid=4] [storey=4] [wall=.3] [parapet_lift=.3] [stair_cells=2], then lines
	wall KIND=PIECE ..., piece ROLE=PIECE ..., snap PIECE NAME C DIR [up=] [kind=]."""
	from .lang import PartScriptError, _unquote, tokenize
	number, header = raw[0]
	tokens = tokenize(header)
	if len(tokens) < 2 or not re.fullmatch(r"[a-z][a-z0-9_]{1,40}", tokens[1]):
		raise PartScriptError("kit needs a lower_snake_case name: kit NAME [walls=SET] [grid=4] [storey=4]", file, number)
	name = program.qualified(tokens[1], file)
	if name in program.kits:
		raise PartScriptError(f"kit {name} defined twice (also {program.kits[name]['file']}:{program.kits[name]['line']})", file, number)
	opts = {}
	for token in tokens[2:]:
		key, sep, value = token.partition("=")
		if not sep or key not in (*KIT_OPTS, "walls", "stair_exit"):
			raise PartScriptError(f"kit option {token!r}: walls=SET (start from a base wall set), {', '.join(f'{k}={v}' for k, v in KIT_OPTS.items())}", file, number)
		opts[key] = _unquote(value)
	spec = {"name": name, "opts": opts, "walls": {}, "pieces": {}, "snaps": {}, "file": file, "line": number}
	for number, statement in raw[1:]:
		if statement == "end":
			continue
		tokens = tokenize(statement)
		head = tokens[0]
		if head == "wall":
			for token in tokens[1:]:
				kind, sep, piece = token.partition("=")
				if not sep or not re.fullmatch(r"[a-z][a-z0-9_]{0,30}", kind) or not piece:
					raise PartScriptError(f"kit wall {token!r}: KIND=PIECE; the usual kinds are {', '.join(WALL_KINDS)} (any word works: shop, sash...)", file, number)
				spec["walls"][kind] = piece
		elif head == "piece":
			for token in tokens[1:]:
				role, sep, piece = token.partition("=")
				if not sep or role not in ROLES or not piece:
					raise PartScriptError(f"kit piece {token!r}: ROLE=PIECE with ROLE one of {', '.join(ROLES)}", file, number)
				spec["pieces"][role] = piece
		elif head == "snap":
			if len(tokens) < 5:
				raise PartScriptError(f"kit snap: snap PIECE NAME C DIR [up=DIR] [kind=WORD] (snap points for a piece that declares none)", file, number)
			spec["snaps"].setdefault(tokens[1], []).append({"tokens": tokens[2:], "file": file, "line": number})
		else:
			raise PartScriptError(f"'{head}' in kit {name}: wall KIND=PIECE, piece ROLE=PIECE or snap PIECE NAME C DIR", file, number)
	program.kits[name] = spec


def resolve_kit(program, name: str, base: dict | None = None, number=float, file: str = "") -> Kit:
	"""The set a building names: its own lines over the host's wall set it starts from (walls=SET).
	base is the host's kit (Host.base_kit()): {"walls": {set: {kind: piece}}, "pieces": {...}, "grid": ...}."""
	name = program.kit_name(name, file)  # its own namespace's kit first
	spec = program.kits.get(name)
	if spec is None:
		known = ", ".join(sorted(program.kits)) or "none yet"
		raise BuildError(f"no kit {name!r} (kits: {known}); define one: kit NAME walls=SET, then wall KIND=PIECE and piece ROLE=PIECE lines")
	opts = spec["opts"]
	kit = Kit(name, file=spec["file"], line=spec["line"])
	for key, default in KIT_OPTS.items():
		value = float(opts.get(key, default))
		setattr(kit, key, int(value) if key == "stair_cells" else value)
	kit.stair_exit = opts.get("stair_exit", "ahead")
	if kit.stair_exit not in ("ahead", "back"):
		raise BuildError(f"kit {name}: stair_exit={kit.stair_exit}: ahead (a straight flight) or back (a dog-leg that turns back on itself)")
	if opts.get("walls"):
		base = base or {}
		walls = base.get("walls", {})
		if opts["walls"] not in walls:
			raise BuildError(f"kit {name}: walls={opts['walls']}: the base kit has wall sets {', '.join(sorted(walls)) or 'none'}")
		kit.walls.update(walls[opts["walls"]])
		pieces = base.get("pieces", {})
		for role, key in (("floor", "floor"), ("roof", "roof"), ("parapet", "parapet"), ("partition", "partition"), ("corner", "pillar"), ("stair", "stair_4m")):
			if key in pieces:
				kit.pieces[role] = pieces[key]
		kit.grid, kit.storey = float(base.get("grid", kit.grid)), float(base.get("storey", kit.storey))
		kit.wall = float(base.get("wall_thickness", kit.wall))
		kit.parapet_lift = float(base.get("lengths", {}).get("parapet_lift", kit.parapet_lift))
		if "corner" in kit.pieces and "corner" not in spec["pieces"]:
			kit.pieces.pop("corner")  # the base kit's pillar is a free-standing column, not a corner cap: name one to use it
	namespace = program.namespaces.get(spec["file"])
	own = {p.name for p in program.props}

	def piece(value: str) -> str:
		"""A piece the kit names, in the kit's own namespace when it was imported as one."""
		return f"{namespace}.{value}" if namespace and f"{namespace}.{value}" in own else value
	kit.walls.update({k: piece(v) for k, v in spec["walls"].items()})
	kit.pieces.update({k: piece(v) for k, v in spec["pieces"].items()})
	if "solid" not in kit.walls:
		raise BuildError(f"kit {name}: no solid wall (wall solid=PIECE, or walls=SET to start from a base wall set)")
	kit.snaps = {piece(k): v for k, v in spec["snaps"].items()}
	return kit


# ------------------------------------------------------------------ building plans
@dataclass
class Placement:
	piece: str
	role: str  # wall:<kind>, floor, roof, parapet, partition, corner, stair, attach, place
	matrix: list | None
	stmt: object
	names: list = field(default_factory=list)
	nav: str = ""  # walkable (navigation bakes from its geometry, as room() walls do), ignore or "" (scenery)
	attach: dict | None = None  # unresolved attach request
	fill: dict | None = None  # a room's furnishing: {"env": variables for the def (w, d, level, door_*...)}


@dataclass
class Plan:
	kit: Kit
	placements: list = field(default_factory=list)
	rooms: list = field(default_factory=list)
	openings: list = field(default_factory=list)
	errors: list = field(default_factory=list)
	warnings: list = field(default_factory=list)  # the layout works but something in it is off (a room nobody can reach)
	by_name: dict = field(default_factory=dict)

	def for_stmt(self, stmt) -> list:
		return [p for p in self.placements if p.stmt is stmt]

	def add(self, placement: Placement) -> Placement:
		self.placements.append(placement)
		for name in placement.names:
			self.by_name.setdefault(name, placement)
		return placement


def _cells(text: str, number, what: str) -> list[tuple[int, int]]:
	"""X,Y with either part a number or an inclusive a..b range."""
	parts = text.split(",")
	if len(parts) != 2:
		raise BuildError(f"{what} {text!r}: X,Y in cells (0,0 is the front-left cell; a..b gives a range: 0..2,0)")
	ranges = []
	for part in parts:
		if ".." in part:
			a, b = (int(round(number(v))) for v in part.split("..", 1))
			ranges.append(range(min(a, b), max(a, b) + 1))
		else:
			ranges.append([int(round(number(part)))])
	return [(x, y) for y in ranges[1] for x in ranges[0]]


def _edge(cell, side):
	x, y = cell
	return {"s": ("x", x, y), "n": ("x", x, y + 1), "w": ("y", x, y), "e": ("y", x + 1, y)}[side]


def _edge_names(edge, storey):
	axis, x, y = edge
	cells = [((x, y - 1), "n"), ((x, y), "s")] if axis == "x" else [((x - 1, y), "e"), ((x, y), "w")]
	return [f"{c[0]},{c[1]},{side},{storey}" for c, side in cells]


def plan(prop, kit: Kit, number) -> Plan:
	"""Grid placements of a building, statement by statement; attach requests stay unresolved.
	number(text) evaluates an expression with the prop's variables."""
	result = Plan(kit)
	g, h = kit.grid, kit.storey
	body = [s for s in prop.body if s.op in BUILD_OPS]
	rooms, defaults, opens, stairs, roof_opts = [], [], {}, [], {"roof": None, "parapet": None}

	def fail(stmt, message):
		result.errors.append(f"{stmt.where()}: {message}")

	def kind_ok(kind: str) -> bool:
		return kind in ("none", "open") or kind in kit.walls

	for stmt in body:
		a, o = stmt.args, stmt.opts
		try:
			if stmt.op == "room":
				if len(a) < 2:
					raise BuildError("room X,Y W,D [storeys=1] [storey=0] [floor=none] [roof=none] [walls=KIND] [theme=STYLE] [as=NAME]")
				corner = _cells(a[0], number, "room")
				if len(corner) != 1:
					raise BuildError(f"room {a[0]}: X,Y is the room's front-left cell (no ranges); W,D its size in cells")
				(x, y), = corner
				size = a[1].split(",")
				if len(size) != 2:
					raise BuildError(f"room size {a[1]!r}: W,D in cells (3,2 is 12 x 8 m on a 4 m grid)")
				w, d = (int(round(number(v))) for v in size)
				storeys, base = int(round(number(o.get("storeys", "1")))), int(round(number(o.get("storey", "0"))))
				if w < 1 or d < 1 or not 1 <= storeys <= 12 or base < 0:
					raise BuildError(f"room {a[0]} {a[1]}: width, depth and storeys from 1 (storeys up to 12), storey from 0")
				if o.get("walls") and not kind_ok(o["walls"]):
					raise BuildError(f"walls={o['walls']}: kit {kit.name} has {', '.join(kit.walls)} (or none)")
				rooms.append({"stmt": stmt, "x": x, "y": y, "w": w, "d": d, "storeys": storeys, "base": base, "opts": o})
			elif stmt.op == "walls":
				if not a or not kind_ok(a[0]):
					raise BuildError(f"walls KIND [storey=N] [side=s,n,e,w]: KIND one of {', '.join(kit.walls)}, none")
				storeys = None if o.get("storey", "all") == "all" else {int(round(number(v))) for v in o["storey"].split(",")}
				sides = None if "side" not in o else set(o["side"].split(","))
				if sides and not sides <= set(SIDES):
					raise BuildError(f"side={o['side']}: s (front), n (back), w, e")
				defaults.append((a[0], storeys, sides))
			elif stmt.op == "open":
				if len(a) < 3 or a[1] not in SIDES or not kind_ok(a[2]):
					raise BuildError(f"open X,Y SIDE KIND [storey=0]: SIDE s n w e, KIND one of {', '.join(k for k in kit.walls)}, none")
				storey = int(round(number(o.get("storey", "0"))))
				for cell in _cells(a[0], number, "open"):
					opens[(_edge(cell, a[1]), storey)] = (a[2], stmt)
			elif stmt.op == "stair":
				if len(a) < 1:
					raise BuildError("stair X,Y [DIR] [storey=0]: rises toward DIR (n back, s front, e, w)")
				direction = a[1] if len(a) > 1 else "n"
				if direction not in SIDES:
					raise BuildError(f"stair direction {direction!r}: n (back), s (front), e or w")
				if not kit.pieces.get("stair"):
					raise BuildError(f"kit {kit.name} has no stair piece (piece stair=PIECE)")
				cell = _cells(a[0], number, "stair")
				if len(cell) != 1:
					raise BuildError(f"stair {a[0]}: one cell X,Y (its foot)")
				(x, y), = cell
				stairs.append({"stmt": stmt, "x": x, "y": y, "dir": direction, "storey": int(round(number(o.get("storey", "0"))))})
			elif stmt.op == "roof":
				for key in ("roof", "parapet"):
					value = o.get(key, a[0] if key == "roof" and a else None)
					if value is not None:
						roof_opts[key] = value
			elif stmt.op in ("attach", "place"):
				if not a:
					raise BuildError(ATTACH_USAGE if stmt.op == "attach" else "place PIECE C [r=rx,ry,rz] [as=NAME]")
				if stmt.op == "attach" and "to" not in o:
					raise BuildError(f"attach needs to=TARGET (a name given with as=, or X,Y,SIDE[,STOREY] for a wall, X,Y,floor[,STOREY], X,Y,roof): {ATTACH_USAGE}")
		except BuildError as error:
			fail(stmt, str(error))
		except ValueError as error:
			fail(stmt, str(error))
	if not rooms and not any(s.op in ("place", "attach") for s in body):
		result.errors.append(f"{prop.file}:{prop.line}: a building needs at least one room X,Y W,D (or place/attach lines)")
	if result.errors:
		return result

	# Which storeys each cell has a floor on, and the stairwells that open the floor above.
	occupied = {}
	for room in rooms:
		for s in range(room["base"], room["base"] + room["storeys"]):
			for cy in range(room["y"], room["y"] + room["d"]):
				for cx in range(room["x"], room["x"] + room["w"]):
					occupied.setdefault((cx, cy, s), room)
	wells = set()
	for stair in stairs:
		dx, dy = SIDES[stair["dir"]]
		for k in range(kit.stair_cells):
			wells.add((stair["x"] + dx * k, stair["y"] + dy * k, stair["storey"] + 1))

	# Edges: the first room to claim one owns it; the other side of a shared edge makes it a partition.
	edges = {}
	for room in rooms:
		inside = {(cx, cy) for cy in range(room["y"], room["y"] + room["d"]) for cx in range(room["x"], room["x"] + room["w"])}
		for s in range(room["base"], room["base"] + room["storeys"]):
			for cx, cy in sorted(inside, key=lambda c: (c[1], c[0])):
				for side, (dx, dy) in SIDES.items():
					if (cx + dx, cy + dy) in inside:
						continue
					key = (_edge((cx, cy), side), s)
					inward = (-dx, -dy)
					if key in edges:
						if edges[key]["inward"] != inward:
							edges[key]["shared"] = True
						continue
					edges[key] = {"room": room, "inward": inward, "shared": False, "storey": s, "side": side, "cell": (cx, cy)}

	def wall_kind(key, data):
		edge, s = key
		kind = "partition" if data["shared"] else "solid"
		if not data["shared"]:
			kind = data["room"]["opts"].get("walls", kind)
			for value, storeys, sides in defaults:
				if (storeys is None or s in storeys) and (sides is None or data["side"] in sides):
					kind = value
		if key in opens:
			kind = opens[key][0]
		return kind

	cutaway = prop.opts.get("cutaway", "") in ("open", "1", "true")  # a dolls' house: no front walls, no roof

	def doors_of(room, s) -> dict:
		"""Where a room's doors are on storey s, per side, in metres from its inside front-left corner
		(-1 where a side has none): door_s door_n along x, door_w door_e along y."""
		out: dict = {f"door_{side}": -1.0 for side in SIDES}
		out["doors"] = []  # every door: (side, metres along it)
		for cy in range(room["y"], room["y"] + room["d"]):
			for cx in range(room["x"], room["x"] + room["w"]):
				for side in SIDES:
					key = (_edge((cx, cy), side), s)
					if key in edges and "door" in wall_kind(key, edges[key]):
						along = round((cx - room["x"] + .5) * g if side in ("s", "n") else (cy - room["y"] + .5) * g, 4) - kit.wall / 2
						out["doors"].append((side, round(along, 4)))
						if out[f"door_{side}"] < 0:
							out[f"door_{side}"] = round(along, 4)
		return out

	# Every cell that is the top of some room: a roof there joins the roofs around it (no parapet between).
	roofed = {(cx, cy, r["base"] + r["storeys"]) for r in rooms for cy in range(r["y"], r["y"] + r["d"]) for cx in range(r["x"], r["x"] + r["w"])
		if (cx, cy, r["base"] + r["storeys"]) not in occupied}
	placed_floor, placed_roof, parapets, corners = set(), set(), set(), set()
	for room in rooms:
		stmt, o = room["stmt"], room["opts"]
		floor_piece = o.get("floor", kit.pieces.get("floor", ""))
		for s in range(room["base"], room["base"] + room["storeys"]):
			if floor_piece and floor_piece != "none":
				for cy in range(room["y"], room["y"] + room["d"]):
					for cx in range(room["x"], room["x"] + room["w"]):
						if (cx, cy, s) in placed_floor or (cx, cy, s) in wells:
							continue
						placed_floor.add((cx, cy, s))
						result.add(Placement(_role_piece(kit, floor_piece, "floor"), "floor", mat_place(((cx + .5) * g, (cy + .5) * g, s * h)),
							stmt, [f"{cx},{cy},floor,{s}"], "walkable"))
			for key, data in edges.items():
				if data["room"] is not room or key[1] != s:
					continue
				kind = wall_kind(key, data)
				if kind in ("none", "open") or (cutaway and data["side"] == "s" and not data["shared"]):
					continue
				role = kind
				if kind == "partition":
					piece = kit.pieces.get("partition") or kit.walls["solid"]
					role = "partition"
				else:
					if kind == "solid" and s > 0 and "upper" in kit.walls:
						kind = "upper"
					piece = kit.walls.get(kind, kit.walls["solid"])
					role = f"wall:{kind}"
				(axis, ex, ey), _ = key
				mid = ((ex + .5) * g, ey * g, s * h) if axis == "x" else (ex * g, (ey + .5) * g, s * h)
				turn = WALL_TURN[data["inward"]]
				result.add(Placement(piece, role, mat_place(mid, turn), stmt, _edge_names(key[0], s), "walkable"))
				if kind not in ("solid", "upper", "blank"):
					result.openings.append({"kind": kind, "cell": list(data["cell"]), "side": data["side"], "storey": s,
						"pos": [round(v, 4) for v in mid], "turn": turn})
			if kit.pieces.get("corner"):
				for (edge, es), data in edges.items():
					if data["room"] is not room or es != s or data["shared"] or wall_kind((edge, es), data) in ("none", "open"):
						continue
					axis, ex, ey = edge
					for vx, vy in ((ex, ey), (ex + 1, ey) if axis == "x" else (ex, ey + 1)):
						perpendicular = [("y", vx, vy - 1), ("y", vx, vy)] if axis == "x" else [("x", vx - 1, vy), ("x", vx, vy)]
						if any((p, s) in edges and not edges[(p, s)]["shared"] for p in perpendicular) and (vx, vy, s) not in corners:
							corners.add((vx, vy, s))
							result.add(Placement(kit.pieces["corner"], "corner", mat_place((vx * g, vy * g, s * h)), stmt, [f"corner:{vx},{vy},{s}"], "walkable"))
		top = room["base"] + room["storeys"]
		roof_piece = o.get("roof", roof_opts["roof"] or kit.pieces.get("roof", ""))
		if roof_piece and roof_piece != "none" and not cutaway:
			for cy in range(room["y"], room["y"] + room["d"]):
				for cx in range(room["x"], room["x"] + room["w"]):
					if (cx, cy, top) in occupied or (cx, cy, top) in placed_roof:
						continue
					placed_roof.add((cx, cy, top))
					result.add(Placement(_role_piece(kit, roof_piece, "roof"), "roof", mat_place(((cx + .5) * g, (cy + .5) * g, top * h)),
						stmt, [f"{cx},{cy},roof", f"{cx},{cy},roof,{top}"], "ignore"))
			parapet = roof_opts["parapet"] if roof_opts["parapet"] is not None else kit.pieces.get("parapet", "")
			if parapet and parapet != "none":
				for cy in range(room["y"], room["y"] + room["d"]):
					for cx in range(room["x"], room["x"] + room["w"]):
						if (cx, cy, top) not in placed_roof:
							continue
						for side, (dx, dy) in SIDES.items():
							nx, ny = cx + dx, cy + dy
							if (nx, ny, top) in roofed or (nx, ny, top) in occupied:
								continue
							edge = _edge((cx, cy), side)
							if (edge, top) in parapets:
								continue
							parapets.add((edge, top))
							axis, ex, ey = edge
							mid = ((ex + .5) * g, ey * g, top * h + kit.parapet_lift) if axis == "x" else (ex * g, (ey + .5) * g, top * h + kit.parapet_lift)
							result.add(Placement(parapet, "parapet", mat_place(mid, 0.0 if axis == "x" else 90.0), stmt, [], "ignore"))
		name = o.get("as", f"room{len(result.rooms) + 1}")
		result.rooms.append({"name": name, "cells": [room["x"], room["y"], room["w"], room["d"]],
			"storey": room["base"], "storeys": room["storeys"], "theme": o.get("theme", "")})
		if o.get("fill"):
			# The room's own options (other than its layout) are the furnishing def's parameters.
			params = {k: v for k, v in o.items() if k not in ROOM_OPTS}
			for s in range(room["base"], room["base"] + room["storeys"]):
				doors = doors_of(room, s)
				every = doors.pop("doors")
				variables = {**params, "w": room["w"] * g - kit.wall, "d": room["d"] * g - kit.wall, "h": h, "level": s, **doors,
					"__seed__": f"{prop.name}|{name}|{s}"}
				result.add(Placement(o["fill"], "fill", mat_place((room["x"] * g + kit.wall / 2, room["y"] * g + kit.wall / 2, s * h)), stmt,
					[f"{name}:{s}"], "", fill={"env": variables, "doors": every, "room": name}))
	turns = {"n": 0.0, "s": 180.0, "e": -90.0, "w": 90.0}
	for stair in stairs:
		x, y, direction, s = stair["x"], stair["y"], stair["dir"], stair["storey"]
		foot = {"n": ((x + .5) * g, y * g), "s": ((x + .5) * g, (y + 1) * g), "e": (x * g, (y + .5) * g), "w": ((x + 1) * g, (y + .5) * g)}[direction]
		result.add(Placement(kit.pieces["stair"], "stair", mat_place((foot[0], foot[1], s * h), turns[direction]), stair["stmt"],
			[stair["stmt"].opts["as"]] if "as" in stair["stmt"].opts else [], "walkable"))
	for stmt in body:
		if stmt.op == "place":
			at = stmt.args[1] if len(stmt.args) > 1 else "0,0,0"
			try:
				position = tuple(number(v) for v in at.split(","))
				rot = tuple(number(v) for v in stmt.opts.get("r", "0,0,0").split(","))
				if len(position) != 3 or len(rot) != 3:
					raise ValueError
			except ValueError:
				fail(stmt, f"place {stmt.args[0]} {at}: C is x,y,z in metres; r=rx,ry,rz in degrees")
				continue
			matrix = mat_translate(position)
			for axis, angle in zip(((1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, 0.0, 1.0)), rot):
				if angle:
					matrix = mat_mul(matrix, mat_axis_turn(axis, angle))
			result.add(Placement(stmt.args[0], "place", matrix, stmt, [stmt.opts["as"]] if "as" in stmt.opts else [], ""))
		elif stmt.op == "attach":
			o = stmt.opts
			try:
				slide = tuple(number(v) for v in o.get("slide", "0,0").split(","))
				if len(slide) != 2:
					raise ValueError
				spin = number(o.get("spin", "0"))
			except ValueError:
				fail(stmt, "slide=A,B: metres right and up along the target face; spin=DEG")
				continue
			target = o["to"]
			parts = target.split(",")
			if len(parts) == 3 and parts[2] in SIDES or (len(parts) == 3 and parts[2] == "floor"):
				target += ",0"
			result.add(Placement(stmt.args[0], "attach", None, stmt, [o["as"]] if "as" in o else [], "",
				{"to": target, "at": o.get("at", ""), "via": o.get("via", ""), "spin": spin, "slide": slide}))
	result.warnings += _reachability(rooms, edges, wall_kind, stairs, occupied, wells, kit)
	# Order the plan as the statements were written (a room's pieces, then its stairs, attachments...).
	order = {id(s): i for i, s in enumerate(prop.body)}
	result.placements.sort(key=lambda p: order.get(id(p.stmt), 0))
	return result


def _passable(kind: str) -> bool:
	return "door" in kind or kind in ("none", "open")


def _reachability(rooms, edges, wall_kind, stairs, occupied, wells, kit) -> list[str]:
	"""Warnings for rooms nobody can walk into from outside (through doors and up stairs), and for stairs
	whose foot or top lands where there is no floor."""
	def room_at(cell, s):
		return occupied.get((cell[0], cell[1], s))

	def label(room, s):
		return f"{room['opts'].get('as', 'room ' + str(room['x']) + ',' + str(room['y']))} (storey {s})"

	links: dict = {}
	starts = set()

	def link(a, b):
		links.setdefault(a, set()).add(b)
		links.setdefault(b, set()).add(a)

	for (edge, s), data in edges.items():
		kind = wall_kind((edge, s), data)
		if not _passable(kind):
			continue
		axis, x, y = edge
		cells = [(x, y - 1), (x, y)] if axis == "x" else [(x - 1, y), (x, y)]
		inside = [room_at(c, s) for c in cells]
		if all(inside):
			link((id(inside[0]), s), (id(inside[1]), s))
		elif any(inside) and "door" in kind:
			starts.add((id(next(r for r in inside if r)), s))
	out = []
	for stair in stairs:
		dx, dy = SIDES[stair["dir"]]
		x, y, s = stair["x"], stair["y"], stair["storey"]
		foot = (x - dx, y - dy)
		top = (x + dx * kit.stair_cells, y + dy * kit.stair_cells) if kit.stair_exit == "ahead" else (x - dx, y - dy)
		where = f"{stair['stmt'].where()}: stair {x},{y} {stair['dir']} storey={s}"
		low, high = room_at(foot, s), room_at(top, s + 1)
		if low is None or (foot[0], foot[1], s) in wells:
			out.append(f"{where}: no floor at its foot (cell {foot[0]},{foot[1]} on storey {s}) to step on from")
		if high is None or (top[0], top[1], s + 1) in wells:
			out.append(f"{where}: no floor where it arrives (cell {top[0]},{top[1]} on storey {s + 1}) to step off onto")
		if low is not None and high is not None:
			link((id(low), s), (id(high), s + 1))
	seen, todo = set(starts), list(starts)
	while todo:
		node = todo.pop()
		for other in links.get(node, ()):
			if other not in seen:
				seen.add(other)
				todo.append(other)
	for room in rooms:
		for s in range(room["base"], room["base"] + room["storeys"]):
			if (id(room), s) not in seen:
				out.append(f"{room['stmt'].where()}: {label(room, s)} cannot be reached from outside (no door or stair leads to it)")
	return out


def _role_piece(kit: Kit, value: str, role: str) -> str:
	"""A room's floor=/roof= option: a role name of the kit, or a piece."""
	return kit.pieces.get(value, value) if value in ROLES else value


def resolve(result: Plan, snaps_of) -> None:
	"""Places attachments in order. snaps_of(piece, role) -> {name: Snap} in the piece's own space."""
	for placement in result.placements:
		if placement.attach is None:
			continue
		request, stmt = placement.attach, placement.stmt
		target = result.by_name.get(request["to"])
		if target is None or target.matrix is None:
			names = sorted(n for n in result.by_name if "," not in n)
			raise BuildError(f"{stmt.where()}: attach to={request['to']}: nothing placed there yet"
				+ (f" (named pieces: {', '.join(names)})" if names else "") + "; walls are X,Y,SIDE,STOREY, floors X,Y,floor,STOREY, roofs X,Y,roof")
		target_snaps = {name: snap.moved(target.matrix) for name, snap in snaps_of(target.piece, target.role).items()}
		at = request["at"] or ("outside" if "outside" in target_snaps else "top")
		if at not in target_snaps:
			raise BuildError(f"{stmt.where()}: {target.piece} has no snap {at!r} (it has {', '.join(sorted(target_snaps))})")
		goal = target_snaps[at]
		own = snaps_of(placement.piece, "attach")
		via = request["via"]
		if not via:
			matching = [s for s in own.values() if s.kind == goal.kind and goal.kind != "any"]
			via = matching[0].name if matching else ("base" if goal.dir[2] > 0.7 else "top" if goal.dir[2] < -0.7 else "back")
		if via not in own:
			raise BuildError(f"{stmt.where()}: {placement.piece} has no snap {via!r} (it has {', '.join(sorted(own))})")
		if not kinds_fit(own[via].kind, goal.kind):
			raise BuildError(f"{stmt.where()}: {placement.piece}.{via} is kind {own[via].kind}, {target.piece}.{at} is kind {goal.kind}: they do not fit")
		placement.matrix = snap_matrix(own[via], goal, request["spin"], request["slide"])


# ------------------------------------------------------------------ export for game engines (Y up)
def yup_position(p) -> list:
	"""An authoring-space point (Z up, +Y back) in Y-up space (glTF, Godot): (x, z, -y)."""
	return [round(p[0], 4) + 0.0, round(p[2], 4) + 0.0, round(-p[1], 4) + 0.0]


def yup_rotation(m) -> dict:
	"""{"yaw": degrees} for a turn about the vertical, else {"rotation": [x, y, z]} radians, Y-up, YXZ order (Godot's)."""
	if abs(m[2][2] - 1.0) < 1e-6:
		return {"yaw": round(math.degrees(math.atan2(m[1][0], m[0][0])), 4)}
	# Y-up basis = C R C^T with C taking authoring (x, y, z) to Y-up (x, z, -y).
	c = [[1, 0, 0], [0, 0, 1], [0, -1, 0]]
	r = [[m[i][j] for j in range(3)] for i in range(3)]
	cr = [[sum(c[i][k] * r[k][j] for k in range(3)) for j in range(3)] for i in range(3)]
	g = [[sum(cr[i][k] * c[j][k] for k in range(3)) for j in range(3)] for i in range(3)]
	x = math.asin(max(-1.0, min(1.0, -g[1][2])))
	if abs(math.cos(x)) > 1e-6:
		y, z = math.atan2(g[0][2], g[2][2]), math.atan2(g[1][0], g[1][1])
	else:
		y, z = math.atan2(-g[2][0], g[0][0]), 0.0
	return {"rotation": [round(x, 6), round(y, 6), round(z, 6)]}


def export_building(result: Plan, title: str, asset_of) -> dict:
	"""A building as data for a game engine to place: its pieces in Y-up space round the front-left
	corner, its rooms and its openings. asset_of(piece) names the asset a piece places."""
	g = result.kit.grid
	placements = []
	for p in result.placements:
		if p.matrix is None:
			continue
		row = {"asset": asset_of(p.piece), "role": p.role, "position": yup_position(mat_point(p.matrix, (0.0, 0.0, 0.0)))}
		row.update(yup_rotation(p.matrix))
		if p.nav:
			row["nav"] = p.nav
		placements.append(row)
	rooms = []
	for room in result.rooms:
		x, y, w, d = room["cells"]
		rooms.append({"name": room["name"], "rect": [x * g, -(y + d) * g, (x + w) * g, -y * g], "storey": room["storey"],
			"storeys": room["storeys"], "theme": room["theme"]})
	openings = [{"kind": o["kind"], "cell": o["cell"], "side": o["side"], "storey": o["storey"], "position": yup_position(o["pos"]),
		"yaw": o["turn"]} for o in result.openings]
	return {"title": title, "set": result.kit.name, "grid": g, "storey": result.kit.storey, "placements": placements, "rooms": rooms,
		"openings": openings}
