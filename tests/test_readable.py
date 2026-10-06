"""The readable form parses to the same statements as the terse one."""

from __future__ import annotations

import pytest

import partscript as ps
from partscript.lang import PartScriptError, _parse_statement


def same(terse: str, readable: str) -> None:
	a, b = _parse_statement(terse, "t", 1), _parse_statement(readable, "t", 1)
	assert (a.op, a.args, a.opts, a.mods) == (b.op, b.args, b.opts, b.mods), (a, b)


@pytest.mark.parametrize("terse, readable", [
	("b 0,0,~ .4,.3,.2 wood", "box at=0,0,on size=.4,.3,.2 mat=wood"),
	("b 0,0,~.8 .4 wood r=0,0,45", "box at=0,0,on(.8) size=.4 material=wood turn=0,0,45"),
	("bb 0,0,~ .4,.4,.05 .015 rubber", "bevel_box at=0,0,on size=.4,.4,.05 bevel=.015 mat=rubber"),
	("c 0,0,~ .16 .64 #e25a1a/plastic rt=.03 s=12", "cylinder at=0,0,on radius=.16 height=.64 mat=#e25a1a/plastic top_radius=.03 sides=12"),
	("sph 0,0,1 .2 brass s=6 rings=3", "sphere at=0,0,1 radius=.2 mat=brass sides=6 rings=3"),
	('sign 0,-.05,2 1.6 .3 "THE HOPE" sub="FREE HOUSE"', 'sign at=0,-.05,2 width=1.6 height=.3 text="THE HOPE" sub="FREE HOUSE"'),
	("ext 0,0,0 1.6 paint -1:0 1:0 0:1 ax=y", "extrude at=0,0,0 width=1.6 mat=paint -1:0 1:0 0:1 axis=y"),
	("ext 0,0,0 1.6 paint -1:0 1:0 0:1 ax=y", 'extrude at=0,0,0 width=1.6 mat=paint points="-1:0 1:0 0:1" axis=y'),
	("pipe steel_dark .03 -1,0,1 1,0,1", "pipe mat=steel_dark radius=.03 -1,0,1 1,0,1"),
	("use table 0,0,0 w=1 top=wood", "use table at=0,0,0 w=1 top=wood"),
	("use flower 0,0,.12 *40~1.6,.75,.09 s=rand(.8,1.2)", "use flower at=0,0,.12 scatter 40 over 1.6,.75 apart .09 scale=rand(.8,1.2)"),
	("use rock *5~1.2", "use rock scatter 5 within 1.2"),
	("use rock *5~1.2,0,.3", "use rock scatter 5 within 1.2 apart .3"),
	("b 0,0,0 .1 wood *5@.3,0,0 mx my", "box at=0,0,0 size=.1 mat=wood repeat 5 every .3,0,0 mirror xy"),
	("b 0,0,0 .1 wood *(n)@.3,0,0", "box at=0,0,0 size=.1 mat=wood repeat n every .3,0,0"),
	("b 0,0,0 .1 wood *4x3@.2,.2", "box at=0,0,0 size=.1 mat=wood grid 4x3 every .2,.2"),
	("b .14,0,3.3 .025 wood *6%(360/6)", "box at=.14,0,3.3 size=.025 mat=wood ring 6"),
	("b .14,0,3.3 .025 wood *6%60", "box at=.14,0,3.3 size=.025 mat=wood ring 6 step 60"),
	("at 0,0,1 r=0,0,45", "group at=0,0,1 turn=0,0,45"),
])
def test_both_forms_parse_the_same(terse: str, readable: str) -> None:
	same(terse, readable)


@pytest.mark.parametrize("text, message", [
	("box 0,0,0 .4 wood at=0,0,0", "also given by position"),
	("box at=0,0,0 mat=wood", "size= is missing"),
	("box at=0 size=.1 mat=wood turn=0,0,1 r=0,0,2", "same option"),
	("box at=0 size=.1 mat=wood sides=6", "sides= is for round shapes"),
	("box at=0 size=.1 mat=wood mirror q", "mirror x"),
])
def test_readable_mistakes_say_what_to_do(text: str, message: str) -> None:
	with pytest.raises(PartScriptError, match=message):
		_parse_statement(text, "t", 1)


def test_a_readable_prop_builds_like_the_terse_one() -> None:
	terse = ps.Project.from_text('prop cone "Cone"\n  c 0,0,~ .16 .64 #e25a1a/plastic rt=.03 s=12\n  bb 0,0,~ .42,.42,.05 .015 rubber\n')
	readable = ps.Project.from_text('prop cone "Cone"\n  cylinder at=0,0,on radius=.16 height=.64 mat=#e25a1a/plastic top_radius=.03 sides=12\n'
		'  bevel_box at=0,0,on size=.42,.42,.05 bevel=.015 mat=rubber\n')
	assert terse.build("cone").glb == readable.build("cone").glb


def _statements(program) -> list:
	def walk(body):
		for s in body:
			yield (s.op, s.args, s.opts, s.mods)
			if s.block:
				yield from walk(s.block)
	out = []
	for prop in program.props:
		out.append((prop.name, list(walk(prop.body))))
	for name, macro in sorted(program.macros.items()):
		out.append((name, list(walk(macro.body))))
	return out


def test_fmt_round_trips_every_example() -> None:
	from pathlib import Path

	from partscript import fmt
	from partscript.project import STD
	root = Path(__file__).resolve().parents[1]
	for path in [*sorted((root / "examples").glob("*.parts")), STD, root / "tests" / "golden" / "shapes.parts"]:
		text = path.read_text()
		original = _statements(ps.parse(text, path.name))
		long = fmt.readable(text)
		assert ps.parse(long, path.name).errors == [], (path.name, ps.parse(long, path.name).errors)
		assert _statements(ps.parse(long, path.name)) == original, path.name
		assert _statements(ps.parse(fmt.terse(long), path.name)) == original, path.name
		assert fmt.readable(long) == long  # stable
