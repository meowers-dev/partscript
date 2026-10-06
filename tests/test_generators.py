"""Generators: noise, terrain, drop, scatter on, break and rubble, taper, here."""

from __future__ import annotations

import textwrap

import pytest

import partscript as ps
from kitlib.noise import noise, rough
from kitlib.surface import SurfaceIndex


def build(body: str, defs: str = "", seed: str = ""):
	text = textwrap.dedent(defs) + f'prop demo "Demo" budget=20000{" seed=" + seed if seed else ""}\n' + textwrap.dedent(body)
	project = ps.Project.from_text(text, "t.parts")
	report = project.check()
	assert project.errors == [] and report["errors"] == [], (project.errors, report["errors"])
	return project.build("demo", glb=False)


def faces(built) -> list:
	return [f for part in built.parts for f in part.faces]


def test_noise_is_smooth_seeded_and_in_range() -> None:
	values = [noise(x * .1, 0, 0, "a") for x in range(200)]
	assert all(0 <= v <= 1 for v in values)
	assert max(abs(a - b) for a, b in zip(values, values[1:])) < .1  # smooth
	assert noise(1.3, 2.1, 0, "a") == noise(1.3, 2.1, 0, "a")
	assert noise(1.3, 2.1, 0, "a") != noise(1.3, 2.1, 0, "b")
	assert 0 <= rough(3.3, 1.2) <= 1


def test_noise_in_expressions_follows_the_prop_seed() -> None:
	one = build("  box size=.2,.2,.2+noise(i*.37,1) mat=wood repeat 6 every .3,0,0\n")
	again = build("  box size=.2,.2,.2+noise(i*.37,1) mat=wood repeat 6 every .3,0,0\n")
	other = build("  box size=.2,.2,.2+noise(i*.37,1) mat=wood repeat 6 every .3,0,0\n", seed="7")
	tops = lambda b: [round(max(q[2] for f in faces(b)[k * 6:k * 6 + 6] for q in f.points), 5) for k in range(6)]  # noqa: E731
	assert tops(one) == tops(again) != tops(other)


def test_here_is_where_the_copy_stands() -> None:
	built = build("  box size=.1,.1,.1+here.x mat=wood repeat 3 every 1,0,0\n")
	tops = sorted(round(max(q[2] for f in faces(built)[k * 6:k * 6 + 6] for q in f.points), 4) for k in range(3))
	assert tops == [0.1, 1.1, 2.1]


def test_terrain_follows_its_height_and_marks_steep_faces() -> None:
	built = build("  terrain size=4,4 cells=8 height=max(0,x)*2 mat=#4a6a2a/fabric steep=stone slope=40\n")
	tops = [f for f in faces(built) if len(f.points) == 3]
	assert len(tops) == 8 * 8 * 2
	materials = {f.material for f in tops}
	assert len(materials) == 2  # flat ground and the steep slope
	assert max(q[2] for f in tops for q in f.points) == pytest.approx(4.0)


def test_drop_lands_things_on_what_is_below() -> None:
	built = build("""\
		  terrain size=4,4 cells=4 height=1.5 mat=#4a6a2a/fabric
		  box at=0,0,5 size=.2,.2,.2 mat=wood drop=1
		  box at=0,0,0 size=.2,.2,.2 mat=wood drop=1
		""")
	boxes = faces(built)[-12:]
	assert min(q[2] for f in boxes[:6] for q in f.points) == pytest.approx(1.5)  # fell from above
	assert min(q[2] for f in boxes[6:] for q in f.points) == pytest.approx(1.5)  # rose out of the hill


def test_dropped_things_pile_up() -> None:
	built = build("  box at=0,0,3 size=.3,.3,.3 mat=wood drop=1 repeat 3 every 0,0,1\n")
	bottoms = sorted(round(min(q[2] for f in faces(built)[k * 6:k * 6 + 6] for q in f.points), 4) for k in range(3))
	assert bottoms == [0.0, 0.3, 0.6]


