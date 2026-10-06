//! Files: variables per file, defs once, import (with and without a name) and parts called by name.
mod common;

use std::collections::HashSet;

use common::*;
use partscript::{fmt, Host, Project};

fn set(items: &[&str]) -> HashSet<String> {
	items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_files_variables_are_its_own() {
	let mut project = sources(&[
		("a.parts", "set paint=#c83a3a/paint\nprop red_a \"A\"\n  box size=.4 mat=paint\n"),
		("b.parts", "set paint=#3a6ac8/paint\nprop blue_b \"B\"\n  box size=.4 mat=paint\n"),
	]);
	assert_eq!(materials(&project.build("red_a", &no_glb()).unwrap()), set(&["xc83a3a_paint"]));
	assert_eq!(materials(&project.build("blue_b", &no_glb()).unwrap()), set(&["x3a6ac8_paint"]));
	let other = sources(&[("a.parts", "set paint=#c83a3a/paint\n"), ("c.parts", "prop plain_c \"C\"\n  box size=.4 mat=paint\n")]);
	assert!(has(&other.check().errors, "unknown material 'paint'"));
}

#[test]
fn a_def_draws_with_its_own_files_variables() {
	let mut project = sources(&[
		("lib.parts", "set red=#c83a3a/paint\ndef red_box\n  box size=.4 mat=red\n"),
		("room.parts", "set red=#00ff00/paint\nprop room_a \"Room\"\n  use red_box\n"),
	]);
	assert_eq!(materials(&project.build("room_a", &no_glb()).unwrap()), set(&["xc83a3a_paint"]));
}

#[test]
fn a_def_defined_twice_is_a_mistake() {
	let project = sources(&[("a.parts", "def stool\n  box size=.3 mat=wood\n"), ("b.parts", "def stool\n  box size=.4 mat=wood\n")]);
	assert!(has(&project.errors(), "def stool defined twice (also a.parts:1)"));
}

#[test]
fn replacing_a_std_part_is_allowed_with_a_warning() {
	let mut project = sources(&[("a.parts", "def table\n  box size=.3 mat=wood\nprop t_a \"T\"\n  use table\n")]);
	assert!(project.errors().is_empty());
	assert!(has(&project.check().warnings, "replaces the std part table"));
	assert_eq!(project.build("t_a", &no_glb()).unwrap().triangles(), 12);
}

#[test]
fn a_broken_block_does_not_spill_its_lines() {
	let project = sources(&[("a.parts", "def stool\n  box size=.3 mat=wood\ndef stool\n  box size=.4 mat=wood\n  cylinder radius=.1 height=.1 mat=wood\n")]);
	assert_eq!(project.errors().len(), 1);
}

#[test]
fn import_brings_in_a_file_relative_to_the_one_importing() {
	let root = scratch("import");
	write(&root, &[
		("lib/furniture.parts", "
		set oak=#7a5a32/wood
		def stool h=.6
		  cylinder at=0,0,on(h) radius=.17 height=.04 mat=oak sides=10
		prop crate_lib \"Crate\"
		  box size=.5 mat=crate_wood
		"),
		("props/room.parts", "
		import \"../lib/furniture.parts\"
		prop room_b \"Room\"
		  use stool
		  use crate_lib at=1,0,0
		"),
	]);
	let mut project = Project::from_paths(&[root.join("props")], Host::default()).unwrap();
	assert!(project.errors().is_empty() && project.check().errors.is_empty(), "{:?} {:?}", project.errors(), project.check().errors);
	assert!(project.build("room_b", &no_glb()).unwrap().triangles() > 0);
	assert_eq!(project.props().iter().filter(|p| !p.imported).map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["room_b"]);
	let out = project.write_all(&root.join("out"), None, None);
	assert_eq!(out.built.iter().map(|b| b.0.as_str()).collect::<Vec<_>>(), vec!["room_b"]); // imported props are used, not built
}

#[test]
fn import_finds_a_folder_and_a_name_without_parts() {
	let root = scratch("folder");
	write(&root, &[
		("lib/a.parts", "def part_a\n  box size=.2 mat=wood\n"),
		("lib/more/b.parts", "def part_b\n  box size=.2 mat=wood\n"),
		("main.parts", "import \"lib\"\nimport \"lib/a\"\nprop main_c \"M\"\n  use part_a\n  use part_b at=1,0,0\n"),
	]);
	let mut project = Project::from_paths(&[root.join("main.parts")], Host::default()).unwrap();
	assert!(project.errors().is_empty(), "{:?}", project.errors());
	assert_eq!(project.build("main_c", &no_glb()).unwrap().triangles(), 24);
}

#[test]
fn a_missing_import_says_so() {
	let root = scratch("missing");
	write(&root, &[("main.parts", "import \"nowhere.parts\"\n")]);
	let project = Project::from_paths(&[root.join("main.parts")], Host::default()).unwrap();
	assert!(has(&project.errors(), "import \"nowhere.parts\": no such file or folder"), "{:?}", project.errors());
}

#[test]
fn import_as_keeps_a_librarys_names_apart() {
	let root = scratch("as");
	write(&root, &[
		("lib/furniture.parts", "
		def leg
		  box size=.05,.05,.7 mat=wood
		def table
		  box at=0,0,on(.7) size=1,.6,.04 mat=wood
		  use leg at=.45,.25,0 mirror xy
		"),
		("main.parts", "
		import \"lib/furniture.parts\" as furniture
		def leg
		  box size=1 mat=wood
		prop dining \"Dining\"
		  use furniture.table
		  use table at=2,0,0
		"),
	]);
	let mut project = Project::from_paths(&[root.join("main.parts")], Host::default()).unwrap();
	assert!(project.errors().is_empty() && project.check().errors.is_empty(), "{:?} {:?}", project.errors(), project.check().errors);
	assert_eq!(project.build("dining", &no_glb()).unwrap().triangles(), 12 * 5 + 12 * 5);
}

#[test]
fn the_same_file_plainly_and_by_name() {
	let root = scratch("both");
	write(&root, &[
		("lib.parts", "def peg\n  box size=.1 mat=wood\n"),
		("main.parts", "import \"lib.parts\"\nimport \"lib.parts\" as lib\nprop pegs \"P\"\n  use peg\n  use lib.peg at=1,0,0\n"),
	]);
	let mut project = Project::from_paths(&[root.join("main.parts")], Host::default()).unwrap();
	assert!(project.errors().is_empty());
	assert_eq!(project.build("pegs", &no_glb()).unwrap().triangles(), 24);
}

#[test]
fn an_imported_kit_finds_its_own_pieces() {
	let root = scratch("kit");
	write(&root, &[
		("kitlib.parts", "
		prop wall_a \"W\"
		  box at=0,0,on size=4,.2,3 mat=brick
		prop floor_a \"F\"
		  box at=0,0,-.1 size=4,4,.2 mat=concrete
		kit plain grid=4 storey=3
		  wall solid=wall_a
		  piece floor=floor_a
		"),
		("main.parts", "import \"kitlib.parts\" as k\nbuilding hut \"Hut\" kit=k.plain\n  room 0,0 1,1\n"),
	]);
	let mut project = Project::from_paths(&[root.join("main.parts")], Host::default()).unwrap();
	assert!(project.errors().is_empty() && project.check().errors.is_empty(), "{:?} {:?}", project.errors(), project.check().errors);
	assert!(project.build("hut", &no_glb()).unwrap().triangles() > 0);
}

#[test]
fn a_part_can_be_called_by_its_name() {
	let mut project = sources(&[
		(
			"a.parts",
			"def stool_x h=.6\n  box at=0,0,on(h) size=.3,.3,.04 mat=wood\nprop calls \"C\"\n  stool_x at=0,0,0 h=.5\n  seat = stool_x at=1,0,0\n  box on=seat size=.1 mat=wood\n  table w=1\n  crate_y\n",
		),
		("b.parts", "prop crate_y \"Crate\"\n  box size=.4 mat=crate_wood\n"),
	]);
	assert!(project.errors().is_empty() && project.check().errors.is_empty(), "{:?}", project.check().errors);
	assert_eq!(project.build("calls", &no_glb()).unwrap().triangles(), 12 * 3 + 12 * 5 + 12);
}

#[test]
fn an_unknown_word_is_an_unknown_statement() {
	let project = sources(&[("a.parts", "prop typo \"T\"\n  bxo size=.3 mat=wood\n")]);
	assert!(has(&project.check().errors, "unknown statement 'bxo'"));
}

#[test]
fn fmt_keeps_a_call_as_written() {
	for line in ["stool_x at=0,0,0 h=.5", "seat = stool_x at=1,0,0", "furniture.table at=2,0,0"] {
		assert_eq!(fmt::readable(&format!("  {line}")).trim(), line);
		assert_eq!(fmt::terse(&format!("  {line}")).trim(), line);
	}
}
