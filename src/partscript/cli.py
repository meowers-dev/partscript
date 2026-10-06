"""partscript: check and build .parts files.

	partscript ref                       the language, in one page
	partscript check props/              parse and check every .parts file (no building)
	partscript build props/ -o out/      every prop as out/<id>.glb
	partscript list props/               props, std parts and materials
	partscript fmt props/a.parts -w      rewrite in words (--terse: in shorthand)
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path

from .host import Host
from .project import Project
from .reference import REFERENCE


def _cache_dir(args) -> Path | None:
	if args.no_cache:
		return None
	if args.cache:
		return Path(args.cache)
	base = os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache"
	return Path(base) / "partscript" / "textures"


def _project(args) -> Project:
	paths = [Path(p) for p in args.paths]
	missing = [str(p) for p in paths if not p.exists()]
	if missing:
		raise SystemExit(f"no such file or directory: {', '.join(missing)}")
	return Project.from_paths(paths, Host(cache_dir=_cache_dir(args)))


def cmd_ref(args) -> int:
	print(REFERENCE)
	return 0


def cmd_check(args) -> int:
	project = _project(args)
	report = project.check()
	if args.json:
		print(json.dumps(report, indent=1))
	else:
		for prop in report["props"]:
			print(f"  {prop['id']:40s} ~{prop['triangles']:6d} tris  {prop['file']}:{prop['line']}")
		for warning in report["warnings"]:
			print(f"warning: {warning}")
		for error in report["errors"]:
			print(f"error: {error}")
		print(f"{len(report['props'])} props, {len(report['errors'])} errors, {len(report['warnings'])} warnings")
	return 1 if report["errors"] else 0


def cmd_build(args) -> int:
	started = time.perf_counter()
	project = _project(args)
	if project.errors:
		for error in project.errors:
			print(f"error: {error}")
		return 1
	result = project.write_all(args.out, args.only.split(",") if args.only else None)
	for row in result["built"]:
		print(f"  {row['id']:40s} {row['triangles']:6d} tris  {row['seconds'] * 1000:7.1f} ms")
	for warning in result["warnings"]:
		print(f"warning: {warning}")
	for error in result["errors"]:
		print(f"error: {error}")
	made = project.host.textures.made
	print(f"{len(result['built'])} props -> {args.out} in {time.perf_counter() - started:.2f} s ({made} textures made)")
	return 1 if result["errors"] else 0


def cmd_fmt(args) -> int:
	from . import fmt
	rewrite = fmt.terse if args.terse else fmt.readable
	for path in [Path(p) for p in args.files]:
		text = path.read_text()
		out = rewrite(text)
		if args.write:
			if out != text:
				path.write_text(out)
				print(f"rewrote {path}")
		else:
			print(out, end="" if out.endswith("\n") else "\n")
	return 0


def cmd_list(args) -> int:
	project = _project(args)
	for prop in project.props():
		print(f"  {prop['kind']:8s} {prop['id']:40s} {prop['title']}")
	print("std parts: " + ", ".join(sorted(getattr(project.program, "std", ()))))
	print("materials: " + ", ".join(sorted(project.host.materials)))
	return 0


def main(argv: list[str] | None = None) -> int:
	parser = argparse.ArgumentParser(prog="partscript", description="Low-poly props in a few words, compiled to .glb.")
	sub = parser.add_subparsers(dest="command", required=True)
	sub.add_parser("ref", help="the language reference").set_defaults(fn=cmd_ref)
	for name, fn, about in (("check", cmd_check, "parse and check, no building"), ("build", cmd_build, "build every prop to .glb"),
			("list", cmd_list, "props, std parts and materials")):
		command = sub.add_parser(name, help=about)
		command.add_argument("paths", nargs="+", help=".parts files or directories")
		command.add_argument("--cache", help="texture cache directory (default ~/.cache/partscript/textures)")
		command.add_argument("--no-cache", action="store_true", help="make every texture afresh")
		command.set_defaults(fn=fn)
		if name == "check":
			command.add_argument("--json", action="store_true")
		if name == "build":
			command.add_argument("-o", "--out", default="out", help="output directory (default out/)")
			command.add_argument("--only", help="comma-separated prop names")
	formatter = sub.add_parser("fmt", help="rewrite files in words (default) or in shorthand (--terse)")
	formatter.add_argument("files", nargs="+")
	formatter.add_argument("--terse", action="store_true", help="shorthand: b 0,0,~ .4 wood")
	formatter.add_argument("--readable", action="store_true", help="in words: box at=0,0,on size=.4 mat=wood (the default)")
	formatter.add_argument("-w", "--write", action="store_true", help="write the files back (default: print)")
	formatter.set_defaults(fn=cmd_fmt)
	args = parser.parse_args(argv)
	return args.fn(args)


if __name__ == "__main__":
	sys.exit(main())
