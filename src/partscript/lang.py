"""The PartScript language: expressions, tokens, statements and the parser. Pure Python, no geometry.

parse() turns text into a Program (props, defs, styles, kits, file variables). Errors are
collected on the program, not raised, so one bad line never hides the rest of a file.
"""

from __future__ import annotations

import ast
import hashlib
import json
import math
import itertools
import operator
import random
import re
from dataclasses import dataclass, field

class PartScriptError(Exception):
	def __init__(self, message: str, file: str = "", line: int = 0):
		super().__init__(f"{file}:{line}: {message}" if file else message)
		self.message = message
		self.file = file
		self.line = line


# ------------------------------------------------------------------ expressions
_BIN = {ast.Add: operator.add, ast.Sub: operator.sub, ast.Mult: operator.mul, ast.Div: operator.truediv,
	ast.Pow: operator.pow, ast.Mod: operator.mod, ast.FloorDiv: operator.floordiv}
_FUNCS = {
	"sin": lambda d: math.sin(math.radians(d)), "cos": lambda d: math.cos(math.radians(d)), "tan": lambda d: math.tan(math.radians(d)),
	"sqrt": math.sqrt, "abs": abs, "min": min, "max": max, "floor": math.floor, "ceil": math.ceil, "round": round,
	"atan2": lambda y, x: math.degrees(math.atan2(y, x)), "hypot": math.hypot, "rad": math.radians, "deg": math.degrees,
	# Inverse trig answers in degrees, like the angles sin/cos/tan take (a ramp: r=0,atan(rise/run),0).
	"atan": lambda v: math.degrees(math.atan(v)), "asin": lambda v: math.degrees(math.asin(v)),
	"acos": lambda v: math.degrees(math.acos(v)),
}
_CONSTS = {"pi": math.pi, "tau": math.tau}
_COMPARE = {ast.Eq: operator.eq, ast.NotEq: operator.ne, ast.Lt: operator.lt, ast.LtE: operator.le, ast.Gt: operator.gt,
	ast.GtE: operator.ge}
_PICK = re.compile(r"pick\(([^()]*)\)")


def _rng(env: dict, text: str, at: int) -> random.Random:
	"""The random stream of one rand()/pick() call: seeded by the copy it runs for (env's __seed__,
	the chain of use lines and copy numbers), its text and where in the text it stands. The same
	build always draws the same numbers; another copy, another line or another call draws others."""
	return random.Random(f"{env.get('__seed__', '')}#{text}#{at}")


def resolve_picks(text: str, env: dict) -> str:
	"""Each pick(a,b,c) in text replaced by one of its items, chosen for this copy."""
	return _PICK.sub(lambda m: _rng(env, text, m.start()).choice([v.strip() for v in m.group(1).split(",")]), text)


_BRACES = re.compile(r"\{([^{}]+)\}")


def interpolate(text: str, env: dict) -> str:
	"""Label text for one copy: {name} of a variable holding text put in, each pick(a,b,c) chosen, each
	{expression} worked out (whole numbers without a decimal point): "{names} ASH {floor(rand(1820,1890))}"."""
	def words(match) -> str:
		value = env.get(match.group(1).strip())
		if isinstance(value, str):
			try:
				float(value)
			except ValueError:
				return value
		return match.group(0)
	text = _BRACES.sub(words, text)
	text = resolve_picks(text, env)

	def number(match) -> str:
		value = evaluate(match.group(1), env)
		return str(int(round(value))) if abs(value - round(value)) < 1e-9 else f"{value:g}"
	return _BRACES.sub(number, text)


def is_dynamic(text: str) -> bool:
	"""Text that changes per copy (has a pick() or an {expression})."""
	return "pick(" in text or bool(_BRACES.search(text))


def pick_options(text: str) -> list[str]:
	"""Every text the pick()s in text can become (to register each colour a pick might choose)."""
	found = list(_PICK.finditer(text))
	if not found:
		return [text]
	choices = [[v.strip() for v in m.group(1).split(",")] for m in found]
	out = []
	for combo in itertools.product(*choices):
		pieces, last = [], 0
		for match, value in zip(found, combo):
			pieces += [text[last:match.start()], value]
			last = match.end()
		out.append("".join(pieces) + text[last:])
	return out
## Shapes whose s= is a side count, not a scale.
_SIDED = frozenset({"c", "cone", "sph", "tube", "lathe", "pipe", "sweep", "vault", "archwall", "torus"})


class _Unknown(Exception):
	pass


class _Skip(Exception):
	"""A shape whose material is none (an optional piece a def leaves out)."""


