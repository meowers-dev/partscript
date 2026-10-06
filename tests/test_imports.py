"""Files: variables per file, defs once, import (with and without a name) and parts called by name."""

from __future__ import annotations

import textwrap
from pathlib import Path

import pytest

import partscript as ps
from partscript import fmt


def write(root: Path, files: dict) -> None:
	for name, text in files.items():
		path = root / name
		path.parent.mkdir(parents=True, exist_ok=True)
		path.write_text(textwrap.dedent(text))


def materials(built) -> set:
	return {f.material for part in built.parts for f in part.faces}


def test_a_files_variables_are_its_own() -> None:
	project = ps.Project([("a.parts", 'set paint=#c83a3a/paint\nprop red_a "A"\n  box size=.4 mat=paint\n'),
		("b.parts", 'set paint=#3a6ac8/paint\nprop blue_b "B"\n  box size=.4 mat=paint\n')])
	assert materials(project.build("red_a", glb=False)) == {"xc83a3a_paint"}
	assert materials(project.build("blue_b", glb=False)) == {"x3a6ac8_paint"}
	other = ps.Project([("a.parts", 'set paint=#c83a3a/paint\n'), ("c.parts", 'prop plain_c "C"\n  box size=.4 mat=paint\n')])
	assert any("unknown material 'paint'" in e for e in other.check()["errors"])


def test_a_def_draws_with_its_own_files_variables() -> None:
	project = ps.Project([("lib.parts", 'set red=#c83a3a/paint\ndef red_box\n  box size=.4 mat=red\n'),
		("room.parts", 'set red=#00ff00/paint\nprop room_a "Room"\n  use red_box\n')])
	assert materials(project.build("room_a", glb=False)) == {"xc83a3a_paint"}


def test_a_def_defined_twice_is_a_mistake() -> None:
	project = ps.Project([("a.parts", "def stool\n  box size=.3 mat=wood\n"), ("b.parts", "def stool\n  box size=.4 mat=wood\n")])
	assert any("def stool defined twice (also a.parts:1)" in e for e in project.errors)


def test_replacing_a_std_part_is_allowed_with_a_warning() -> None:
	project = ps.Project([("a.parts", 'def table\n  box size=.3 mat=wood\nprop t_a "T"\n  use table\n')])
	assert project.errors == [] and any("replaces the std part table" in w for w in project.check()["warnings"])
	assert project.build("t_a", glb=False).triangles == 12


def test_a_broken_block_does_not_spill_its_lines() -> None:
	project = ps.Project([("a.parts", "def stool\n  box size=.3 mat=wood\ndef stool\n  box size=.4 mat=wood\n  cylinder radius=.1 height=.1 mat=wood\n")])
	assert len(project.errors) == 1


def test_import_brings_in_a_file_relative_to_the_one_importing(tmp_path: Path) -> None:
	write(tmp_path, {"lib/furniture.parts": """
		set oak=#7a5a32/wood
		def stool h=.6
		  cylinder at=0,0,on(h) radius=.17 height=.04 mat=oak sides=10
		prop crate_lib "Crate"
		  box size=.5 mat=crate_wood
		""", "props/room.parts": """
		import "../lib/furniture.parts"
		prop room_b "Room"
		  use stool
		  use crate_lib at=1,0,0
		"""})
	project = ps.Project.from_paths([tmp_path / "props"])
	assert project.errors == [] and project.check()["errors"] == []
	assert project.build("room_b", glb=False).triangles > 0
	assert [p["id"] for p in project.props() if not p["imported"]] == ["room_b"]
	out = project.write_all(tmp_path / "out")
	assert [row["id"] for row in out["built"]] == ["room_b"]  # an imported file's props are used, not built


