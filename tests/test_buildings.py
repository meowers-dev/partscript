"""Buildings: rooms furnished by fill= defs, door positions, cutaways, roofs."""

from __future__ import annotations

import textwrap

import partscript as ps
from partscript import building as pb
from partscript.lang import evaluate

KIT = textwrap.dedent("""
prop wall_a "Wall"
  box at=0,0,on size=3,.2,3 mat=brick
prop door_a "Door wall"
  box at=-1.05,0,on size=.9,.2,3 mat=brick mirror x
prop slab "Floor"
  box at=0,0,-.1 size=3,3,.2 mat=concrete
prop lid "Roof"
  box at=0,0,-.1 size=3,3,.2 mat=concrete
prop rail "Parapet"
  box at=0,0,on size=3,.2,.6 mat=brick
kit kk grid=3 storey=3 wall=.2 parapet_lift=0
  wall solid=wall_a door=door_a
  piece floor=slab roof=lid parapet=rail
def marker colour=#ff0000/paint
  box at=w/2,d/2,on size=w*.5,d*.5,.1+level mat=colour
  box at=door_s,.2,on size=.3 mat=wood when=door_s>=0
""")


def project(building: str) -> ps.Project:
	return ps.Project.from_text(KIT + textwrap.dedent(building), "b.parts")


def plan(p: ps.Project, name: str):
	prop = p.prop(name)
	kit = pb.resolve_kit(p.program, prop.opts["kit"], {})
	return pb.plan(prop, kit, lambda text: evaluate(text, p.program.env_of(prop.file)))


def test_a_room_fill_gets_its_size_level_and_doors() -> None:
	p = project("""
	building house "House" kit=kk
	  room 0,0 2,1 storeys=2 fill=marker colour=#00ff00/paint
	  open 1,0 s door
	""")
	assert p.check()["errors"] == []
	fills = [pl for pl in plan(p, "house").placements if pl.role == "fill"]
	assert [f.fill["env"]["level"] for f in fills] == [0, 1]
	env = fills[0].fill["env"]
	assert (env["w"], env["d"], env["colour"]) == (5.8, 2.8, "#00ff00/paint")
	assert env["door_s"] == 4.4 and env["door_n"] == -1
	assert fills[1].fill["env"]["door_s"] == -1  # the door is on the ground floor only
	built = p.build("house", glb=False)
	assert "x00ff00_paint" in {f.material for f in built.parts[0].faces}


def test_a_cutaway_has_no_front_walls_and_no_roof() -> None:
	text = """
	building house_{v} "House" kit=kk cutaway={v} for v=closed,open
	  room 0,0 2,1
	"""
	p = project(text)
	closed, opened = plan(p, "house_closed"), plan(p, "house_open")
	roles = lambda result: sorted(pl.role for pl in result.placements)  # noqa: E731
	assert roles(closed).count("roof") == 2 and roles(opened).count("roof") == 0
	assert roles(closed).count("wall:solid") == 6 and roles(opened).count("wall:solid") == 4


def test_neighbouring_rooms_share_one_roof_without_a_parapet_between() -> None:
	p = project("""
	building pair "Pair" kit=kk
	  room 0,0 1,1
	  room 1,0 1,1
	""")
	parapets = [pl for pl in plan(p, "pair").placements if pl.role == "parapet"]
	assert len(parapets) == 6  # round the outside of the two cells, none between them


def test_the_example_apartment_builds() -> None:
	from pathlib import Path
	project_ = ps.Project.from_paths([Path(__file__).resolve().parents[1] / "examples"])
	for name in ("cottage", "apartment_closed", "apartment_open"):
		built = project_.build(name, glb=False)
		assert built.triangles > 1000 and built.warnings == [], (name, built.warnings)


def test_furnishing_that_leaves_its_room_is_warned_about() -> None:
	p = project("""
	def tower
	  box at=1,1,on(i*2) size=.2 mat=wood repeat 4 every 0,0,0
	building tall "Tall" kit=kk
	  room 0,0 1,1 fill=tower
	""")
	warnings = p.build("tall", glb=False).warnings
	assert any("reaches outside the room: z 0.00..6.20" in w for w in warnings), warnings


def test_build_warnings_reach_the_result() -> None:
	p = ps.Project.from_text('prop big "Big"\n  box at=0,0,0 size=.1 mat=wood repeat 300 every .2,0,0\n')
	assert any("over the 2500 budget" in w for w in p.build("big", glb=False).warnings)


def test_rooms_nobody_can_reach_are_warned_about() -> None:
	p = project("""
	building flats "Flats" kit=kk
	  room 0,0 1,1 as=front
	  room 1,0 1,1 as=back
	  room 0,0 1,1 storey=1 as=upstairs
	  open 0,0 s door
	""")
	warnings = plan(p, "flats").warnings
	assert sorted(w.split(": ", 1)[1] for w in warnings) == [
		"back (storey 0) cannot be reached from outside (no door or stair leads to it)",
		"upstairs (storey 1) cannot be reached from outside (no door or stair leads to it)"]
	p = project("""
	building flats "Flats" kit=kk
	  room 0,0 1,1 as=front
	  room 1,0 1,1 as=back
	  open 0,0 s door
	  open 0,0 e door
	""")
	assert plan(p, "flats").warnings == []


def test_a_stair_needs_floor_at_its_foot_and_where_it_arrives() -> None:
	stair = "prop steps \"Steps\"\n  box at=0,1.5,on size=1,3,3 mat=concrete\n"
	p = ps.Project.from_text(KIT.replace("  piece floor=slab roof=lid parapet=rail", "  piece floor=slab roof=lid parapet=rail stair=steps") + stair + textwrap.dedent("""
	building tower "Tower" kit=kk
	  room 0,0 1,2 storeys=2 as=hall
	  open 0,0 s door
	  stair 0,0 n
	"""), "b.parts")
	warnings = plan(p, "tower").warnings
	assert any("no floor at its foot (cell 0,-1" in w for w in warnings), warnings
	p2 = ps.Project.from_text(p.sources[0][1].replace("stair 0,0 n", "stair 0,1 n").replace("room 0,0 1,2", "room 0,0 1,4"), "b.parts")
	assert plan(p2, "tower").warnings == []


def test_furniture_in_a_doorway_is_warned_about_but_a_rug_is_not() -> None:
	p = project("""
	def blocker
	  box at=door_s,.3,on size=.4 mat=wood
	def mat_only
	  box at=door_s,.3,.004 size=.8,.5,.008 mat=wood
	building shed_{v} "Shed" kit=kk for v=blocker,mat_only
	  room 0,0 1,1 fill={v}
	  open 0,0 s door
	""")
	assert any("stands in the doorway on its s wall" in w for w in p.build("shed_blocker", glb=False).warnings)
	assert p.build("shed_mat_only", glb=False).warnings == []