class Bounds:
	"""What a named line made (desk = box ...): the box its shapes fill, kept in prop space and read in
	the space of the line that asks (its group, its def), so desk.top means the same surface anywhere."""

	NUMBERS = ("left", "right", "front", "back", "bottom", "top", "x", "y", "z", "w", "d", "h")

	def __init__(self, name: str, lo: tuple | None, hi: tuple | None, faces: list | None = None, world: bool = False):
		self.name, self.lo, self.hi = name, lo, hi
		self.faces = faces or []  # the faces it made (scatter ... on NAME spreads copies over them)
		self.world = world  # here: read in prop space wherever it is asked
		self._seen: tuple = (None, None)

	def local(self, frame) -> tuple[tuple, tuple]:
		"""(lo, hi) in the space frame places things in (frame None: prop space)."""
		if self.lo is None:
			raise ValueError(f"{self.name} made no shapes (when= or a none material left them all out)")
		if frame is None or self.world:
			return self.lo, self.hi
		if self._seen[0] is frame:
			return self._seen[1]
		inverse = frame.inverted()
		corners = [inverse @ (x, y, z) for x in (self.lo[0], self.hi[0]) for y in (self.lo[1], self.hi[1]) for z in (self.lo[2], self.hi[2])]
		out = (tuple(min(c[k] for c in corners) for k in range(3)), tuple(max(c[k] for c in corners) for k in range(3)))
		self._seen = (frame, out)
		return out

	def number(self, word: str, frame) -> float:
		lo, hi = self.local(frame)
		k = {"left": 0, "right": 0, "x": 0, "w": 0, "front": 1, "back": 1, "y": 1, "d": 1, "bottom": 2, "top": 2, "z": 2, "h": 2}.get(word)
		if k is None:
			raise ValueError(f"{self.name}.{word}: a named shape gives {', '.join(self.NUMBERS)} (and points like {self.name}.top_left)")
		if word in ("left", "front", "bottom"):
			return lo[k]
		if word in ("right", "back", "top"):
			return hi[k]
		if word in ("w", "d", "h"):
			return hi[k] - lo[k]
		return (lo[k] + hi[k]) / 2

	def point(self, words: str, frame) -> tuple:
		"""A point on it: desk (its centre), desk.top (the middle of its top), desk.top_left_back (a corner)..."""
		lo, hi = self.local(frame)
		out = [(lo[k] + hi[k]) / 2 for k in range(3)]
		for word in (w for w in words.split("_") if w):
			if word not in ("left", "right", "front", "back", "bottom", "top", "centre", "center"):
				raise ValueError(f"{self.name}.{words}: a point is its centre, a side (top, left...) or sides joined (top_left, bottom_front_right)")
			if word in ("centre", "center"):
				continue
			k = {"left": 0, "right": 0, "front": 1, "back": 1, "bottom": 2, "top": 2}[word]
			out[k] = lo[k] if word in ("left", "front", "bottom") else hi[k]
		return tuple(out)


_ANCHOR = re.compile(r"^([a-z_]\w*)(?:\.([a-z_]+))?$")


def anchor_point(text: str, env: dict) -> tuple | None:
	"""A whole position given by a named shape (desk, desk.top, desk.top_left), else None."""
	match = _ANCHOR.match(text.strip())
	if not match or not isinstance(env.get(match.group(1)), Bounds):
		return None
	return env[match.group(1)].point(match.group(2) or "", env.get("__frame__"))


def evaluate(text: str, env: dict) -> float:
	"""A number from an expression over env's numbers (safe: arithmetic and a few functions)."""
	text = text.strip()
	if not text:
		raise ValueError("empty number")
	try:
		return float(text)
	except ValueError:
		pass
	if "pick(" in text:
		text = resolve_picks(text, env)
	try:
		tree = ast.parse(text, mode="eval")
	except SyntaxError:
		raise ValueError(f"not a number or expression: {text!r}")

	def walk(node):
		if isinstance(node, ast.Expression):
			return walk(node.body)
		if isinstance(node, ast.Constant) and isinstance(node.value, (int, float)):
			return float(node.value)
		if isinstance(node, ast.BinOp) and type(node.op) in _BIN:
			return _BIN[type(node.op)](walk(node.left), walk(node.right))
		if isinstance(node, ast.UnaryOp) and isinstance(node.op, (ast.USub, ast.UAdd)):
			value = walk(node.operand)
			return -value if isinstance(node.op, ast.USub) else value
		if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.Not):
			return 0.0 if walk(node.operand) else 1.0
		if isinstance(node, ast.Compare) and all(type(op) in _COMPARE for op in node.ops):
			left = walk(node.left)
			for op, right_node in zip(node.ops, node.comparators):
				right = walk(right_node)
				if not _COMPARE[type(op)](left, right):
					return 0.0
				left = right
			return 1.0
		if isinstance(node, ast.BoolOp):
			values = [walk(v) for v in node.values]
			return float(all(values) if isinstance(node.op, ast.And) else any(values))
		if isinstance(node, ast.IfExp):
			return walk(node.body) if walk(node.test) else walk(node.orelse)
		if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id in ("noise", "rough") and not node.keywords:
			# Smooth noise over space (0-1), the same for the whole prop (its seed=): noise(x, y[, z]).
			from kitlib.noise import noise, rough
			values = [walk(a) for a in node.args]
			if not 1 <= len(values) <= 3:
				raise ValueError(f"{node.func.id}(x[, y[, z]]): one to three numbers, a point in space")
			return (noise if node.func.id == "noise" else rough)(*values, seed=str(env.get("__seed__", "")).split("|")[0])
		if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id == "rand" and not node.keywords:
			bounds = [walk(a) for a in node.args]
			low, high = (0.0, 1.0) if not bounds else (0.0, bounds[0]) if len(bounds) == 1 else bounds[:2]
			return _rng(env, text, node.col_offset).uniform(low, high)
		if isinstance(node, ast.Attribute) and isinstance(node.value, ast.Name):
			target = env.get(node.value.id)
			if not isinstance(target, Bounds):
				raise _Unknown(node.value.id) if target is None else ValueError(f"{node.value.id}.{node.attr}: {node.value.id} is not a named shape")
			return target.number(node.attr, env.get("__frame__"))
		if isinstance(node, ast.Name):
			if node.id in env:
				value = env[node.id]
				if isinstance(value, Bounds):
					raise ValueError(f"{node.id} is a named shape: say which number (left right front back bottom top x y z w d h), as in {node.id}.top")
				if isinstance(value, (int, float)):
					return float(value)
				try:
					return evaluate(str(value), env)
				except ValueError:
					raise ValueError(f"{node.id} is {value!r}, not a number")
			if node.id in _CONSTS:
				return _CONSTS[node.id]
			raise _Unknown(node.id)
		if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id in _FUNCS and not node.keywords:
			return float(_FUNCS[node.func.id](*[walk(a) for a in node.args]))
		raise ValueError(f"unsupported in an expression: {ast.dump(node)[:40]}")

	try:
		return walk(tree)
	except _Unknown as missing:
		raise ValueError(f"unknown name {missing} in {text!r}")
	except (ZeroDivisionError, OverflowError, TypeError) as error:
		raise ValueError(f"{text!r}: {error}")