def test_import_finds_a_folder_and_a_name_without_parts(tmp_path: Path) -> None:
	write(tmp_path, {"lib/a.parts": "def part_a\n  box size=.2 mat=wood\n", "lib/more/b.parts": "def part_b\n  box size=.2 mat=wood\n",
		"main.parts": 'import "lib"\nimport "lib/a"\nprop main_c "M"\n  use part_a\n  use part_b at=1,0,0\n'})
	project = ps.Project.from_paths([tmp_path / "main.parts"])
	assert project.errors == [] and project.build("main_c", glb=False).triangles == 24


def test_a_missing_import_says_so(tmp_path: Path) -> None:
	write(tmp_path, {"main.parts": 'import "nowhere.parts"\n'})
	project = ps.Project.from_paths([tmp_path / "main.parts"])
	assert any('import "nowhere.parts": no such file or folder' in e for e in project.errors)


def test_import_as_keeps_a_librarys_names_apart(tmp_path: Path) -> None:
	write(tmp_path, {"lib/furniture.parts": """
		def leg
		  box size=.05,.05,.7 mat=wood
		def table
		  box at=0,0,on(.7) size=1,.6,.04 mat=wood
		  use leg at=.45,.25,0 mirror xy
		""", "main.parts": """
		import "lib/furniture.parts" as furniture
		def leg
		  box size=1 mat=wood
		prop dining "Dining"
		  use furniture.table
		  use table at=2,0,0
		"""})
	project = ps.Project.from_paths([tmp_path / "main.parts"])
	assert project.errors == [] and project.check()["errors"] == []
	built = project.build("dining", glb=False)
	assert built.triangles == 12 * 5 + 12 * 5  # furniture.table uses its own leg; table is the std table


def test_the_same_file_plainly_and_by_name(tmp_path: Path) -> None:
	write(tmp_path, {"lib.parts": "def peg\n  box size=.1 mat=wood\n",
		"main.parts": 'import "lib.parts"\nimport "lib.parts" as lib\nprop pegs "P"\n  use peg\n  use lib.peg at=1,0,0\n'})
	project = ps.Project.from_paths([tmp_path / "main.parts"])
	assert project.errors == [] and project.build("pegs", glb=False).triangles == 24


def test_an_imported_kit_finds_its_own_pieces(tmp_path: Path) -> None:
	write(tmp_path, {"kitlib.parts": """
		prop wall_a "W"
		  box at=0,0,on size=4,.2,3 mat=brick
		prop floor_a "F"
		  box at=0,0,-.1 size=4,4,.2 mat=concrete
		kit plain grid=4 storey=3
		  wall solid=wall_a
		  piece floor=floor_a
		""", "main.parts": 'import "kitlib.parts" as k\nbuilding hut "Hut" kit=k.plain\n  room 0,0 1,1\n'})
	project = ps.Project.from_paths([tmp_path / "main.parts"])
	assert project.errors == [] and project.check()["errors"] == []
	assert project.build("hut", glb=False).triangles > 0


def test_a_part_can_be_called_by_its_name() -> None:
	project = ps.Project([("a.parts", 'def stool_x h=.6\n  box at=0,0,on(h) size=.3,.3,.04 mat=wood\n'
		'prop calls "C"\n  stool_x at=0,0,0 h=.5\n  seat = stool_x at=1,0,0\n  box on=seat size=.1 mat=wood\n  table w=1\n  crate_y\n'),
		("b.parts", 'prop crate_y "Crate"\n  box size=.4 mat=crate_wood\n')])
	assert project.errors == [] and project.check()["errors"] == []
	assert project.build("calls", glb=False).triangles == 12 * 3 + 12 * 5 + 12


def test_an_unknown_word_is_an_unknown_statement() -> None:
	project = ps.Project([("a.parts", 'prop typo "T"\n  bxo size=.3 mat=wood\n')])
	assert any("unknown statement 'bxo'" in e for e in project.check()["errors"])


@pytest.mark.parametrize("line", ["stool_x at=0,0,0 h=.5", "seat = stool_x at=1,0,0", "furniture.table at=2,0,0"])
def test_fmt_keeps_a_call_as_written(line: str) -> None:
	assert fmt.readable("  " + line).strip() == line and fmt.terse("  " + line).strip() == line
