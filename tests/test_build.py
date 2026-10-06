from __future__ import annotations

import json
import struct
import textwrap

import pytest

import partscript as ps
from partscript.host import Host

PROPS = textwrap.dedent("""
prop stall "Market Stall" street
  use stall
prop cone_{c} "Traffic Cone" street for c=e25a1a,d8c020
  c 0,0,~ .18 .7 #{c}/plastic rt=.03 s=8
  b 0,0,.02 .4,.4,.04 rubber
prop pub_sign "Pub Sign"
  sign 0,-.03,2 1.6 .3 "THE HOPE & ANCHOR" sub="FREE HOUSE"
  b 0,0,2 1.7,.04,.4 wood
prop lamp_post "Lamp Post"
  use post h=3
  use lamp 0,-.3,2.9
  b 0,-.15,2.95 .04,.3,.04 steel_dark
  part glass smooth=1
  sph 0,0,3.3 .1 glass_opaque s=8 rings=4
""")


def project(**kwargs) -> ps.Project:
	return ps.Project.from_text(PROPS, "test.parts", Host(**kwargs))


def read_glb(data: bytes) -> tuple[dict, bytes]:
	magic, version, length = struct.unpack_from("<4sII", data, 0)
	assert (magic, version, length) == (b"glTF", 2, len(data))
	json_len, json_kind = struct.unpack_from("<I4s", data, 12)
	assert json_kind == b"JSON"
	document = json.loads(data[20:20 + json_len])
	bin_len, bin_kind = struct.unpack_from("<I4s", data, 20 + json_len)
	assert bin_kind == b"BIN\x00"
	return document, data[28 + json_len:28 + json_len + bin_len]


def test_every_prop_builds_to_a_valid_glb() -> None:
	p = project()
	for prop in p.props():
		built = p.build(prop["id"])
		document, blob = read_glb(built.glb)
		assert document["asset"]["version"] == "2.0"
		assert built.triangles > 0
		indices = sum(document["accessors"][prim["indices"]]["count"] for mesh in document["meshes"] for prim in mesh["primitives"])
		assert indices == built.triangles * 3
		assert len(blob) == document["buffers"][0]["byteLength"]
		assert built.warnings == []
		for image in document["images"]:
			view = document["bufferViews"][image["bufferView"]]
			assert blob[view["byteOffset"]:view["byteOffset"] + 8] == b"\x89PNG\r\n\x1a\n"


def test_builds_are_deterministic() -> None:
	assert project().build("stall").glb == project().build("stall").glb


def test_parts_become_nodes_under_the_asset() -> None:
	document, _ = read_glb(project().build("lamp_post").glb)
	root = document["nodes"][document["scenes"][0]["nodes"][0]]
	assert root["name"] == "lamp_post" and len(root["children"]) == 2


def test_steps_tag_faces_with_their_statement() -> None:
	built = project().build("lamp_post", steps=True)
	assert [step["stmt"].op for step in built.steps] == ["use", "use", "b", "part", "sph"]
	document, _ = read_glb(built.glb)
	assert all("TEXCOORD_1" in prim["attributes"] for mesh in document["meshes"] for prim in mesh["primitives"])


def test_textures_are_cached_on_disk(tmp_path) -> None:
	first = project(cache_dir=tmp_path)
	first.write_all(tmp_path / "out")
	made = first.host.textures.made
	assert made > 0
	second = project(cache_dir=tmp_path)
	second.write_all(tmp_path / "out2")
	assert second.host.textures.made == 0
	assert (tmp_path / "out" / "stall.glb").read_bytes() == (tmp_path / "out2" / "stall.glb").read_bytes()


def test_a_prop_with_errors_does_not_build() -> None:
	p = ps.Project.from_text("prop bad\n  b 0 1 nowhere_material\n")
	with pytest.raises(ps.PartScriptError):
		p.build("bad")


def test_buildings_export_placements() -> None:
	text = textwrap.dedent("""
	prop wall_solid "Wall"
	  b 0,-.15,~ 4,.3,4 #a08060/plaster
	prop wall_door "Door wall"
	  b -1.5,-.15,~ 1,.3,4 #a08060/plaster mx
	prop slab "Floor"
	  b 0,0,-.1 4,4,.2 #707070/concrete
	kit cottage
	  wall solid=wall_solid door=wall_door
	  piece floor=slab roof=slab
	building hut "Hut" kit=cottage
	  room 0,0 2,1
	  open 0,0 s door
	""")
	p = ps.Project.from_text(text)
	assert p.check()["errors"] == []
	data = p.building("hut")
	assert data["set"] == "cottage"
	assert {row["asset"] for row in data["placements"]} == {"wall_solid", "wall_door", "slab"}
	assert [o["kind"] for o in data["openings"]] == ["door"]


def test_props_use_other_props_and_defs() -> None:
	text = textwrap.dedent("""
	def leg h=1
	  b 0,0,~ .1,.1,h wood
	prop stool_a "Stool"
	  use leg .2,0,0 mx
	  b 0,0,1 .5,.3,.05 wood
	prop two_stools "Two"
	  use stool_a -1,0,0
	  use stool_a 1,0,0
	""")
	p = ps.Project.from_text(text)
	report = p.check()
	assert report["errors"] == []
	assert [x["triangles"] for x in report["props"]] == [36, 72]
	assert p.build("two_stools").triangles == 2 * p.build("stool_a").triangles
	tree = p.uses("two_stools")
	assert [(c["name"], c["kind"]) for c in tree["children"]] == [("stool_a", "prop"), ("stool_a", "prop")]
	assert tree["children"][0]["children"][0] == {"name": "leg", "kind": "def", "file": "<text>", "line": 2, "count": 2, "at": 5, "children": []}


def test_faces_know_the_use_lines_they_came_through() -> None:
	text = textwrap.dedent("""
	def leg h=1
	  b 0,0,~ .1,.1,h wood
	prop stool_a "Stool"
	  use leg .2,0,0 mx
	  b 0,0,1 .5,.3,.05 wood
	prop two_stools "Two"
	  use stool_a -1,0,0
	  use stool_a 1,0,0
	""")
	built = ps.Project.from_text(text, "t.parts").build("two_stools")
	counts: dict = {}
	for baked in built.baked:
		for polygon in baked["polygons"]:
			key = built.origins[polygon["origin"]]
			counts[key] = counts.get(key, 0) + 1
	# Each stool: its seat (6 faces) straight from the stool's body, its two legs (12) through the leg def.
	assert counts == {(("t.parts", 8),): 6, (("t.parts", 8), ("t.parts", 5)): 12, (("t.parts", 9),): 6, (("t.parts", 9), ("t.parts", 5)): 12}
