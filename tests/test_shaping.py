"""Shapes and tools for detail: torus, frame, smooth curves, twist/bend/shrink, named copy numbers, if, \\."""

from __future__ import annotations

import math
import textwrap

import pytest

import partscript as ps
from kitlib.paths import smooth_points


def build(text: str, name: str):
	p = ps.Project.from_text(textwrap.dedent(text), "s.parts")
	assert p.errors == [] and p.check()["errors"] == [], (p.errors, p.check()["errors"])
	return p.build(name, glb=False)


def bounds(built) -> tuple:
	pts = [q for part in built.parts for f in part.faces for q in f.points]
	return tuple(round(min(q[k] for q in pts), 3) for k in range(3)), tuple(round(max(q[k] for q in pts), 3) for k in range(3))


def test_torus_lies_flat_or_stands_on_an_axis() -> None:
	flat = build('prop rr "Ring"\n  torus at=0,0,1 radius=.5 thick=.05 mat=brass sides=24\n', "rr")
	low, high = bounds(flat)
	assert high[2] - low[2] == pytest.approx(2 * .05 * math.sin(math.radians(60)), abs=.005)  # six sides round its tube
	assert high[0] == pytest.approx(.55, abs=.01)
	upright = build('prop rr "Ring"\n  torus at=0,0,on radius=.5 thick=.05 mat=rubber axis=x sides=24\n', "rr")
	low, high = bounds(upright)
	assert low[2] == pytest.approx(0, abs=.01) and high[2] == pytest.approx(1.1, abs=.01) and high[0] == pytest.approx(.05, abs=.01)


def test_frame_leaves_its_hole_open() -> None:
	built = build('prop ff "Frame"\n  frame at=0,0,1 width=1 height=.8 depth=.04 mat=wood hole=.6,.4\n', "ff")
	assert built.triangles == 2 * 12 + 2 * 8  # side bars whole; the bars above and below the hole have no hidden ends
	inside = [q for f in built.parts[0].faces for q in f.points if abs(q[0]) < .29 and abs(q[2] - 1) < .19]
	assert inside == []


def test_smooth_passes_through_every_point() -> None:
	points = [(0, 0, 0), (1, 0, 1), (2, 0, 0), (3, 0, 1)]
	curve = smooth_points(points, 4)
	assert len(curve) == 3 * 4 + 1 and all(p in curve for p in [tuple(map(float, q)) for q in points])
	built = build('prop vv "Vine"\n  pipe mat=wood radius=.02 0,0,0 .5,0,.5 1,0,0 smooth=6 sides=4\n', "vv")
	assert built.triangles > build('prop vv "Vine"\n  pipe mat=wood radius=.02 0,0,0 .5,0,.5 1,0,0 sides=4\n', "vv").triangles * 3


def test_twist_shrink_and_bend() -> None:
	plain = bounds(build('prop cc "C"\n  box at=0,0,on size=.4,.4,2 mat=stone\n', "cc"))
	twisted = bounds(build('prop cc "C"\n  box at=0,0,on size=.4,.4,2 mat=stone twist=45\n', "cc"))
	assert twisted[1][0] == pytest.approx(.2 * math.sqrt(2), abs=.01) and plain[1][0] == pytest.approx(.2)
	shrunk = bounds(build('prop cc "C"\n  box at=0,0,on size=.4,.4,2 mat=stone shrink=.5\n', "cc"))
	assert shrunk[1][0] == pytest.approx(.2)  # the base is unchanged
	bent = bounds(build('prop cc "C"\n  cylinder at=0,0,on radius=.05 height=2 mat=steel_dark sides=6 bend=90\n', "cc"))
	# A 2 m post bent a quarter turn round a radius R = 2/(pi/2): it ends R over, its outside edge R + .05 up.
	radius = 2 / (math.pi / 2)
	assert bent[1][1] == pytest.approx(radius, abs=.02) and bent[1][2] == pytest.approx(radius + .05, abs=.02)
	sideways = bounds(build('prop cc "C"\n  cylinder at=0,0,on radius=.05 height=2 mat=steel_dark sides=6 bend=90,90\n', "cc"))
	assert sideways[0][0] == pytest.approx(-radius, abs=.02)  # heading 90: it bends toward -X


def test_a_grid_names_its_columns_and_rows() -> None:
	built = build('prop gg "G"\n  box at=0,0,0 size=.1 mat=wood grid 3x3 every .5,.5 as col,row when=col==row\n', "gg")
	centres = sorted({(round(sum(q[0] for f in built.parts[0].faces[k:k + 6] for q in f.points) / 24, 2),
		round(sum(q[1] for f in built.parts[0].faces[k:k + 6] for q in f.points) / 24, 2)) for k in range(0, len(built.parts[0].faces), 6)})
	assert centres == [(0, 0), (.5, .5), (1, 1)]


def test_nested_copies_keep_their_own_names() -> None:
	built = build('''
	prop shelves "Shelves"
	  group at=0,0,0 repeat 3 every 0,0,.5 as shelf {
	    box at=0,0,shelf*.5 size=.1,.1,.05+.05*item mat=wood repeat 2 every .3,0,0 as item
	  }
	''', "shelves")
	heights = sorted({round(max(q[2] for f in built.parts[0].faces[k:k + 6] for q in f.points) - min(q[2] for f in built.parts[0].faces[k:k + 6] for q in f.points), 3)
		for k in range(0, len(built.parts[0].faces), 6)})
	assert heights == [.05, .1]


def test_if_blocks_and_continued_lines() -> None:
	built = build('''
	prop tidy "Tidy"
	  set big=1
	  if big == 1 and 2 > 1 {
	    box at=0,0,0 \\
	        size=.5 \\
	        mat=wood
	  }
	  if big == 0 {
	    box at=2,0,0 size=.5 mat=wood
	  }
	  box at=0,0,1 size=.1 mat=brass
	''', "tidy")
	assert built.triangles == 24
	p = ps.Project.from_text('prop tidy "Tidy"\n  box at=0,0,0 \\\n    size=.5 \\\n    mat=nope\n  box at=0,0,0 size=.1 mat=nope2\n', "t.parts")
	errors = p.check()["errors"]
	assert any(e.startswith("t.parts:2:") and "nope" in e for e in errors) and any(e.startswith("t.parts:5:") for e in errors), errors


def test_an_option_a_shape_does_not_take_is_an_error() -> None:
	p = ps.Project.from_text('prop aa "A"\n  torus at=0,0,0 radius=.3 radius_out=0 thick=.02 mat=rubber\n')
	errors = p.check()["errors"]
	assert len(errors) == 1 and "torus: no option radius_out=" in errors[0] and "sides" in errors[0], errors
