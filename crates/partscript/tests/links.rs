//! Links: loose ends a join line bridges when they nearly meet.
mod common;

use common::*;
use partscript::{Built, Host, Project};

const RAIL: &str = "
def run len=1
  pipe mat=steel_dark radius=.02 0,0,.9 len,0,.9 sides=4
  link rail at=0,0,.9 toward=-x
  link rail at=len,0,.9 toward=+x
def swag
  box at=length/2,0,0 size=length,.02,.02 mat=steel_dark
";

fn rail(body: &str) -> Built {
	let mut p = Project::from_text(&format!("{RAIL}prop demo \"Demo\"\n{}", dedent(body)), "t.parts", Host::default());
	assert!(p.check().errors.is_empty(), "{:?}", p.check().errors);
	p.build("demo", &no_glb()).unwrap()
}

fn ends(built: &Built) -> Vec<[f64; 3]> {
	let mut out: Vec<[f64; 3]> = built.links.iter().map(|e| e.pos.map(|v| round(v, 3) + 0.0)).collect();
	out.sort_by(|a, b| a.partial_cmp(b).unwrap());
	out
}

#[test]
fn gap_is_bridged_and_outer_ends_stay_open() {
	let built = rail("  use run at=0,0,0\n  use run at=1.3,0,0\n  join rail\n");
	assert_eq!(ends(&built), vec![[0.0, 0.0, 0.9], [2.3, 0.0, 0.9]]);
	assert_eq!(built.joints.len(), 2);
}

#[test]
fn no_join_line_leaves_every_end_open() {
	let built = rail("  use run at=0,0,0\n  use run at=1.3,0,0\n");
	assert_eq!(built.links.len(), 4);
	assert!(built.joints.is_empty());
}

#[test]
fn reach_limits_the_gap() {
	assert_eq!(rail("  use run at=0,0,0\n  use run at=1.5,0,0\n  join rail reach=.4\n").links.len(), 4);
}

#[test]
fn ends_that_face_away_do_not_join() {
	assert_eq!(rail("  use run at=0,0,0\n  use run at=.8,.1,0 turn=0,0,180\n  join rail\n").links.len(), 4);
}

#[test]
fn side_by_side_ends_make_a_return() {
	let built = rail("  use run at=0,0,0\n  use run at=1,.2,0 turn=0,0,180\n  join rail\n");
	assert!(built.links.is_empty());
	assert_eq!(built.joints.len(), 4);
}

#[test]
fn corner_is_bridged() {
	let built = rail("  use run at=0,0,0\n  use run at=1.2,.2,0 turn=0,0,90\n  join rail\n");
	assert_eq!(ends(&built), vec![[0.0, 0.0, 0.9], [1.2, 1.2, 0.9]]);
}

#[test]
fn touching_ends_count_as_joined_without_a_bridge() {
	let plain = rail("  use run at=0,0,0\n  use run at=1,0,0\n");
	let joined = rail("  use run at=0,0,0\n  use run at=1,0,0\n  join rail\n");
	assert_eq!(joined.links.len(), 2);
	assert_eq!(joined.triangles(), plain.triangles());
}

#[test]
fn each_end_joins_once_nearest_first() {
	let built = rail("  use run at=0,0,0\n  use run at=1.2,0,0\n  use run at=1.25,.1,0\n  join rail\n");
	assert!(ends(&built).contains(&[1.25, 0.1, 0.9]));
	assert_eq!(built.links.len(), 4);
}

#[test]
fn join_with_def_draws_the_bridge() {
	let plain = rail("  use run at=0,0,0\n  use run at=1.5,0,0\n  join rail\n");
	let swag = rail("  use run at=0,0,0\n  use run at=1.5,0,0\n  join rail with=swag\n");
	assert_eq!(swag.links.len(), 2);
	assert_ne!(swag.triangles(), plain.triangles());
}

#[test]
fn join_inside_a_def_resolves_there_and_leaves_its_open_ends() {
	let text = format!(
		"{RAIL}{}",
		dedent(
			"
		def pair
		  use run at=0,0,0
		  use run at=1.1,0,0
		  join rail
		prop demo \"Demo\"
		  use pair at=0,0,0
		  use pair at=2.4,0,0
		  join rail
		"
		)
	);
	let built = Project::from_text(&text, "t.parts", Host::default()).build("demo", &no_glb()).unwrap();
	assert_eq!(ends(&built), vec![[0.0, 0.0, 0.9], [4.5, 0.0, 0.9]]);
	assert_eq!(built.joints.len(), 6);
}

#[test]
fn join_checks_its_options() {
	let p = Project::from_text(&format!("{RAIL}prop demo \"Demo\"\n  use run\n  join rail with=nope\n"), "t.parts", Host::default());
	assert!(!p.check().errors.is_empty());
	let p = Project::from_text(&format!("{RAIL}prop demo \"Demo\"\n  link rail at=0,0,0 toward=sideways\n"), "t.parts", Host::default());
	assert!(!p.check().errors.is_empty());
}

#[test]
fn repeat_count_with_an_x_in_brackets() {
	assert_eq!(rail("  box at=0,0,on size=.1,.1,.1 mat=steel_dark repeat (max(1,3)) every .2,0,0\n").triangles(), 3 * 12);
}
