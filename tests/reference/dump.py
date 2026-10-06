"""Dumps what the Python implementation makes, as the reference the Rust port is held to.

	uv run python tests/reference/dump.py

Writes tests/reference/*.json.gz: every example, docs demo and golden shape built (baked faces, tones,
snaps, steps, the glTF JSON), the checker's reports, buildings as data, the formatter's output, every
texture's pixels, and the low-level pieces (random streams, float formatting, rounding, expressions).
"""

from __future__ import annotations

import base64
import gzip
import json
import math
import random
import re
import struct
import sys
from pathlib import Path

import numpy as np

import partscript as ps
from partscript import fmt
from partscript.host import Host
from partscript.lang import evaluate, interpolate
from partscript.project import STD

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent
LIBRARY = {f"../library/{p.name}": p.read_text() for p in sorted((ROOT / "library").glob("*.parts"))}


def reader(target: str) -> list:
	return [(target, LIBRARY[target])] if target in LIBRARY else []


def write(name: str, data) -> None:
	text = json.dumps(data, separators=(",", ":"))
	(OUT / f"{name}.json.gz").write_bytes(gzip.compress(text.encode(), 9, mtime=0))
	print(f"{name}: {len(text) // 1024} KB json")


def glb_json(glb: bytes) -> str:
	length = struct.unpack_from("<I", glb, 12)[0]
	return glb[20:20 + length].decode().rstrip(" ")


def model(project, prop_id: str, seed=None) -> dict:
	built = project.build(prop_id, steps=True, snaps=True, seed=seed)
	parts = []
	for baked in built.baked:
		polys = []
		for q in baked["polygons"]:
			polys.append({"m": baked["materials"].index(q["face"].material), "step": q["step"], "origin": q.get("origin", 0),
				"co": [v for c in q["coords"] for v in c], "uv": [v for uv in q["uvs"] for v in uv], "tone": q["colours"],
				"tris": [v for t in q["triangles"] for v in t], "n": list(q["normal"])})
		parts.append({"name": baked["name"], "smooth": baked["smooth"], "materials": baked["materials"], "polys": polys})
	return {"id": built.asset_id, "triangles": built.triangles, "warnings": built.warnings,
		"snaps": [s.as_dict() for s in built.snaps], "joints": [s.as_dict() for s in built.joints], "links": [s.as_dict() for s in built.links],
		"steps": [[step["stmt"].line, sum(end - first for _, first, end in step["faces"]), step.get("note", "")] for step in built.steps],
		"origins": [[list(o) for o in origin] for origin in built.origins], "parts": parts, "gltf": glb_json(built.glb),
		"bin_len": len(built.glb)}


def examples_project() -> ps.Project:
	std = ps.load_program([])
	sources = [(p.name, p.read_text()) for p in sorted((ROOT / "examples").glob("*.parts"))]
	sources.append(("std parts", "\n".join(f'prop std_{n} "{n}"\n  use {n}\n' for n in sorted(std.macros))))
	return ps.Project(sources, Host(), reader=reader)


def doc_blocks() -> list:
	out = []
	for page in sorted((ROOT / "docs").rglob("*.md")):
		for k, m in enumerate(re.finditer(r"^```parts[^\n]*\n(.*?)^```", page.read_text(), re.S | re.M)):
			out.append((f"{page.relative_to(ROOT / 'docs')}#{k}", m.group(1)))
	return out


def textures(host) -> dict:
	out = {}
	store = host.textures
	for name, recipe in sorted(store.recipes.items()):
		image = store.provider.make(recipe)
		out[name] = {"recipe": json.loads(json.dumps(recipe, default=list)), "shape": list(image.shape),
			"pixels": base64.b64encode(np.ascontiguousarray(image, dtype=np.uint8).tobytes()).decode(),
			"png": base64.b64encode(store.png(name)).decode()}
	return out


def main() -> None:
	# ---------------------------------------------------------- examples
	project = examples_project()
	models = [model(project, p["id"]) for p in project.props() if not p["imported"]]
	models.append(model(project, "weathered_fence", seed=4817))
	models[-1]["id"] = "weathered_fence@4817"
	buildings = {p["id"]: project.building(p["id"]) for p in project.props() if p["kind"] == "building"}
	uses = {p["id"]: project.uses(p["id"]) for p in project.props() if not p["imported"]}
	report = project.check()
	write("examples", {"models": models, "check": report, "buildings": buildings, "uses": uses,
		"props": project.props(), "errors": project.errors})
	write("textures", textures(project.host))

	# ---------------------------------------------------------- docs demos and the golden shapes
	examples = [(p.name, p.read_text()) for p in sorted((ROOT / "examples").glob("*.parts"))]
	demos = []
	for name, code in doc_blocks():
		project = ps.Project(examples + [(name, code)], Host(), reader=reader)
		entry = {"name": name, "code": code, "errors": project.errors, "models": []}
		if not project.errors:
			entry["check"] = {k: v for k, v in project.check().items() if k in ("errors", "warnings")}
			for prop in project.props():
				if prop["file"] == name:
					try:
						entry["models"].append(model(project, prop["id"]))
					except ps.PartScriptError as error:
						entry["models"].append({"id": prop["id"], "error": str(error)})
		demos.append(entry)
	write("docs", demos)
	golden = ps.Project.from_text((ROOT / "tests/golden/shapes.parts").read_text(), "shapes.parts", Host())
	write("golden", [model(golden, p["id"]) for p in golden.props()])

	# ---------------------------------------------------------- the formatter
	files = sorted((ROOT / "examples").glob("*.parts")) + sorted((ROOT / "library").glob("*.parts")) + [STD]
	texts = [(str(p.relative_to(ROOT)) if p != STD else "std.parts", p.read_text()) for p in files] + doc_blocks()
	write("fmt", [{"name": n, "text": t, "readable": fmt.readable(t), "terse": fmt.terse(t)} for n, t in texts])

	# ---------------------------------------------------------- low-level pieces
	write("basics", basics())


