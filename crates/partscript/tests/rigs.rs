//! Pivots, parents and marks: a prop's node tree, for things that move (a rifle's bolt, a door, a limb).

mod common;

use common::*;
use kitlib::json::Json;
use partscript::{BuildOptions, Host, Project};

const RIFLE: &str = "
prop bolt_rifle \"Bolt Rifle\" weapons hero=1
  part receiver
    box at=0,0,.03 size=.045,.32,.06 mat=steel_dark
  part bolt parent=receiver pivot=0,.02,.045
    cylinder at=0,.02,.045 radius=.011 height=.16 mat=steel_grey axis=y sides=8
  part bolt_handle parent=bolt pivot=0,.09,.045
    cylinder at=.035,.09,.045 radius=.005 height=.05 mat=steel_grey axis=x sides=6
  mark muzzle at=0,-.62,.05
  mark eject at=.03,0,.06 on=receiver turn=0,0,90
";

fn glb_document(glb: &[u8]) -> Json {
	Json::parse(&kitlib::gltf::glb_json(glb)).unwrap()
}

fn node<'a>(doc: &'a Json, name: &str) -> (usize, &'a Json) {
	doc.get("nodes").unwrap().as_list().iter().enumerate().find(|(_, n)| n.get("name").unwrap().as_str() == name).unwrap()
}

fn floats(value: Option<&Json>) -> Vec<f64> {
	value.map(|v| v.as_list().iter().map(|x| (x.as_f64() * 1e5).round() / 1e5).collect()).unwrap_or_default()
}

#[test]
fn a_rig_checks_clean_and_builds_a_node_tree() {
	let mut project = Project::from_text(&dedent(RIFLE), "rifle.parts", Host::default());
	let report = project.check();
	assert!(report.errors.is_empty() && report.warnings.is_empty(), "{:?} {:?}", report.errors, report.warnings);
	let built = project.build("bolt_rifle", &BuildOptions::default()).unwrap();
	assert!(built.rigged && built.warnings.is_empty(), "{:?}", built.warnings);
	let names: Vec<(&str, Option<&str>, bool)> = built.nodes.iter().map(|n| (n.name.as_str(), n.parent.as_deref(), n.mark)).collect();
	assert_eq!(
		names,
		vec![
			("receiver", None, false),
			("bolt", Some("receiver"), false),
			("bolt_handle", Some("bolt"), false),
			("muzzle", None, true),
			("eject", Some("receiver"), true)
		]
	);
	let doc = glb_document(&built.glb);
	let (root, root_node) = node(&doc, "bolt_rifle");
	assert_eq!(doc.get("scenes").unwrap().idx(0).get("nodes").unwrap().as_list(), &[Json::Int(root as i64)]);
	let children: Vec<i64> = root_node.get("children").unwrap().as_list().iter().map(Json::as_i64).collect();
	let (receiver, _) = node(&doc, "receiver");
	let (muzzle, muzzle_node) = node(&doc, "muzzle");
	assert_eq!(children, vec![receiver as i64, muzzle as i64]);
	// the bolt sits at its pivot (glTF axes: x, z, -y) and the handle at its own, relative to the bolt
	let (_, bolt) = node(&doc, "bolt");
	assert_eq!(floats(bolt.get("translation")), vec![0.0, 0.045, -0.02]);
	let (_, handle) = node(&doc, "bolt_handle");
	assert_eq!(floats(handle.get("translation")), vec![0.0, 0.0, -0.07]);
	// marks are empty nodes; a turned mark carries its rotation
	assert!(muzzle_node.get("mesh").is_none());
	assert_eq!(floats(muzzle_node.get("translation")), vec![0.0, 0.05, 0.62]);
	let (_, eject) = node(&doc, "eject");
	let q = floats(eject.get("rotation"));
	assert!((q[1].abs() - 0.70711).abs() < 1e-4 && (q[3] - 0.70711).abs() < 1e-4, "{q:?}");
	// the bolt's corners are written from its pivot: its mesh is centred on the node
	let bolt_mesh = bolt.get("mesh").unwrap().as_i64() as usize;
	let primitive = doc.get("meshes").unwrap().idx(bolt_mesh).get("primitives").unwrap().idx(0);
	let accessor = doc.get("accessors").unwrap().idx(primitive.get("attributes").unwrap().get("POSITION").unwrap().as_i64() as usize);
	let (lo, hi) = (floats(accessor.get("min")), floats(accessor.get("max")));
	for k in 0..3 {
		assert!((lo[k] + hi[k]).abs() < 1e-3, "{lo:?} {hi:?}");
	}
}