def split_top(text: str, sep: str = ",") -> list[str]:
	"""Split at sep outside parentheses."""
	out, depth, current = [], 0, ""
	for ch in text:
		if ch == "(":
			depth += 1
		elif ch == ")":
			depth -= 1
		if ch == sep and depth == 0:
			out.append(current)
			current = ""
		else:
			current += ch
	out.append(current)
	return out


# ------------------------------------------------------------------ tokens and statements
_COMMENT = re.compile(r"(^|\s)#(\s|$)")


def _strip_comment(line: str) -> str:
	quoted = False
	for index, ch in enumerate(line):
		if ch == '"':
			quoted = not quoted
		elif ch == "#" and not quoted and (index == 0 or line[index - 1].isspace()) and (index + 1 == len(line) or line[index + 1].isspace()):
			return line[:index]
	return line


def _split_statements(line: str) -> list[str]:
	"""Statements on a line: ';' separates them, and a block opened and closed on one line
	(at 0,4,0 { a ; b }) becomes its opener, its statements and '}'."""
	out, current, quoted = [], "", False
	for index, ch in enumerate(line):
		if ch == '"':
			quoted = not quoted
		before = line[index - 1] if index else " "
		after = line[index + 1] if index + 1 < len(line) else " "
		# A brace is a block only standing alone ({v} is a variant placeholder).
		brace = ch in "{}" and before in " \t;" and after in " \t;"
		if not quoted and (ch == ";" or brace):
			if ch == "{":
				current += "{"
				out.append(current)
			else:
				out.append(current)
				if ch == "}":
					out.append("}")
			current = ""
		else:
			current += ch
	out.append(current)
	return [s.strip() for s in out if s.strip()]


def tokenize(text: str) -> list[str]:
	"""Whitespace tokens; "quoted strings" stay whole (key="a b" too)."""
	tokens, current, quoted = [], "", False
	for ch in text:
		if ch == '"':
			quoted = not quoted
			current += ch
		elif ch.isspace() and not quoted:
			if current:
				tokens.append(current)
			current = ""
		else:
			current += ch
	if quoted:
		raise ValueError("unclosed quote")
	if current:
		tokens.append(current)
	return tokens


def _unquote(token: str) -> str:
	return token[1:-1] if len(token) >= 2 and token[0] == token[-1] == '"' else token


@dataclass
class Stmt:
	op: str
	args: list
	opts: dict
	mods: list
	file: str
	line: int
	block: list | None = None  # at { ... }
	name: str = ""  # desk = box ...: what the line made, by name, for later lines (desk.top, on=desk)
	called: bool = False  # stool at=...: a part called by its name, as if it were a shape (a use)
	key: str = ""  # what its draws are seeded by: its block, its words, which of the same words it is (_body)

	def where(self) -> str:
		return f"{self.file}:{self.line}"

	def seed(self) -> str:
		"""The line's own part of a random seed. It follows the line's words, not its line number, so a
		comment or a line added elsewhere leaves every other line's draws as they were."""
		return f"{self.file}:{self.key or self.line}"


@dataclass
class Prop:
	name: str
	title: str
	subcategory: str
	opts: dict
	body: list
	file: str
	line: int
	kind: str = "prop"  # or "building": rooms of a kit's pieces (building.py)


@dataclass
class Macro:
	name: str
	params: dict
	body: list
	file: str
	line: int


@dataclass
class Program:
	props: list = field(default_factory=list)
	macros: dict = field(default_factory=dict)
	errors: list = field(default_factory=list)
	files: list = field(default_factory=list)
	styles: dict = field(default_factory=dict)  # room style name -> {layer: [entries], "exterior": bool}
	dressing: dict = field(default_factory=dict)  # dressing bucket -> [ids or [id, radius]]
	style_lines: dict = field(default_factory=dict)  # style name -> (file, line) for errors
	kits: dict = field(default_factory=dict)  # kit (set of pieces) name -> spec (building.parse_kit)
	file_env: dict = field(default_factory=dict)  # file -> its unindented set variables (they hold for that file only)
	namespaces: dict = field(default_factory=dict)  # file -> the name it was imported as (import "..." as NAME)
	imports: list = field(default_factory=list)  # (file, line, path, alias) of its import lines
	imported: set = field(default_factory=set)  # files that came in by import (used, not built on their own)
	std: set = field(default_factory=set)  # the std parts' names

	def env_of(self, file: str) -> dict:
		"""The variables a file's unindented set lines give it."""
		return dict(self.file_env.get(file, {}))

	def qualified(self, name: str, file: str) -> str:
		"""A name defined in file, as the program knows it: furniture.table in a file imported as furniture."""
		namespace = self.namespaces.get(file)
		return f"{namespace}.{name}" if namespace else name

	def _candidates(self, name: str, file: str) -> list:
		namespace = self.namespaces.get(file)
		return ([f"{namespace}.{name}"] if namespace else []) + [name]

	def macro(self, name: str, file: str = "") -> "Macro | None":
		"""The def a name used in file means: its own namespace's first, then the project's."""
		return next((self.macros[c] for c in self._candidates(name, file) if c in self.macros), None)

	def find_prop(self, name: str, file: str = "", names=lambda p: (p.name,)) -> "Prop | None":
		"""The prop a name used in file means (names(prop): the names it answers to)."""
		for candidate in self._candidates(name, file):
			found = next((p for p in self.props if p.kind == "prop" and candidate in names(p)), None)
			if found is not None:
				return found
		return None

	def kit_name(self, name: str, file: str = "") -> str:
		return next((c for c in self._candidates(name, file) if c in self.kits), name)


