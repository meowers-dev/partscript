"""Build speed. Every std part as a prop, the golden shapes and the examples, built N rounds.

	uv run python bench/bench.py              # cold (textures made) and warm (textures cached) timings
	uv run python bench/bench.py --profile    # where the time goes
"""

from __future__ import annotations

import argparse
import cProfile
import pstats
import sys
import tempfile
import time
from pathlib import Path

import partscript as ps
from partscript.host import Host
from partscript.project import STD

ROOT = Path(__file__).resolve().parents[1]


def workload() -> list[tuple[str, str]]:
	program = ps.load_program([])
	std_props = "\n".join(f'prop std_{name} "{name}"\n  use {name}\n' for name in sorted(program.macros))
	sources = [("std_props.parts", std_props), ("shapes.parts", (ROOT / "tests" / "golden" / "shapes.parts").read_text())]
	sources += [(p.name, p.read_text()) for p in sorted((ROOT / "examples").glob("*.parts"))]
	return sources


def run(cache_dir, rounds: int = 1) -> tuple[float, int, int]:
	started = time.perf_counter()
	count = triangles = 0
	for _ in range(rounds):
		project = ps.Project(workload(), Host(cache_dir=cache_dir))
		for prop in project.props():
			built = project.build(prop["id"])
			count += 1
			triangles += built.triangles
	return time.perf_counter() - started, count, triangles


def main() -> int:
	parser = argparse.ArgumentParser()
	parser.add_argument("--rounds", type=int, default=3)
	parser.add_argument("--profile", action="store_true")
	args = parser.parse_args()
	with tempfile.TemporaryDirectory() as cache:
		cold, count, triangles = run(None, 1)
		print(f"cold (no texture cache): {count} props, {triangles} triangles, {cold:.3f} s, {cold / count * 1000:.2f} ms/prop")
		run(cache, 1)
		if args.profile:
			profile = cProfile.Profile()
			profile.enable()
		warm, count, triangles = run(cache, args.rounds)
		if args.profile:
			profile.disable()
			pstats.Stats(profile).sort_stats("tottime").print_stats(20)
		print(f"warm (textures cached):  {count} props, {warm:.3f} s, {warm / count * 1000:.2f} ms/prop")
	return 0


if __name__ == "__main__":
	sys.exit(main())