#[test]
fn a_prop_without_a_rig_keeps_its_plain_layout() {
	let mut project = Project::from_text(&dedent("
		prop crate_box \"Crate\"
		  box at=0,0,.2 size=.4,.4,.4 mat=wood
		  part lid
		  box at=0,0,.42 size=.42,.42,.04 mat=wood
	"), "c.parts", Host::default());
	let built = project.build("crate_box", &BuildOptions::default()).unwrap();
	assert!(!built.rigged);
	assert_eq!(built.nodes.iter().map(|n| (n.name.as_str(), n.baked)).collect::<Vec<_>>(), vec![("crate_box", Some(0)), ("lid", Some(1))]);
	let doc = glb_document(&built.glb);
	assert!(doc.get("nodes").unwrap().as_list().iter().all(|n| n.get("translation").is_none()));
}

#[test]
fn an_empty_part_a_rig_names_stays_as_a_node() {
	let mut project = Project::from_text(&dedent("
		prop door_frame \"Door\"
		  part hinge pivot=.4,0,0
		  part door parent=hinge pivot=.4,0,1
		  box at=0,0,1 size=.8,.04,2 mat=wood
	"), "d.parts", Host::default());
	let built = project.build("door_frame", &BuildOptions::default()).unwrap();
	assert_eq!(built.nodes.iter().map(|n| (n.name.as_str(), n.baked)).collect::<Vec<_>>(), vec![("hinge", None), ("door", Some(0))]);
	let doc = glb_document(&built.glb);
	let (_, door) = node(&doc, "door");
	assert_eq!(floats(door.get("translation")), vec![0.0, 1.0, 0.0]);
}

#[test]
fn a_rig_names_parts_that_exist_and_does_not_go_round() {
	let report = check("
		prop broken \"Broken\"
		  part a parent=b pivot=0,0,1
		  box at=0,0,1 size=.1,.1,.1 mat=wood
		  part b parent=a
		  box at=0,0,2 size=.1,.1,.1 mat=wood
		  part c parent=nothing
		  box at=0,0,3 size=.1,.1,.1 mat=wood
		  mark tip at=0,0,4 on=d
		  mark tip at=0,0,5
		  mark two words at=0,0,1
		  mark grip at=1,2 size=3
	");
	let errors = report.errors.join("\n");
	assert!(errors.contains("parents go round in a circle (a -> b -> a)"), "{errors}");
	assert!(errors.contains("parent=nothing: no part of that name in broken (parts: a, b, c)"), "{errors}");
	assert!(errors.contains("on=d: no part of that name"), "{errors}");
	assert!(errors.contains("tip names two nodes of the prop"), "{errors}");
	assert!(errors.contains("mark NAME at=X,Y,Z"), "{errors}");
	assert!(errors.contains("mark takes at= on= turn= (not size)"), "{errors}");
	assert!(errors.contains("expected 3 numbers"), "{errors}");
}

#[test]
fn a_pivot_far_from_its_part_is_a_warning() {
	let mut project = Project::from_text(&dedent("
		prop lever \"Lever\"
		  part arm pivot=0,0,9
		  box at=0,0,.5 size=.05,.05,1 mat=steel_grey
	"), "l.parts", Host::default());
	let built = project.build("lever", &BuildOptions::default()).unwrap();
	assert_eq!(built.warnings, vec!["lever: part arm's pivot is 8 m outside its shapes".to_string()]);
}

#[test]
fn marks_follow_the_group_they_are_in() {
	let mut project = Project::from_text(&dedent("
		prop post \"Post\"
		  box at=0,0,.5 size=.1,.1,1 mat=wood
		  group at=0,0,1 turn=0,0,90 {
		    mark top at=.2,0,0
		  }
	"), "p.parts", Host::default());
	let built = project.build("post", &BuildOptions::default()).unwrap();
	let top = built.nodes.iter().find(|n| n.mark).unwrap();
	assert!((top.at[0]).abs() < 1e-9 && (top.at[1] - 0.2).abs() < 1e-9 && (top.at[2] - 1.0).abs() < 1e-9, "{:?}", top.at);
}