_COUNT = r"(?:\d+|\([^\s@%]*?\))"  # a number or an (expression) without @ or %
_MOD = re.compile(r"^\*(" + _COUNT + r"(?:x" + _COUNT + r"){0,2})(?:@(.+))?$|^\*(" + _COUNT + r")%(.+)$|^\*(" + _COUNT + r")~(.+)$"
	r"|^\*(" + _COUNT + r")\^(.+)$")  # *N^NAME[,GAP]: N copies spread over the faces of the shape named NAME


def _counts(text: str, env: dict) -> list[int]:
	"""The copy counts of a *N / *AxB / *(expr) modifier."""
	out = []
	for item in split_top(text, "x"):
		value = evaluate(item[1:-1] if item.startswith("(") else item, env)
		if value < 0 or value != value:
			raise ValueError(f"copy count {item} = {value}")
		out.append(int(round(value)))
	return out
USE_OPTIONS = ("r", "s", "jit", "when", "fade", "wobble", "along", "every", "fit", "closed", "joints", "corners", "index", "twist", "bend",
	"shrink", "smooth")
BUILD_OPS = frozenset({"room", "open", "walls", "stair", "roof", "attach", "place"})  # building lines (building.py)
SHAPES = {"b", "bb", "bx", "c", "cone", "sph", "tube", "wedge", "lathe", "ext", "pipe", "sweep", "face", "pan", "trim", "sign", "use", "at",
	"set", "part", "size", "vault", "archwall", "card", "snap", "chain", "torus", "frame", "link", "join", "row", "terrain", *BUILD_OPS}
ALIASES = {"box": "b", "bevel": "bb", "bevel_box": "bb", "box_between": "bx", "cyl": "c", "cylinder": "c", "sphere": "sph", "panel": "pan",
	"group": "at", "extrude": "ext", "label": "sign", "decal": "trim", "arch_wall": "archwall"}


# ------------------------------------------------------------------ the readable form
# Every shape takes its arguments by name as well as by position (box at=0,0,on size=.4 mat=wood), the
# copies modifiers have words (repeat 5 every .3,0,0 / ring 6 / scatter 40 over 2,1 apart .1 / mirror x),
# options have long names (turn= for r=), and on / on(z) stands for ~ / ~z ("sit it on"). Both forms
# parse to the same statement; fmt.py rewrites a file from one to the other.
SIGNATURES = {
	"b": ("at", "size", "mat"), "bb": ("at", "size", "bevel", "mat"), "bx": ("from", "to", "mat"),
	"c": ("at", "radius", "height", "mat"), "cone": ("at", "radius", "height", "mat"), "sph": ("at", "radius", "mat"),
	"tube": ("at", "radius", "inner", "height", "mat"), "wedge": ("at", "size", "mat"),
	"pan": ("at", "width", "height", "mat"), "trim": ("at", "width", "height", "cell"), "sign": ("at", "width", "height", "text"),
	"ext": ("at", "width", "mat"), "lathe": ("at", "mat"), "pipe": ("mat", "radius"), "sweep": ("mat",), "face": ("mat",),
	"vault": ("at", "width", "depth", "rise", "mat"), "archwall": ("at", "width", "height", "thick", "mat"),
	"use": ("name", "at"), "at": ("at",), "chain": (),
	"torus": ("at", "radius", "thick", "mat"), "frame": ("at", "width", "height", "depth", "mat"),
	"link": ("kind", "at", "toward"), "join": ("kind",), "row": ("axis",), "terrain": ("at", "size", "mat"),
}
_POINTS = {"ext", "lathe", "pipe", "sweep", "face"}  # shapes whose further arguments are a list of points
_SIDED_OPS = {"c", "cone", "sph", "tube", "lathe", "pipe", "sweep", "vault", "archwall", "torus", "join"}
OPTION_NAMES = {"turn": "r", "rotate": "r", "top_radius": "rt", "axis": "ax", "jitter": "jit", "cap_mat": "capm"}
_WORDS = ("repeat", "grid", "ring", "scatter", "mirror")


def _count(text: str) -> str:
	return text if text.isdigit() or (text.startswith("(") and text.endswith(")")) else f"({text})"


def _modifier_words(tokens: list[str], file: str, line: int) -> list[str]:
	"""Readable copies modifiers rewritten as their marks: repeat N every V -> *N@V, ring N [step D] -> *N%D,
	scatter N over W,D [apart G] / scatter N within R [apart G] -> *N~..., mirror xy -> mx my."""
	out, k = [], 0
	while k < len(tokens):
		word = tokens[k]
		if word == "as" and k + 1 < len(tokens) and re.fullmatch(r"[a-z_]\w*(,[a-z_]\w*){0,2}", tokens[k + 1]):
			out.append(f"index={tokens[k + 1]}")  # repeat 5 every ... as k / grid 4x3 every ... as col,row
			k += 2
			continue
		if word not in _WORDS or k + 1 >= len(tokens):
			out.append(word)
			k += 1
			continue
		rest = tokens[k + 1:]
		if word == "mirror":
			axes = rest[0] if rest and re.fullmatch(r"[xyz]{1,3}", rest[0]) else ""
			if not axes:
				raise PartScriptError("mirror x, mirror y, mirror z or mirror xy...", file, line)
			out += [f"m{axis}" for axis in axes]
			k += 2
		elif word in ("repeat", "grid"):
			count = rest[0]
			items = split_top(count, "x")  # 3x4 is a grid; an x inside brackets (max(...)) is not
			counts = "x".join(_count(c) for c in items) if 2 <= len(items) <= 3 and all(items) else _count(count)
			if len(rest) > 2 and rest[1] == "every":
				out.append(f"*{counts}@{rest[2]}")
				k += 4
			else:
				out.append(f"*{counts}")
				k += 2
		elif word == "ring":
			count = _count(rest[0])
			if len(rest) > 2 and rest[1] == "step":
				out.append(f"*{count}%{rest[2]}")
				k += 4
			else:
				out.append(f"*{count}%(360/{count})")
				k += 2
		elif word == "scatter" and len(rest) > 2 and rest[1] == "on":
			# scatter N on NAME [facing up|side|down] [apart G]: over the faces of a named shape
			count, target, gap, k = _count(rest[0]), rest[2], "", k + 4
			while k + 1 < len(tokens) and tokens[k] in ("facing", "apart"):
				if tokens[k] == "facing":
					out.append(f"facing={tokens[k + 1]}")
				else:
					gap = tokens[k + 1]
				k += 2
			out.append(f"*{count}^{target}" + (f",{gap}" if gap else ""))
		elif word == "scatter":
			if len(rest) < 3 or rest[1] not in ("over", "within"):
				raise PartScriptError("scatter N over W,D [apart G], scatter N within R [apart G] or scatter N on NAME [facing up]", file, line)
			apart = rest[4] if len(rest) > 4 and rest[3] == "apart" else ""
			area = rest[2] if rest[1] == "over" else (f"{rest[2]},0" if apart else rest[2])
			out.append(f"*{_count(rest[0])}~{area}" + (f",{apart}" if apart else ""))
			k += 6 if apart else 4
	return out


