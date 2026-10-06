//! Snapping and paths: chain, along=, joints=, snap markers.
mod common;

use common::*;
use kitlib::paths::path_frames;
use partscript::{BuildOptions, Host, Project};

const PIECES: &str = "
prop straight \"Straight\"
  box at=0,0,on size=1.2,2,.06 mat=concrete
  snap start 0,-1,0 -y kind=path
  snap end 0,1,0 +y kind=path
prop corner \"Corner\"
  box at=0,0,on size=1.2,1.2,.06 mat=concrete
  snap start 0,-.6,0 -y kind=path
  snap end .6,0,0 +x kind=path
prop rise \"Rise\"
  box at=0,0,on size=1.2,2,.3 mat=concrete
  snap start 0,-1,0 -y kind=path
  snap end 0,1,.3 +y kind=path
prop pipe \"Pipe\"
  box at=0,0,on size=.2,1,.2 mat=steel_grey
  snap start 0,-.5,0 -y kind=pipe
  snap end 0,.5,0 +y kind=pipe
";

fn with_pieces(extra: &str) -> Project {
	Project::from_text(&format!("{PIECES}{}", dedent(extra)), "p.parts", Host::default())
}

fn pts(points: &[[f64; 2]]) -> Vec<Vec<f64>> {
	points.iter().map(|p| p.to_vec()).collect()
}

#[test]
fn chain_joins_pieces_snap_to_snap() {
	let mut p = with_pieces("prop walk \"Walk\"\n  chain straight*2 corner straight rise\n");
	assert!(p.check().errors.is_empty());
	let built = p.build("walk", &no_glb()).unwrap();
	let joints: Vec<[f64; 3]> = built.joints.iter().map(|j| j.pos.map(|v| round(v, 3) + 0.0)).collect();
	assert_eq!(joints, vec![[0.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 4.0, 0.0], [0.6, 4.6, 0.0], [2.6, 4.6, 0.0], [4.6, 4.6, 0.3]]);
	assert_eq!(built.triangles(), 5 * 12 - 6); // the corner's start face sits on the straight's end face and is deduped
}

#[test]
fn chain_refuses_pieces_whose_kinds_do_not_fit() {
	let mut p = with_pieces("prop bad \"Bad\"\n  chain straight pipe\n");
	assert!(p.build("bad", &BuildOptions::default()).err().unwrap().to_string().contains("kind pipe"));
}

#[test]
fn along_fits_panels_and_puts_posts_at_the_joins() {
	let yard = pts(&[[0.0, 0.0], [6.0, 0.0], [6.0, 4.0], [0.0, 4.0]]);
	let frames = path_frames(&yard, 2.0, true, false, true, false).unwrap();
	assert_eq!(frames.len(), 10);
	assert!(frames.iter().all(|f| round(f.stretch, 6) == 1.0));
	let odd = path_frames(&pts(&[[0.0, 0.0], [5.0, 0.0]]), 2.0, true, false, false, false).unwrap();
	assert_eq!(odd.len(), 2);
	assert!(close(odd[0].stretch, 1.25));
	let posts = path_frames(&yard, 2.0, true, false, true, true).unwrap();
	let first: Vec<[f64; 2]> = posts.iter().take(4).map(|f| [round(f.pos[0], 3), round(f.pos[1], 3)]).collect();
	assert_eq!(first, vec![[0.0, 0.0], [2.0, 0.0], [4.0, 0.0], [6.0, 0.0]]);
	assert_eq!(posts.len(), 10);
	let corners = path_frames(&pts(&[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0]]), 0.0, false, true, false, false).unwrap();
	assert_eq!(corners.iter().map(|f| kitlib::py::round(f.yaw)).collect::<Vec<_>>(), vec![0.0, 45.0, 90.0]);
}

#[test]
fn along_in_a_prop_follows_a_named_line_with_a_gap() {
	let mut p = with_pieces(
		"
	prop yard \"Yard\"
	  set edge=\"0,0 6,0 6,4\"
	  box at=0,0,on size=2,.04,.8 mat=wood along=edge every=2 fit=1 when=i!=1
	",
	);
	assert!(p.check().errors.is_empty(), "{:?}", p.check().errors);
	let built = p.build("yard", &no_glb()).unwrap();
	let faces = &built.parts[0].faces;
	let mut centres: Vec<(f64, f64)> = (0..faces.len())
		.step_by(6)
		.map(|k| {
			let q: Vec<[f64; 3]> = faces[k..k + 6].iter().flat_map(|f| f.points.iter().copied()).collect();
			(round(q.iter().map(|p| p[0]).sum::<f64>() / 24.0, 2) + 0.0, round(q.iter().map(|p| p[1]).sum::<f64>() / 24.0, 2) + 0.0)
		})
		.collect();
	centres.sort_by(|a, b| a.partial_cmp(b).unwrap());
	centres.dedup();
	assert_eq!(centres, vec![(1.0, 0.0), (5.0, 0.0), (6.0, 1.0), (6.0, 3.0)]);
}

#[test]
fn snap_markers_are_a_separate_part_not_counted() {
	let mut p = with_pieces("prop walk \"Walk\"\n  chain straight corner\n");
	let plain = p.build("walk", &BuildOptions::default()).unwrap();
	let marked = p.build("walk", &BuildOptions { snaps: true, ..Default::default() }).unwrap();
	assert_eq!(marked.triangles(), plain.triangles());
	assert_eq!(marked.baked.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(), vec!["walk", "snaps"]);
	assert_eq!(marked.joints.len(), 3);
}