def basics() -> dict:
	out: dict = {}
	# random.Random streams: string and int seeds, and every method the code uses.
	streams = []
	for seed in ["", "a", "noise#", "ruin#x", "|v.parts:k#1:0:scatter", "x" * 300, "héllo"] + [0, 1, 7, 2**32 + 5, 1234567891011]:
		rng = random.Random(seed)
		row = {"seed": seed, "random": [rng.random() for _ in range(5)], "uniform": [rng.uniform(-2.5, 7.25) for _ in range(5)],
			"choice": [rng.choice([-1, 1]) for _ in range(8)], "choice7": [rng.choice(list(range(7))) for _ in range(8)]}
		values = list(range(256))
		rng.shuffle(values)
		row["shuffle"] = values
		row["sample"] = [rng.sample(list(range(n)), k) for n, k in ((10, 3), (30, 25), (100, 7), (400, 120), (5, 5))]
		streams.append(row)
	out["random"] = streams
	# numpy Generator streams (textures).
	out["numpy"] = [{"seed": s, "f32": np.random.default_rng(s).random(40, dtype=np.float32).tolist(),
		"f64": np.random.default_rng(s).random(40).tolist()} for s in (0, 1, 7, 996, 123456789, 2**40 + 3)]
	# Floats: repr, :g, round(x, n), round(x), % and //.
	rng = random.Random(5)
	floats = [0.0, -0.0, 1.0, 0.1, 0.2 + 0.1, 1e-05, 1e-4, 123456789012345678.0, 1e16, 1e15, 2.5, 3.5, -2.5, 0.125, 1 / 3, math.pi,
		5e-324, 1.7976931348623157e308, 0.000123456, 1234.5, 99999.95, 0.5, 1.5, 0.045, 2.675, -0.0049999]
	floats += [rng.uniform(-1000, 1000) for _ in range(60)] + [rng.uniform(-1, 1) * 10 ** rng.randint(-8, 18) for _ in range(60)]
	out["floats"] = [{"x": x, "repr": repr(x), "g": f"{x:g}", "g6": "%.6g" % x, "f2": f"{x:.2f}", "r0": round(x), "r4": round(x, 4),
		"r5": round(x, 5), "r6": round(x, 6), "r12": round(x, 12), "mod3": x % 3.0, "modn": x % -2.5, "fdiv": x // 0.7,
		"tuple": repr((round(x, 4), round(-x, 4), 0.0))} for x in floats]
	out["hypot"] = [[a, b, math.hypot(a, b), math.dist((a, b, 1.5), (b, -a, 0.25))] for a, b in
		((rng.uniform(-50, 50), rng.uniform(-50, 50)) for _ in range(200))]
	out["sum"] = [[xs, sum(xs)] for xs in ([rng.uniform(-1e3, 1e3) * 10 ** rng.randint(-6, 6) for _ in range(rng.randint(2, 9))] for _ in range(200))]
	# Expressions: the evaluator against Python's own grammar.
	env = {"a": 2.0, "b": -3.5, "w": 1.2, "i": 3, "k": 0, "name": "1.5+a", "mat": "wood", "__seed__": "s|x.parts:1:0"}
	exprs = ["1+2*3", "-2**2", "2**-1", "(-2)**2", "2**3**2", "7//2", "-7//2", "7%3", "-7%3", "7.5%-2", "1/3", "a*b", "w/2+.1", "-w/2+.02",
		"not a", "not 0", "a and b", "0 or 5", "1 if a > 1 else 2", "0 < a < 3", "1 < 2 > 3", "a == 2.0", "a != 2", "i%2", "i // 2 * 2",
		"min(1,2,3)", "max(a,b)", "floor(-1.5)", "ceil(1.2)", "round(2.5)", "round(3.5)", "round(-0.5)", "abs(-3)", "sqrt(2)", "hypot(3,4)",
		"sin(30)", "cos(60)", "tan(45)", "atan2(1,1)", "atan(1)", "asin(.5)", "acos(.5)", "rad(180)", "deg(pi)", "tau/2", "pi",
		"rand()", "rand(5)", "rand(-2,2)", "rand(1)+rand(1)", "pick(1,2,3)", "pick(4,5)*2", "odds(70,20,10)", "odds(0,5,0)", "odds(1,1)+odds(1,1)",
		"noise(1.5)", "noise(.3,.7)", "rough(1.2,3.4,.5)", "name", "1_000", ".5", "5.", "1e3", "1E-2", "  3 ", "0x10", "+.5", "inf", "-inf",
		"1/0", "unknown", "a.top", "rand(", "2 +", "'x'", "[1]", "a < b < c", "1 is 1", "3 in 4", "a ** 0.5", "(1)(2)", "max()", "x1",
		"((1+2))*3", "--1", "+-1", "not not 1", "1 if 0 else 2 if 1 else 3", "a if b else w", "rand(2) if i else rand(3)"]
	out["expressions"] = []
	for text in exprs:
		try:
			value = evaluate(text, env)
			out["expressions"].append({"text": text, "value": value if math.isfinite(value) else repr(value)})
		except ValueError as error:
			out["expressions"].append({"text": text, "error": str(error)})
	out["interpolate"] = [[t, interpolate(t, env)] for t in ("{a}", "{w*2}", "{floor(rand(1800,1899))}", "pick(ADA,NELL) {i}", "{mat} X", "{1/3}",
		"{name}", "plain")]
	return out


if __name__ == "__main__":
	sys.exit(main())