def _on(token: str) -> str:
	"""on / on(z) components of a position as ~ / ~z."""
	if "on" not in token or token.startswith('"'):
		return token
	parts = split_top(token)
	for k, part in enumerate(parts):
		if part == "on":
			parts[k] = "~"
		elif part.startswith("on(") and part.endswith(")"):
			parts[k] = "~" + part[3:-1]
	return ",".join(parts)


def _parse_statement(text: str, file: str, line: int) -> Stmt:
	if text.startswith("if "):
		# if EXPR { ... }: a group kept only while EXPR holds (the rest of the line, spaces and all)
		return Stmt("at", [], {"when": text[3:].strip()}, [], file, line)
	tokens = tokenize(text)
	name = ""
	if len(tokens) > 2 and tokens[1] == "=":
		# desk = box ...: the line's shapes by name, for later lines to place things against.
		name, tokens = tokens[0], tokens[2:]
		if not re.fullmatch(r"[a-z_][a-z0-9_]*", name) or name in _RESERVED:
			raise PartScriptError(f"{name!r} can't name a shape: a lower_snake_case word that is not {', '.join(sorted(_RESERVED))}", file, line)
	if tokens[0] == "stack":
		tokens = ["row", "z", *tokens[1:]]  # stack { ... }: a row going up
	called = False
	if ALIASES.get(tokens[0], tokens[0]) not in SHAPES and tokens[0] not in _TOP and re.fullmatch(r"[a-z_][a-z0-9_]*(\.[a-z_][a-z0-9_]*)*", tokens[0]):
		tokens, called = ["use", *tokens], True  # stool at=...: a part called by name, as use stool at=...
	op = ALIASES.get(tokens[0], tokens[0])
	args, opts, mods = [], {}, []
	words = _modifier_words(tokens[1:], file, line) if op in SIGNATURES else tokens[1:]
	for token in words:
		if _MOD.match(token) or token in ("mx", "my", "mz"):
			mods.append(token)
		elif "=" in token and not token.startswith('"') and re.match(r"^[A-Za-z_]\w*=", token):
			key, value = token.split("=", 1)
			if key in opts:
				raise PartScriptError(f"{key}= given twice", file, line)
			opts[key] = _unquote(value)
		else:
			args.append(token)
	if tokens[0] == "label":
		opts.setdefault("printed", "1")
	loose = [t for t in args if t in ("and", "or", "not")]
	if loose:
		raise PartScriptError(f"'{loose[0]}' on its own: an expression with spaces goes in quotes (when=\"i<3 or i>7\")", file, line)
	if op in SIGNATURES:
		args, opts = _named(op, args, opts, file, line)
	return Stmt(op, args, opts, mods, file, line, name=name, called=called)


_TOP = frozenset({"prop", "def", "kit", "building", "style", "dressing", "import", "end", "theme"})  # words that start blocks, not calls
_RESERVED = frozenset({"i", "pi", "tau", "on", "w", "d", "h", "level", "length", "rand", "pick", "here", "noise", "rough", "x", "y"})
_CENTRED = {"b", "bb", "c", "cone", "sph", "tube", "wedge", "torus", "frame"}  # shapes on= sits on (their at= is a centre)


