"""Snapping and paths: chain, along=, joints=, snap markers."""

from __future__ import annotations

import math
import textwrap

import pytest

import partscript as ps
from kitlib.paths import path_frames

PIECES = textwrap.dedent("""
prop straight "Straight"
  box at=0,0,on size=1.2,2,.06 mat=concrete
  snap start 0,-1,0 -y kind=path
  snap end 0,1,0 +y kind=path
prop corner "Corner"
  box at=0,0,on size=1.2,1.2,.06 mat=concrete
  snap start 0,-.6,0 -y kind=path
  snap end .6,0,0 +x kind=path
prop rise "Rise"
  box at=0,0,on size=1.2,2,.3 mat=concrete
  snap start 0,-1,0 -y kind=path
  snap end 0,1,.3 +y kind=path
prop pipe "Pipe"
  box at=0,0,on size=.2,1,.2 mat=steel_grey
  snap start 0,-.5,0 -y kind=pipe
  snap end 0,.5,0 +y kind=pipe
""")


def project(extra: str) -> ps.Project:
	return ps.Project.from_text(PIECES + textwrap.dedent(extra), "p.parts")


def test_chain_joins_pieces_snap_to_snap() -> None:
	p = project('prop walk "Walk"\n  chain straight*2 corner straight rise\n')
	assert p.check()["errors"] == []
	built = p.build("walk", glb=False)
	joints = [tuple(round(v, 3) for v in j.pos) for j in built.joints]
	assert joints == [(0, 0, 0), (0, 2, 0), (0, 4, 0), (.6, 4.6, 0), (2.6, 4.6, 0), (4.6, 4.6, .3)]
	assert built.triangles == 5 * 12 - 6  # the corner's start face sits on the straight's end face and is deduped


def test_chain_refuses_pieces_whose_kinds_do_not_fit() -> None:
	p = project('prop bad "Bad"\n  chain straight pipe\n')
	with pytest.raises(ps.PartScriptError, match="kind pipe"):
		p.build("bad")


def test_along_fits_panels_and_puts_posts_at_the_joins() -> None:
	frames = path_frames([(0, 0), (6, 0), (6, 4), (0, 4)], 2, fit=True, closed=True)
	assert len(frames) == 10 and {round(f.stretch, 6) for f in frames} == {1.0}
	odd = path_frames([(0, 0), (5, 0)], 2, fit=True)
	assert len(odd) == 2 and odd[0].stretch == pytest.approx(1.25)
	posts = path_frames([(0, 0), (6, 0), (6, 4), (0, 4)], 2, fit=True, closed=True, joints=True)
	assert [tuple(round(v, 3) for v in f.pos[:2]) for f in posts][:4] == [(0, 0), (2, 0), (4, 0), (6, 0)] and len(posts) == 10
	corners = path_frames([(0, 0), (4, 0), (4, 4)], corners=True)
	assert [round(f.yaw) for f in corners] == [0, 45, 90]


def test_along_in_a_prop_follows_a_named_line_with_a_gap() -> None:
	p = project("""
	prop yard "Yard"
	  set edge="0,0 6,0 6,4"
	  box at=0,0,on size=2,.04,.8 mat=wood along=edge every=2 fit=1 when=i!=1
	""")
	assert p.check()["errors"] == []
	built = p.build("yard", glb=False)
	centres = sorted({(round(sum(q[0] for f in built.parts[0].faces[k:k + 6] for q in f.points) / 24, 2),
		round(sum(q[1] for f in built.parts[0].faces[k:k + 6] for q in f.points) / 24, 2)) for k in range(0, len(built.parts[0].faces), 6)})
	assert centres == [(1.0, 0.0), (5.0, 0.0), (6.0, 1.0), (6.0, 3.0)]


def test_snap_markers_are_a_separate_part_not_counted() -> None:
	p = project('prop walk "Walk"\n  chain straight corner\n')
	plain, marked = p.build("walk"), p.build("walk", snaps=True)
	assert marked.triangles == plain.triangles
	assert [b["name"] for b in marked.baked] == ["walk", "snaps"]
	assert math.isclose(len(marked.joints), 3)
