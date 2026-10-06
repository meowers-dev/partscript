"""Builds the preview site's models: every example and std part as site/models/<id>.glb + index.json.

	uv run python site/build.py && python3 -m http.server -d site 8765

Each model is step-tagged (TEXCOORD_1.x = the source statement of each face), and index.json gives
each prop its source, the source line of every step and its "built from" tree, so the page can
light up what any line or any part it uses (TEXCOORD_1.y: the use chain) adds.
"""

from __future__ import annotations

import json
from pathlib import Path

import partscript as ps
from partscript.host import Host
from partscript.project import STD

ROOT = Path(__file__).resolve().parents[1]
ORDER = ["ruins.parts", "showpieces.parts", "layout.parts", "tavern.parts", "fairground.parts", "buildings.parts", "paths.parts", "graveyard.parts", "garden.parts", "street.parts", "market.parts", "cafe.parts", "library.parts"]
OUT = ROOT / "site" / "models"
GROUPS = {"fairground.parts": "Fairground", "ruins.parts": "Ruins (generators)", "showpieces.parts": "Showpieces (detail tools)", "layout.parts": "Layout (placing against names)", "tavern.parts": "Tavern (imported libraries)", "buildings.parts": "Buildings", "paths.parts": "Paths and fences (snapping)", "graveyard.parts": "Graveyard (procedural)", "garden.parts": "Garden (variety)", "street.parts": "Street", "market.parts": "Market", "cafe.parts": "Cafe", "library.parts": "Library", "std parts": "Standard parts"}


def main() -> None:
	std = ps.load_program([])
	examples = sorted((ROOT / "examples").glob("*.parts"), key=lambda p: ORDER.index(p.name) if p.name in ORDER else 9)
	sources = [(p.name, p.read_text()) for p in examples]
	sources.append(("std parts", "\n".join(f'prop std_{n} "{n}"\n  use {n}\n' for n in sorted(std.macros))))
	texts = dict(sources) | {"std.parts": STD.read_text()}
	project = ps.Project(sources, Host(), reader=library_reader)
	for label in project.program.imported:  # an imported file's label is NAME:path when it came in with a name
		texts[label] = LIBRARY[label.split(":", 1)[-1]]
	OUT.mkdir(parents=True, exist_ok=True)
	for old in OUT.glob("*.glb"):
		old.unlink()  # models of props that are gone go too
	rows = []
	for prop in [p for p in project.props() if not p["imported"]]:
		built = project.build(prop["id"], steps=True, snaps=True)
		(OUT / f"{built.asset_id}.glb").write_bytes(built.glb)
		rows.append({"id": built.asset_id, "title": prop["title"], "group": GROUPS.get(prop["file"], prop["file"]), "file": prop["file"],
			"line": prop["line"], "triangles": built.triangles, "snaps": len(built.snaps) + len(built.joints), "ms": round(built.seconds * 1000, 1),
			"source": _block(texts[prop["file"]], prop["line"]),
			"steps": [step["stmt"].line for step in built.steps],
			"origins": ["/".join(f"{file}:{line}" for file, line in origin) for origin in built.origins],
			"tree": project.uses(prop["id"])})
	# Source of everything a tree can name (defs, std parts, props), for clicking through.
	blocks = {name: {"file": m.file, "line": m.line, "source": _block(texts[m.file], m.line)} for name, m in project.program.macros.items()}
	blocks |= {p.name: {"file": p.file, "line": p.line, "source": _block(texts[p.file], p.line)} for p in project.program.props}
	(OUT / "index.json").write_text(json.dumps({"props": rows, "blocks": blocks}, indent=1))
	print(f"{len(rows)} models -> {OUT}")
	playground(examples)


# The library the examples import ("../library/x.parts" from an example's folder).
LIBRARY = {f"../library/{p.name}": p.read_text() for p in sorted((ROOT / "library").glob("*.parts"))}


def library_reader(target: str) -> list:
	return [(target, LIBRARY[target])] if target in LIBRARY else []


def playground(examples: list) -> None:
	"""What the playground (play.html) runs in the browser: the packages as a zip for Pyodide, and the examples."""
	import zipfile
	py = ROOT / "site" / "py"
	py.mkdir(exist_ok=True)
	with zipfile.ZipFile(py / "partscript.zip", "w", zipfile.ZIP_DEFLATED) as archive:
		for package in ("partscript", "kitlib"):
			for path in sorted((ROOT / "src" / package).iterdir()):
				if path.suffix in (".py", ".parts"):
					archive.write(path, f"{package}/{path.name}")
	shelf = ROOT / "site" / "examples"
	shelf.mkdir(exist_ok=True)
	for path in examples:
		(shelf / path.name).write_text(path.read_text())
	(shelf / "index.json").write_text(json.dumps([p.name for p in examples]))
	(shelf / "library.json").write_text(json.dumps(LIBRARY))
	print(f"playground: {py / 'partscript.zip'} and {len(examples)} examples")


def _block(text: str, line: int) -> str:
	"""A prop's or def's lines in its file, from its header to the next unindented line."""
	lines = text.splitlines()[line - 1:]
	out = [lines[0]]
	for row in lines[1:]:
		if row and not row[0].isspace():
			if row.strip() == "end":
				out.append(row)
			break
		out.append(row)
	while out and not out[-1].strip():
		out.pop()
	return "\n".join(out)


if __name__ == "__main__":
	main()
