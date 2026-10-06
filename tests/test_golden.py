"""The baked geometry of every shape stays what it was (coordinates to 0.1 mm, tones to 1/500).

Regenerate after an intended change: python tests/test_golden.py --write
"""

from __future__ import annotations

import gzip
import json
import sys
from pathlib import Path

import partscript as ps
from partscript.host import Host

HERE = Path(__file__).resolve().parent
SOURCE = HERE / "golden" / "shapes.parts"
GOLDEN = HERE / "golden" / "shapes.json.gz"
FILE_NAME = "shapes.parts"  # jit= seeds from the file name and the line's words


def snapshot() -> dict:
	project = ps.Project.from_text(SOURCE.read_text(), FILE_NAME, Host())
	out = {}
	for prop in project.props():
		built = project.build(prop["id"], glb=False)
		out[prop["id"]] = [{"name": b["name"], "materials": b["materials"], "polygons": [
			{"m": q["face"].material, "co": [[round(v, 6) for v in c] for c in q["coords"]], "uv": [[round(v, 6) for v in u] for u in q["uvs"]],
				"tone": [round(t, 6) for t in q["colours"]]} for q in b["polygons"]]} for b in built.baked]
	return out


def _close(a: list, b: list, tolerance: float) -> bool:
	return len(a) == len(b) and all(abs(x - y) <= tolerance for x, y in zip(a, b))


def test_baked_geometry_matches_golden() -> None:
	golden = json.loads(gzip.decompress(GOLDEN.read_bytes()))
	now = snapshot()
	assert sorted(now) == sorted(golden)
	for prop, parts in golden.items():
		assert [p["name"] for p in now[prop]] == [p["name"] for p in parts], prop
		for got, want in zip(now[prop], parts):
			assert got["materials"] == want["materials"], prop
			assert len(got["polygons"]) == len(want["polygons"]), prop
			for index, (g, w) in enumerate(zip(got["polygons"], want["polygons"])):
				where = f"{prop} {got['name']} polygon {index}"
				assert g["m"] == w["m"], where
				assert len(g["co"]) == len(w["co"]), where
				assert all(_close(c, d, 1e-4) for c, d in zip(g["co"], w["co"])), where
				assert all(_close(c, d, 1e-4) for c, d in zip(g["uv"], w["uv"])), where
				assert _close(g["tone"], w["tone"], 2e-3), where


if __name__ == "__main__" and "--write" in sys.argv:
	GOLDEN.write_bytes(gzip.compress(json.dumps(snapshot(), separators=(",", ":")).encode(), mtime=0))
	print(f"wrote {GOLDEN}")
