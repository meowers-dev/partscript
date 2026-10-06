"""Links: loose ends a join line bridges when they nearly meet."""

from __future__ import annotations

import textwrap

import partscript as ps

RAIL = textwrap.dedent("""
def run len=1
  pipe mat=steel_dark radius=.02 0,0,.9 len,0,.9 sides=4
  link rail at=0,0,.9 toward=-x
  link rail at=len,0,.9 toward=+x
def swag
  box at=length/2,0,0 size=length,.02,.02 mat=steel_dark
""")


def build(body: str):
	p = ps.Project.from_text(RAIL + 'prop demo "Demo"\n' + textwrap.dedent(body), "t.parts")
	assert p.check()["errors"] == []
	return p.build("demo", glb=False)


def ends(built) -> list[tuple]:
	return sorted(tuple(round(v, 3) for v in end.pos) for end in built.links)


def test_gap_is_bridged_and_outer_ends_stay_open() -> None:
	built = build("  use run at=0,0,0\n  use run at=1.3,0,0\n  join rail\n")
	assert ends(built) == [(0.0, 0.0, 0.9), (2.3, 0.0, 0.9)]
	assert len(built.joints) == 2


def test_no_join_line_leaves_every_end_open() -> None:
	built = build("  use run at=0,0,0\n  use run at=1.3,0,0\n")
	assert len(built.links) == 4 and built.joints == []


def test_reach_limits_the_gap() -> None:
	built = build("  use run at=0,0,0\n  use run at=1.5,0,0\n  join rail reach=.4\n")
	assert len(built.links) == 4


def test_ends_that_face_away_do_not_join() -> None:
	# The runs overlap, turned opposite ways: every pair of nearby ends has one pointing away from the other.
	built = build("  use run at=0,0,0\n  use run at=.8,.1,0 turn=0,0,180\n  join rail\n")
	assert len(built.links) == 4


def test_side_by_side_ends_make_a_return() -> None:
	# Two runs a hand's width apart, the second turned back: at each end both point the same way and the
	# bridge loops round (a stair rail's return), so the pair closes into one loop with no end left.
	built = build("  use run at=0,0,0\n  use run at=1,.2,0 turn=0,0,180\n  join rail\n")
	assert built.links == [] and len(built.joints) == 4


def test_corner_is_bridged() -> None:
	built = build("  use run at=0,0,0\n  use run at=1.2,.2,0 turn=0,0,90\n  join rail\n")
	assert ends(built) == [(0.0, 0.0, 0.9), (1.2, 1.2, 0.9)]


def test_touching_ends_count_as_joined_without_a_bridge() -> None:
	plain = build("  use run at=0,0,0\n  use run at=1,0,0\n")
	joined = build("  use run at=0,0,0\n  use run at=1,0,0\n  join rail\n")
	assert len(joined.links) == 2
	assert joined.triangles == plain.triangles


def test_each_end_joins_once_nearest_first() -> None:
	built = build("  use run at=0,0,0\n  use run at=1.2,0,0\n  use run at=1.25,.1,0\n  join rail\n")
	# The nearer run takes the bridge; the farther one's start is left open.
	assert (1.25, 0.1, 0.9) in ends(built)
	assert len(built.links) == 4


def test_join_with_def_draws_the_bridge() -> None:
	plain = build("  use run at=0,0,0\n  use run at=1.5,0,0\n  join rail\n")
	swag = build("  use run at=0,0,0\n  use run at=1.5,0,0\n  join rail with=swag\n")
	assert len(swag.links) == 2
	assert swag.triangles != plain.triangles


def test_join_inside_a_def_resolves_there_and_leaves_its_open_ends() -> None:
	p = ps.Project.from_text(RAIL + textwrap.dedent("""
		def pair
		  use run at=0,0,0
		  use run at=1.1,0,0
		  join rail
		prop demo "Demo"
		  use pair at=0,0,0
		  use pair at=2.4,0,0
		  join rail
		"""), "t.parts")
	built = p.build("demo", glb=False)
	assert ends(built) == [(0.0, 0.0, 0.9), (4.5, 0.0, 0.9)]
	assert len(built.joints) == 6


def test_join_checks_its_options() -> None:
	p = ps.Project.from_text(RAIL + 'prop demo "Demo"\n  use run\n  join rail with=nope\n', "t.parts")
	assert p.check()["errors"]
	p = ps.Project.from_text(RAIL + 'prop demo "Demo"\n  link rail at=0,0,0 toward=sideways\n', "t.parts")
	assert p.check()["errors"]


def test_repeat_count_with_an_x_in_brackets() -> None:
	built = build("  box at=0,0,on size=.1,.1,.1 mat=steel_dark repeat (max(1,3)) every .2,0,0\n")
	assert built.triangles == 3 * 12