def _named(op: str, args: list, opts: dict, file: str, line: int) -> tuple[list, dict]:
	"""Named arguments moved into their places, long option names to short, on to ~."""
	renamed: dict = {}
	given: dict = {}  # short name -> the name it was given as
	for key, value in opts.items():
		short = OPTION_NAMES.get(key) or {"material": "mat"}.get(key)
		if key == "sides" and op == "sign":
			short = "sides"  # a wrapped label's facets (its sleeve), not a shape's
		elif key in ("sides", "scale"):
			if key == "sides" and op not in _SIDED_OPS or key == "scale" and op not in ("use", "at"):
				raise PartScriptError(f"{op}: no {key}= ({'scale= is for use and group' if key == 'scale' else 'sides= is for round shapes'})", file, line)
			short = "s"
		short = short or key
		if short in renamed:
			raise PartScriptError(f"{given[short]}= and {key}= are the same option; give one", file, line)
		renamed[short] = value
		given[short] = key
	opts = renamed
	side = opts["on"].partition(".")[2] if "on" in opts else ""
	if "on" in opts and op not in ("join", "link", "chain") and side in ("", "top"):
		# on=desk: at= is measured from the middle of desk's top, and a shape sits on it unless at= says how high.
		at = opts.get("at")
		if at is None and not args:
			opts["at"] = "0,0,on" if op in _CENTRED else "0,0,0"
		elif at is not None and len(split_top(at)) == 2:
			opts["at"] = at + (",on" if op in _CENTRED else ",0")
	elif "on" in opts and op not in ("join", "link", "chain"):
		# on=drawers.front: against that side; at= is two numbers along it (across, then up), from its middle.
		at = opts.get("at")
		if at is None and not args:
			opts["at"] = "0,0,0"
		elif at is not None and len(split_top(at)) == 2:
			a, b = split_top(at)
			opts["at"] = {"front": f"{a},0,{b}", "back": f"{a},0,{b}", "left": f"0,{a},{b}", "right": f"0,{a},{b}",
				"bottom": f"{a},{b},0"}.get(side, at)
	if op in _CENTRED and "at" not in opts and not args and "from" not in opts:
		opts["at"] = "0,0,on"  # a shape without at= stands at the origin
	if SIGNATURES.get(op, ("",))[:1] == ("at",) and op not in ("use", "at", "row", "link") and "at" not in opts \
			and (not args or (op in _POINTS and ":" in args[0])) and any(k in opts for k in SIGNATURES[op][1:]):
		opts["at"] = "0,0,0"  # any other shape without at= is drawn about the origin
	if "from" in opts and "to" in opts and op in ("b", "bb", "c", "cone") and not args:
		# box/cylinder from=A to=B: a beam or rod between two points (size= or radius= across it).
		opts.setdefault("at", "0,0,0")
		if op in ("c", "cone"):
			opts.setdefault("height", "0")
		elif "size" in opts and len(split_top(opts["size"])) == 2:
			opts["size"] = "0," + opts["size"]
		elif "size" in opts and len(split_top(opts["size"])) == 1:
			opts["size"] = f"0,{opts['size']},{opts['size']}"
	signature = SIGNATURES[op]
	named = [key for key in signature if key in opts]
	if named:
		if len(args) > signature.index(named[0]) and op not in _POINTS:
			raise PartScriptError(f"{named[0]}= is also given by position; use one or the other", file, line)
		positional = list(args[:signature.index(named[0])])
		points = args[len(positional):] if op in _POINTS else []
		for key in signature[len(positional):]:
			if key not in opts:
				if op in ("use", "at") and key == "at":
					break
				raise PartScriptError(f"{op}: {key}= is missing (it takes {' '.join(k + '=' for k in signature)})", file, line)
			positional.append(f'"{opts[key]}"' if key == "text" else opts.pop(key))
			if key == "text":
				opts.pop(key)
		args = positional + points
	if "points" in opts:
		args = args + opts.pop("points").split()
	return [_on(a) for a in args], {k: (_on(v) if k == "at" else v) for k, v in opts.items()}


def _join_continued(lines: list[str]) -> list:
	"""Lines ending in a backslash carry on into the next: the joined line keeps the first's number and
	the lines it swallowed become None (so later line numbers do not move)."""
	out: list = []
	carry = None
	for line in lines:
		code = _strip_comment(line).rstrip()
		if carry is not None:
			out[carry] = out[carry][:-1].rstrip() + " " + code.strip() if code.endswith("\\") else out[carry][:-1].rstrip() + " " + code.strip()
			out.append(None)
			if not code.endswith("\\"):
				carry = None
			continue
		out.append(code if code.endswith("\\") else line)
		if code.endswith("\\"):
			carry = len(out) - 1
	return out


def parse(text: str, file: str = "<text>", program: Program | None = None) -> Program:
	"""Props, defs and file variables from PartScript text. Errors are collected, not raised."""
	program = program or Program()
	program.files.append(file)
	raw: list[tuple[int, str]] = []
	# An unindented set is file scope wherever it stands: it ends the prop or def above it.
	top_sets: set[int] = set()
	for number, line in enumerate(_join_continued(text.splitlines()), 1):
		if line is None:
			continue
		if line[:1] not in (" ", "\t") and line.startswith("set "):
			top_sets.add(number)
		line = _strip_comment(line).strip()
		if not line:
			continue
		try:
			for statement in _split_statements(line):
				raw.append((number, statement))
		except ValueError as error:
			program.errors.append(PartScriptError(str(error), file, number))
	index = 0
	while index < len(raw):
		number, statement = raw[index]
		head = statement.split(None, 1)[0]
		if head in ("prop", "building", "kit", "def", "style"):
			block_end = _block_end(raw, index + 1, top_sets)  # a block whose header is wrong is still skipped whole
		else:
			block_end = index + 1
		try:
			if head in ("prop", "building"):
				end = _block_end(raw, index + 1, top_sets)
				block = raw[index:end]
				if head == "building":
					block = [(block[0][0], "prop" + block[0][1][len("building"):])] + block[1:]
				before = len(program.props)
				_parse_prop(block, file, program)
				for built in program.props[before:]:
					built.kind = "building" if head == "building" else "prop"
					if head == "building" and not built.opts.get("kit"):
						program.errors.append(PartScriptError(f"building {built.name}: name its set of pieces with kit=NAME", file, number))
				index = end
				continue
			if head == "kit":
				from . import building
				end = _block_end(raw, index + 1, top_sets)
				building.parse_kit(raw[index:end], file, program)
				index = end
				continue
			if head == "def":
				end = _block_end(raw, index + 1, top_sets)
				_parse_macro(raw[index:end], file, program)
				index = end
				continue
			if head == "style":
				end = _block_end(raw, index + 1, top_sets)
				_parse_style(raw[index:end], file, program)
				index = end
				continue
			if head == "dressing":
				tokens = tokenize(statement)
				if len(tokens) < 3:
					raise PartScriptError("dressing BUCKET piece[:radius] ...", file, number)
				for token in tokens[2:]:
					if "=" in token:
						# key=piece adds to a named bucket (signs: sign kind -> piece, used by terrace units).
						key, _, piece = token.partition("=")
						named = program.dressing.setdefault(tokens[1], {})
						if not isinstance(named, dict):
							raise PartScriptError(f"dressing {tokens[1]}: mix of list and key=piece entries", file, number)
						named[key] = piece
						continue
					items = program.dressing.setdefault(tokens[1], [])
					if not isinstance(items, list):
						raise PartScriptError(f"dressing {tokens[1]}: mix of list and key=piece entries", file, number)
					name, _, radius = token.partition(":")
					items.append([name, float(radius)] if radius else name)
				index += 1
				continue
			if head == "set":
				stmt = _parse_statement(statement, file, number)
				program.file_env.setdefault(file, {}).update(stmt.opts)
			elif head == "import":
				tokens = tokenize(statement)
				alias = tokens[3] if len(tokens) == 4 and tokens[2] == "as" else None
				if len(tokens) not in (2, 4) or not tokens[1].startswith('"') or (len(tokens) == 4 and alias is None):
					raise PartScriptError('import "path/to/file.parts" [as NAME] (a file or a folder, from this file\'s folder)', file, number)
				if alias is not None and not re.fullmatch(r"[a-z][a-z0-9_]*", alias):
					raise PartScriptError(f"import ... as {alias}: a lower_snake_case name", file, number)
				program.imports.append((file, number, _unquote(tokens[1]), alias))
			elif head in ("end", "theme"):
				pass
			else:
				raise PartScriptError(f"'{head}' outside a prop, def, kit or building (start one with 'prop NAME')", file, number)
		except PartScriptError as error:
			program.errors.append(error)
			index = block_end
			continue
		except ValueError as error:
			program.errors.append(PartScriptError(str(error), file, number))
			index = block_end
			continue
		index += 1
	return program


