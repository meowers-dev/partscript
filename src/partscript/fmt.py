"""Rewrite PartScript between its terse form and its readable form, keeping layout and comments.

	b 0,0,~ .4,.3,.2 wood r=0,0,45 *5@.3,0,0 mx
	box at=0,0,on size=.4,.3,.2 mat=wood turn=0,0,45 repeat 5 every .3,0,0 mirror x

Only shape, use and group lines change; headers (prop, def, set, kit, building...) and everything a
line says stay as they are. readable(terse(text)) parses to the same program as text.
"""

from __future__ import annotations

import re

from .lang import (_CENTRED, _MOD, _POINTS, _SIDED_OPS, ALIASES, OPTION_NAMES, SIGNATURES, _join_continued, _parse_statement, _split_statements,
	_strip_comment, split_top)

WIDTH = 120  # readable lines longer than this wrap, carrying on with a backslash
TAIL = 30  # but not to leave a carried-on line shorter than this

LONG = {"b": "box", "bb": "bevel_box", "bx": "box_between", "c": "cylinder", "sph": "sphere", "pan": "panel", "ext": "extrude", "at": "group",
	"trim": "decal", "archwall": "arch_wall"}
SHORT_OPTS = {short: long for long, short in OPTION_NAMES.items() if long not in ("rotate",)}


def readable(text: str) -> str:
	return _rewrite(text, True)


def terse(text: str) -> str:
	return _rewrite(text, False)


def _rewrite(text: str, long: bool) -> str:
	lines = [line for line in _join_continued(text.split("\n")) if line is not None]
	return "\n".join(_wrap(_line(line, long)) if long else _line(line, long) for line in lines)


def _wrap(line: str) -> str:
	"""A long statement line broken between words, each carried-on line indented under it."""
	code = _strip_comment(line)
	if len(line) <= WIDTH or ";" in code or "{" in code or "}" in code or code != line:
		return line
	indent = line[:len(line) - len(line.lstrip())]
	words, rows, row = _words(line.strip()), [], ""
	for word in words:
		if row and len(indent) + len(row) + 1 + len(word) > WIDTH - 2:
			rows.append(row)
			row = word
		else:
			row = f"{row} {word}" if row else word
	rows.append(row)
	# A short last piece goes back on the line before it (one long-ish line beats a stub of a line).
	while len(rows) > 1 and len(rows[-1]) < TAIL:
		tail = rows.pop()
		rows[-1] += " " + tail
	return indent + (" \\\n" + indent + "    ").join(rows)


def _words(text: str) -> list[str]:
	"""Words of a line, keeping quoted text and the words of a copies phrase (repeat 5 every V) together."""
	from .lang import tokenize
	tokens, out, k = tokenize(text), [], 0
	joins = {"repeat": 3, "grid": 3, "ring": 3, "scatter": 5, "mirror": 1, "as": 1}
	while k < len(tokens):
		word = tokens[k]
		take = joins.get(word, 0)
		if take and k + take < len(tokens) + 1:
			group = tokens[k:k + 1 + take]
			# Only as many as the phrase really has (ring 6 alone, scatter N within R without apart G).
			if word == "ring" and (len(group) < 3 or group[2] != "step"):
				group = group[:2]
			if word in ("repeat", "grid") and (len(group) < 3 or group[2] != "every"):
				group = group[:2]
			if word == "scatter" and (len(group) < 5 or group[4] != "apart"):
				group = group[:4]
			out.append(" ".join(group))
			k += len(group)
		else:
			out.append(word)
			k += 1
	return out


def _line(line: str, long: bool) -> str:
	code = _strip_comment(line)
	comment = line[len(code):]
	stripped = code.strip()
	if not stripped:
		return line
	indent = code[:len(code) - len(code.lstrip())]
	pieces = _split_statements(stripped)
	out = []
	for k, piece in enumerate(pieces):
		opener = piece.endswith("{")
		body = piece[:-1].strip() if opener else piece
		named = re.match(r"^([a-z_][a-z0-9_]*) = (.+)$", body)
		name, body = (named.group(1) + " = ", named.group(2)) if named else ("", body)
		head = body.split(None, 1)[0] if body else ""
		if body and body != "}" and (ALIASES.get(head, head) in SIGNATURES or head == "stack"):
			body = _statement(body, long)
		out.append(name + body + (" {" if opener else ""))
	joined = ""
	for k, piece in enumerate(out):
		if k == 0:
			joined = piece
		elif out[k - 1].endswith("{") or piece == "}":
			joined += " " + piece
		else:
			joined += " ; " + piece
	trailing = code[len(code.rstrip()):]
	return indent + joined + (trailing if comment else "") + comment


def _quote(value: str) -> str:
	return f'"{value}"' if (" " in value or value == "") else value