def test_drop_lean_tilts_with_the_ground() -> None:
	built = build("""\
		  face #4a6a2a/fabric -2,-2,0 2,-2,2 2,2,2 -2,2,0
		  box at=0,0,4 size=.2,.2,.6 mat=wood drop=lean
		""")
	box = faces(built)[1:]
	top = [q for f in box for q in f.points if q[2] > 1.3]
	assert top and min(q[0] for q in top) < -0.1  # leaning back with the slope, not standing straight


def test_scatter_on_grows_on_the_faces_that_face_the_way_asked() -> None:
	built = build("""\
		  block = box size=2,2,1 mat=stone
		  box size=.04 mat=wood scatter 30 on block facing up
		""")
	moss = faces(built)[6:]
	assert len(moss) == 30 * 6
	assert all(abs(min(q[2] for q in f.points) - 1.0) < .05 for f in moss[::6])


def test_scatter_on_side_faces_stands_copies_out_of_them() -> None:
	built = build("""\
		  wall = box size=2,.2,2 mat=stone
		  box size=.05,.05,.3 mat=wood scatter 10 on wall facing side
		""")
	ivy = faces(built)[6:]
	for k in range(10):
		points = [q for f in ivy[k * 6:k * 6 + 6] for q in f.points]
		depth = max(max(q[a] for q in points) - min(q[a] for q in points) for a in (0, 1))
		assert depth == pytest.approx(.3, abs=.01)  # its +Z points out of the wall (its sides or its ends)


def test_break_knocks_chunks_out_and_rubble_falls() -> None:
	whole = build("  box size=3,.3,2.4 mat=stone\n")
	broken = build("  box size=3,.3,2.4 mat=stone break=.4 chunk=.3 core=brick\n")
	rubble = build("  box size=3,.3,2.4 mat=stone break=.4 chunk=.3 core=brick rubble=.5\n")
	top = lambda b: max(q[2] for f in faces(b) for q in f.points)  # noqa: E731
	assert len(faces(broken)) > len(faces(whole))
	assert top(broken) <= 2.4 + 1e-6
	assert len(faces(rubble)) > len(faces(broken))
	again = build("  box size=3,.3,2.4 mat=stone break=.4 chunk=.3 core=brick rubble=.5\n")
	assert [tuple(f.points[0]) for f in faces(again)] == [tuple(f.points[0]) for f in faces(rubble)]


def test_taper_narrows_a_pipe_toward_its_end() -> None:
	built = build("  pipe mat=wood radius=.2 0,0,0 0,0,2 taper=.25 sides=6\n")
	points = [q for f in faces(built) for q in f.points]
	at = lambda z: max((q[0] ** 2 + q[1] ** 2) ** .5 for q in points if abs(q[2] - z) < 1e-6)  # noqa: E731
	assert at(0) == pytest.approx(.2) and at(2) == pytest.approx(.05)


def test_a_def_parameter_wins_over_a_placing_option() -> None:
	built = build("  use vine at=0,0,2 drop=.5\n", defs="""\
		def vine drop=.8
		  pipe mat=wood radius=.01 0,0,0 0,0,-drop sides=3
		""")
	assert min(q[2] for f in faces(built) for q in f.points) == pytest.approx(1.5)


def test_surface_index_finds_the_highest_surface_under_a_point() -> None:
	ground = SurfaceIndex([[(-1, -1, 0), (1, -1, 0), (1, 1, 0), (-1, 1, 0)], [(-1, -1, 1), (1, -1, 1), (1, 1, 1), (-1, 1, 1)]])
	assert ground.below(0, 0, 5)[0] == 1 and ground.below(0, 0, .5)[0] == 0 and ground.below(3, 3, 5) is None


def test_mistakes_say_what_to_do() -> None:
	for body, message in (("  box size=.1 mat=wood scatter 5 on nothing\n", "no shape of that name"),
			("  box size=.1 mat=wood drop=2\n", "drop=2"),
			("  box size=1 mat=wood break=1.5\n", "break=1.5"),
			("  box size=1 mat=wood core=brick\n", "go with break="),
			("  terrain size=4,4 cells=200 mat=wood\n", "cells=200")):
		project = ps.Project.from_text('prop demo "Demo"\n' + body, "t.parts")
		assert any(message in e for e in project.check()["errors"]), (body, project.check()["errors"])