def _block_end(raw: list, start: int, top_sets: set[int] | frozenset = frozenset()) -> int:
	index = start
	while index < len(raw):
		head = raw[index][1].split(None, 1)[0]
		if head in ("prop", "def", "style", "dressing", "kit", "building") or (head == "set" and raw[index][0] in top_sets):
			return index
		if head == "end":
			return index + 1
		index += 1
	return index


def _body(raw: list, file: str, errors: list | None = None, scope: str = "") -> list[Stmt]:
	"""Statements with at { } blocks nested. A bad statement is reported (to errors) and skipped;
	the rest of the prop still parses. scope (the prop's or def's name) goes into each line's seed key."""
	stack: list[list] = [[]]
	openers: list[Stmt] = []
	seen: dict[str, int] = {}
	for number, statement in raw:
		if statement == "end":
			continue
		words = " ".join(statement.split())
		seen[words] = seen.get(words, 0) + 1
		key = f"{scope}#{hashlib.sha1(words.encode()).hexdigest()[:12]}#{seen[words]}"
		if errors is not None:
			try:
				_body_line(statement, number, file, stack, openers, key)
			except (PartScriptError, ValueError) as error:
				errors.append(error if isinstance(error, PartScriptError) else PartScriptError(str(error), file, number))
			continue
		_body_line(statement, number, file, stack, openers, key)
	if openers:
		raise PartScriptError("unclosed block (add a '}' line)", file, openers[-1].line)
	return stack[0]


def _body_line(statement: str, number: int, file: str, stack: list, openers: list, key: str = "") -> None:
	if statement == "}":
		if not openers:
			raise PartScriptError("'}' without a block to close", file, number)
		opener = openers.pop()
		opener.block = stack.pop()
		stack[-1].append(opener)
		return
	opens = statement.endswith("{")
	stmt = _parse_statement(statement[:-1].strip() if opens else statement, file, number)
	stmt.key = key
	if stmt.op not in SHAPES:
		raise PartScriptError(f"unknown statement '{stmt.op}' (shapes: {', '.join(sorted(SHAPES - {'set', 'part', 'size'}))})", file, number)
	if opens:
		if stmt.op not in ("at", "row"):
			raise PartScriptError("only group, if, row and stack open a { block }", file, number)
		openers.append(stmt)
		stack.append([])
	else:
		stack[-1].append(stmt)


def _parse_prop(raw: list, file: str, program: Program) -> None:
	number, header = raw[0]
	head = tokenize(header)
	# for v=a,b,c clauses expand the whole block by text substitution.
	fors, keep, i = [], [], 1
	while i < len(head):
		if head[i] == "for" and i + 1 < len(head) and "=" in head[i + 1]:
			key, values = head[i + 1].split("=", 1)
			fors.append((key, [v for v in values.split(",") if v != ""]))
			i += 2
		else:
			keep.append(head[i])
			i += 1
	if fors:
		combos = [{}]
		for key, values in fors:
			combos = [{**combo, key: value} for combo in combos for value in values]
		for combo in combos:
			def sub(text: str) -> str:
				for key, value in combo.items():
					text = text.replace("{" + key + "}", value)
				return text
			expanded = [(number, sub("prop " + " ".join(keep)))] + [(n, sub(s)) for n, s in raw[1:]]
			_parse_prop(expanded, file, program)
		return
	if not keep:
		raise PartScriptError("prop needs a name: prop NAME \"Title\" [subcategory]", file, number)
	name = keep[0]
	if not re.fullmatch(r"[a-z][a-z0-9_]{1,60}", name):
		raise PartScriptError(f"prop name {name!r}: lower_snake_case (a {{v}} in it needs a 'for v=...')", file, number)
	title, subcategory, opts = "", "props", {}
	for token in keep[1:]:
		if token.startswith('"'):
			title = _unquote(token)
		elif "=" in token:
			key, value = token.split("=", 1)
			opts[key] = _unquote(value)
		else:
			subcategory = token
	program.props.append(Prop(program.qualified(name, file), title or name.replace("_", " ").title(), subcategory, opts,
		_body(raw[1:], file, program.errors, name), file, number))