def _statement(text: str, long: bool) -> str:
	stmt = _parse_statement(text, "<fmt>", 0)
	word = text.split(None, 1)[0]
	op = stmt.op
	opts = dict(stmt.opts)
	if word == "label":
		opts.pop("printed", None)
	signature = SIGNATURES[op]
	args = list(stmt.args)
	if op == "row":
		# row x / stack: the same in both forms
		words = ["stack"] if args[:1] == ["z"] else ["row", *args[:1]]
		names = opts.pop("index", None)
		if "on" in opts and opts.get("at", "").endswith(",0") and len(split_top(opts["at"])) == 3:
			opts["at"] = opts["at"][:-2]
		opts = {**{k: opts.pop(k) for k in ("on", "at") if k in opts}, **opts}
		for key, value in opts.items():
			name = SHORT_OPTS.get(key, key) if long else key
			words.append(f"{name}={_on_word(value) if key == 'at' else _quote(value)}")
		return " ".join(words + (_modifiers(stmt.mods) if long else stmt.mods) + ([f"as {names}"] if names else []))
	words = [("label" if word == "label" else LONG.get(op, op)) if long else ("label" if word == "label" else op)]
	if long:
		# what a line is placed against reads first: box from=A to=B ..., use lamp on=desk at=...
		beam = "from" in opts and "to" in opts and op in ("b", "bb", "c", "cone") and args[:1] == ["0,0,0"]
		words += [f"{key}={_on_word(opts.pop(key))}" for key in ("from", "to") if key in opts and op != "bx"]
		lead = [f"on={opts.pop('on')}"] if "on" in opts else []
		if beam:
			# a beam from=A to=B: no at=, and size= only across it
			args[0] = ""
			if op in ("c", "cone"):
				args[2] = ""
			elif args[1].startswith("0,"):
				across = split_top(args[1])[1:]
				args[1] = across[0] if len(set(across)) == 1 else ",".join(across)
		if lead and "at" in signature:
			# on=desk: at= is measured from desk's top; what it filled in by itself goes unsaid
			k = signature.index("at")
			if k < len(args):
				parts = split_top(args[k])
				if args[k] in ("0,0,~", "0,0,0"):
					args[k] = ""
				elif len(parts) == 3 and parts[2] in ("~", "0"):
					args[k] = ",".join(parts[:2])
		if (op in _CENTRED and args[:1] == ["0,0,~"]) or (op not in _CENTRED and signature[:1] == ("at",) and op not in ("use", "at", "row",
				"link") and args[:1] == ["0,0,0"]):
			args[0] = ""  # standing at the origin is what a shape without at= does
		fixed = len(signature) if op not in ("use",) else 1
		if op != "use":
			words += lead
		for key, value in zip(signature[:fixed], args):
			if value == "":
				continue
			if op == "use" and key == "name":
				words += [value, *lead]
			elif key == "text":
				words.append(f"text={value}")
			else:
				words.append(f"{key}={_on_word(value) if key in ('at', 'from', 'to') else value}")
		rest = args[fixed:]
		if op == "use" and rest:
			if rest[0] != "":
				words.append(f"at={_on_word(rest[0])}")
			rest = rest[1:]
		words += rest
	else:
		words += args
	names = opts.pop("index", None) if long else None
	facing = opts.pop("facing", "") if long and any("^" in m for m in stmt.mods) else ""
	for key, value in opts.items():
		name = key
		if long:
			if key == "s":
				name = "sides" if op in _SIDED_OPS else "scale" if op in ("use", "at") else key
			else:
				name = SHORT_OPTS.get(key, key)
		words.append(f"{name}={_quote(value)}")
	words += _modifiers(stmt.mods, facing) if long else stmt.mods
	if names:
		words.append(f"as {names}")
	return " ".join(words)


def _on_word(value: str) -> str:
	parts = split_top(value)
	return ",".join("on" if p == "~" else f"on({p[1:]})" if p.startswith("~") else p for p in parts)


def _bare(count: str) -> str:
	return count[1:-1] if count.startswith("(") and count.endswith(")") and re.fullmatch(r"\(\w+\)", count) else count


def _modifiers(mods: list[str], facing: str = "") -> list[str]:
	out, mirrors = [], ""
	for mod in mods:
		if mod in ("mx", "my", "mz"):
			mirrors += mod[1]
			continue
		match = _MOD.match(mod)
		if match and match.group(1):
			counts = match.group(1)
			grid = "x" in re.sub(r"\([^)]*\)", "", counts)
			text = ("grid " if grid else "repeat ") + "x".join(_bare(c) for c in split_top(counts, "x")) if grid else "repeat " + _bare(counts)
			out.append(text + (f" every {match.group(2)}" if match.group(2) else ""))
		elif match and match.group(3):
			count, step = match.group(3), match.group(4)
			out.append(f"ring {_bare(count)}" + ("" if step == f"(360/{count})" else f" step {step}"))
		elif match and match.group(7):
			target, _, gap = match.group(8).partition(",")
			out.append(f"scatter {_bare(match.group(7))} on {target}" + (f" facing {facing}" if facing else "") + (f" apart {gap}" if gap else ""))
		elif match and match.group(5):
			count, values = _bare(match.group(5)), split_top(match.group(6))
			if len(values) == 1:
				out.append(f"scatter {count} within {values[0]}")
			elif len(values) == 3 and values[1] in ("0", "0.0"):
				out.append(f"scatter {count} within {values[0]} apart {values[2]}")
			else:
				out.append(f"scatter {count} over {values[0]},{values[1]}" + (f" apart {values[2]}" if len(values) > 2 else ""))
		else:
			out.append(mod)
	if mirrors:
		out.append(f"mirror {mirrors}")
	return out
