"""Readable placement: named shapes, anchors, on=, row/stack and from=/to=."""

from __future__ import annotations

import textwrap

import pytest

import partscript as ps
from partscript import fmt


def build(body: str, defs: str = ""):
	text = textwrap.dedent(defs) + 'prop demo "Demo"\n' + textwrap.dedent(body)
	project = ps.Project.from_text(text, "t.parts")
	assert project.errors == [] and project.check()["errors"] == [], (project.errors, project.check()["errors"])
	return project.build("demo", glb=False)


def bounds(built, first: int = 0, last: int | None = None):
	faces = [f for part in built.parts for f in part.faces][first:last]
	points = [p for f in faces for p in f.points]
	return [round(min(p[k] for p in points), 4) for k in range(3)], [round(max(p[k] for p in points), 4) for k in range(3)]


def test_on_sits_a_shape_on_a_named_one() -> None:
	built = build("""\
		  desk = box at=1,0,on size=1.2,.6,.75 mat=wood
		  box on=desk at=.2,.1 size=.1,.1,.2 mat=wood
		""")
	lo, hi = bounds(built, 6)
	assert lo == [1.15, 0.05, 0.75] and hi == [1.25, 0.15, 0.95]


def test_anchors_are_numbers_and_points() -> None:
	built = build("""\
		  desk = box at=0,0,on size=1,.5,.8 mat=wood
		  box at=desk.right+.1,desk.y,on(desk) size=.1 mat=wood
		  box at=desk.top_left_front size=.02 mat=wood
		""")
	assert bounds(built, 6, 12) == ([0.55, -0.05, 0.8], [0.65, 0.05, 0.9])
	assert bounds(built, 12) == ([-0.51, -0.26, 0.79], [-0.49, -0.24, 0.81])


def test_anchors_read_in_the_space_of_the_line_that_asks() -> None:
	# Inside a turned group, desk.top is still the desk's top: the lamp lands on it, not beside it.
	built = build("""\
		  desk = box at=2,0,on size=1,1,.8 mat=wood
		  group at=1,0,0 turn=0,0,90 {
		    box at=0,0,on(desk.top) size=.1 mat=wood
		  }
		""")
	lo, hi = bounds(built, 6)
	assert lo[2] == 0.8 and hi[2] == 0.9


def test_a_def_sees_the_named_shapes_of_the_line_that_uses_it() -> None:
	built = build("""\
		  shelf = box at=0,0,on(1) size=1,.3,.04 mat=wood
		  use vase at=-.3,0,0
		""", defs="""\
		def vase
		  cylinder at=0,0,on(shelf) radius=.05 height=.2 mat=wood
		""")
	assert bounds(built, 6)[0][2] == 1.04


def test_row_sets_copies_end_to_end() -> None:
	built = build("""\
		  row x gap=.1 pack=start {
		    box at=0,0,on size=.2,.2,.2 mat=wood repeat 3
		  }
		""")
	assert bounds(built) == ([0.0, -0.1, 0.0], [0.8, 0.1, 0.2])


def test_row_over_spreads_and_align_lines_up() -> None:
	built = build("""\
		  row x over=2 align=back {
		    box at=0,0,on size=.2,.1,.3 mat=wood
		    box at=0,0,on size=.2,.4,.3 mat=wood
		    box at=0,0,on size=.2,.2,.3 mat=wood
		  }
		""")
	assert bounds(built) == ([-1.0, -0.4, 0.0], [1.0, 0.0, 0.3])
	assert bounds(built, 6, 12)[0][0] == pytest.approx(-0.1)  # the middle one, in the middle


def test_stack_piles_up_from_the_floor() -> None:
	built = build("""\
		  stack {
		    box size=.5,.4,.3 mat=wood repeat 3
		    cylinder radius=.1 height=.2 mat=wood
		  }
		""")
	assert bounds(built)[1][2] == pytest.approx(1.1)


def test_named_row_and_things_on_it() -> None:
	built = build("""\
		  books = row x gap=0 {
		    box size=.05,.2,.25 mat=wood repeat 4
		  }
		  box at=books.right+.05,0,on size=.1 mat=wood
		""")
	assert bounds(built, 24)[0][0] == pytest.approx(0.1)  # its middle .05 past the last book


def test_beam_between_two_points() -> None:
	built = build("  box from=0,0,0 to=3,4,0 size=.1 mat=wood\n  cylinder from=0,0,0 to=0,0,2 radius=.05 mat=wood\n")
	lo, hi = bounds(built, 0, 6)
	assert hi[0] - lo[0] > 2.9 and hi[1] - lo[1] > 3.9 and hi[2] - lo[2] == pytest.approx(.1)
	assert bounds(built, 6)[1][2] == pytest.approx(2.0)


def test_use_from_to_lays_a_def_along_and_gives_it_length() -> None:
	built = build("  use plank from=1,1,0 to=1,3,0\n", defs="""\
		def plank
		  box at=length/2,0,0 size=length,.1,.02 mat=wood
		""")
	lo, hi = bounds(built)
	assert lo[1] == pytest.approx(1.0) and hi[1] == pytest.approx(3.0) and hi[0] - lo[0] == pytest.approx(.1)


def test_mistakes_say_what_to_do() -> None:
	for body, message in (("  box on=desk size=.1 mat=wood\n", "no shape of that name"),
			("  desk = box at=0 size=1 mat=wood\n  box at=desk.middle,0,0 size=.1 mat=wood\n", "a named shape gives"),
			("  row q {\n    box at=0 size=.1 mat=wood\n  }\n", "the axis it runs along")):
		project = ps.Project.from_text('prop demo "Demo"\n' + body, "t.parts")
		assert any(message in e for e in project.check()["errors"]), project.check()["errors"]


@pytest.mark.parametrize("line", [
	"desk = box at=1,0,on size=1.4,.7,.75 mat=wood",
	"box size=.4 mat=wood",
	"box on=desk at=.2,.1 size=.1 mat=wood",
	"cylinder from=desk.top to=1,1,2 radius=.02 mat=steel_dark",
	"box from=0,0,0 to=1,0,0 size=.03 mat=brass",
	"use lamp on=desk at=.3,.1",
	"stack gap=0 {",
	"books = row x on=desk at=-.3,.2 gap=.004 align=back {",
])
def test_fmt_keeps_the_readable_form(line: str) -> None:
	assert fmt.readable("  " + line).strip() == line
	again = fmt.readable(fmt.terse("  " + line)).strip()
	assert again == line