STYLE_LAYERS = ("wall", "corner", "center", "surface", "decor", "small", "debris", "decals")
_STYLE_BOOLS = {"hero", "once", "surface", "stack", "double", "grid", "things_on_top", "exterior"}


def _style_value(key: str, value: str):
	if key in _STYLE_BOOLS:
		return value.lower() in ("1", "true", "yes")
	items = value.split(",")
	def one(v: str):
		try:
			f = float(v)
			return int(f) if f.is_integer() else f
		except ValueError:
			return v
	return [one(v) for v in items] if len(items) > 1 else one(value)


def _parse_style(raw: list, file: str, program: Program) -> None:
	"""style NAME [exterior=1] / LAYER piece [key=value ...] lines: a room dresser style."""
	number, header = raw[0]
	head = tokenize(header)
	if len(head) < 2 or not re.fullmatch(r"[a-z][a-z0-9_]*", head[1]):
		raise PartScriptError("style needs a lower_snake_case name: style NAME [exterior=1]", file, number)
	style: dict = {}
	for token in head[2:]:
		key, _, value = token.partition("=")
		style[key] = _style_value(key, value)
	for line_number, statement in raw[1:]:
		if statement == "end":
			continue
		tokens = tokenize(statement)
		layer = tokens[0]
		if layer not in STYLE_LAYERS:
			raise PartScriptError(f"style layer {layer!r}: one of {', '.join(STYLE_LAYERS)}", file, line_number)
		if layer == "decals":
			style.setdefault("decals", []).extend(tokens[1:])
			continue
		if len(tokens) < 2:
			raise PartScriptError(f"{layer} needs a piece: {layer} PIECE [repeat=1,3 hero=1 front=PIECE ...]", file, line_number)
		entry: dict = {"piece": tokens[1]}
		for token in tokens[2:]:
			key, _, value = token.partition("=")
			entry[key] = _style_value(key, value) if value else True
		style.setdefault(layer, []).append(entry if len(entry) > 1 else tokens[1])
	program.styles[head[1]] = style
	program.style_lines[head[1]] = (file, number)


def _parse_macro(raw: list, file: str, program: Program) -> None:
	number, header = raw[0]
	head = tokenize(header)
	if len(head) < 2 or not re.fullmatch(r"[a-z][a-z0-9_]*", head[1]):
		raise PartScriptError("def needs a lower_snake_case name: def NAME [param=default ...]", file, number)
	params = {}
	for token in head[2:]:
		if "=" not in token:
			raise PartScriptError(f"def parameters need defaults: {token}=...", file, number)
		key, value = token.split("=", 1)
		if key in USE_OPTIONS:
			raise PartScriptError(f"def parameter {key!r} is taken by use ({', '.join(USE_OPTIONS)} place the part); call it something else (rad for a radius)", file, number)
		params[key] = _unquote(value)
	name = program.qualified(head[1], file)
	if name in program.macros and program.macros[name].file != "std.parts":
		before = program.macros[name]
		raise PartScriptError(f"def {head[1]} defined twice (also {before.file}:{before.line})", file, number)
	program.macros[name] = Macro(name, params, _body(raw[1:], file, program.errors, name), file, number)


# ------------------------------------------------------------------ materials
FINISHES = ("paint", "metal", "plastic", "rubber", "fabric", "wood", "plaster", "concrete", "stone", "glow", "glass")
_COLOUR = re.compile(r"^#([0-9a-fA-F]{6})(?:/(\w+))?$")


def colour_key(prefix: str, token: str) -> tuple[str, str, str] | None:
	"""(material key, hex, finish) for a #rrggbb[/finish] token."""
	match = _COLOUR.match(token)
	if not match:
		return None
	hex_, finish = match.group(1).lower(), (match.group(2) or "paint").lower()
	if finish not in FINISHES:
		raise ValueError(f"colour finish {finish!r}: one of {', '.join(FINISHES)}")
	return f"{prefix + '_' if prefix else ''}x{hex_}_{finish}", hex_, finish

def colour_material_kwargs(hex_: str, finish: str) -> dict:
	rgb = tuple(int(hex_[i:i + 2], 16) / 255.0 for i in (0, 2, 4))
	if finish == "glow":
		return {"emission_color": rgb, "emission_strength": 1.25, "ao": False}
	if finish == "glass":
		return {"roughness": 0.3, "alpha": 0.4, "ao": False}
	if finish == "metal":
		return {"roughness": 0.9}
	return {}


def sign_key(prefix: str, stmt_args: dict) -> str:
	digest = hashlib.sha1(json.dumps(stmt_args, sort_keys=True).encode()).hexdigest()[:8]
	return f"{prefix + '_' if prefix else ''}sign_{digest}"

def expand_points(tokens: list, env: dict) -> list[str]:
	"""A shape's point tokens with any variable that holds a line of points spelled out:
	set path="0,0,0 .5,0,1 1,0,0"  then  pipe mat=wood radius=.02 path."""
	out = []
	for token in tokens:
		value = env.get(token)
		out += value.split() if isinstance(value, str) and len(value.split()) > 1 else [token]
	return out


def material_items(token: str, env: dict) -> list[str]:
	"""A material token's cycle (a|b|c), with parameters looked up (a parameter may hold a cycle)."""
	out = []
	for item in token.split("|"):
		value = str(env.get(item, item)) if not item.startswith("#") else item
		for picked in resolve_picks(value, env).split("|"):
			# pick(stone,dark) may choose a variable that holds a material
			held = env.get(picked) if not picked.startswith("#") else None
			out.extend(resolve_picks(held, env).split("|") if isinstance(held, str) else [picked])
	return out


def _arg_value(value: str, env: dict):
	"""A use/def argument: a number when it evaluates, else the text (a material, a word)."""
	try:
		return evaluate(value, env)
	except ValueError:
		return env.get(value, value)
