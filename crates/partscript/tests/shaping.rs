//! Shapes and tools for detail: torus, frame, smooth curves, twist/bend/shrink, named copy numbers, if, \.
mod common;

use std::f64::consts::PI;

use common::*;
use kitlib::paths::smooth_points;
use partscript::{Built, Host, Project};

fn shaped(text: &str, name: &str) -> Built {
	let mut p = Project::from_text(&dedent(text), "s.parts", Host::default());
	assert!(p.errors().is_empty() && p.check().errors.is_empty(), "{:?} {:?}", p.errors(), p.check().errors);
	p.build(name, &no_glb()).unwrap()
}

fn extent(built: &Built) -> ([f64; 3], [f64; 3]) {
	let pts: Vec<[f64; 3]> = built.parts.iter().flat_map(|p| p.faces.iter().flat_map(|f| f.points.iter().copied())).collect();
	let lo = [0, 1, 2].map(|k| round(pts.iter().map(|q| q[k]).fold(f64::INFINITY, f64::min), 3));
	let hi = [0, 1, 2].map(|k| round(pts.iter().map(|q| q[k]).fold(f64::NEG_INFINITY, f64::max), 3));
	(lo, hi)
}

fn near(a: f64, b: f64, tol: f64) -> bool {
	(a - b).abs() <= tol
}

#[test]
fn torus_lies_flat_or_stands_on_an_axis() {
	let (low, high) = extent(&shaped("prop rr \"Ring\"\n  torus at=0,0,1 radius=.5 thick=.05 mat=brass sides=24\n", "rr"));
	assert!(near(high[2] - low[2], 2.0 * 0.05 * 60f64.to_radians().sin(), 0.005));
	assert!(near(high[0], 0.55, 0.01));
	let (low, high) = extent(&shaped("prop rr \"Ring\"\n  torus at=0,0,on radius=.5 thick=.05 mat=rubber axis=x sides=24\n", "rr"));
	assert!(near(low[2], 0.0, 0.01) && near(high[2], 1.1, 0.01) && near(high[0], 0.05, 0.01));
}

#[test]
fn frame_leaves_its_hole_open() {
	let built = shaped("prop ff \"Frame\"\n  frame at=0,0,1 width=1 height=.8 depth=.04 mat=wood hole=.6,.4\n", "ff");
	assert_eq!(built.triangles(), 2 * 12 + 2 * 8);
	let inside = built.parts[0].faces.iter().flat_map(|f| f.points.iter()).filter(|q| q[0].abs() < 0.29 && (q[2] - 1.0).abs() < 0.19).count();
	assert_eq!(inside, 0);
}

#[test]
fn smooth_passes_through_every_point() {
	let points = [[0.0, 0.0, 0.0], [1.0, 0.0, 1.0], [2.0, 0.0, 0.0], [3.0, 0.0, 1.0]];
	let curve = smooth_points(&points, 4, false);
	assert_eq!(curve.len(), 3 * 4 + 1);
	assert!(points.iter().all(|p| curve.contains(p)));
	let smooth = shaped("prop vv \"Vine\"\n  pipe mat=wood radius=.02 0,0,0 .5,0,.5 1,0,0 smooth=6 sides=4\n", "vv");
	let plain = shaped("prop vv \"Vine\"\n  pipe mat=wood radius=.02 0,0,0 .5,0,.5 1,0,0 sides=4\n", "vv");
	assert!(smooth.triangles() > plain.triangles() * 3);
}

#[test]
fn twist_shrink_and_bend() {
	let plain = extent(&shaped("prop cc \"C\"\n  box at=0,0,on size=.4,.4,2 mat=stone\n", "cc"));
	let twisted = extent(&shaped("prop cc \"C\"\n  box at=0,0,on size=.4,.4,2 mat=stone twist=45\n", "cc"));
	assert!(near(twisted.1[0], 0.2 * 2f64.sqrt(), 0.01) && near(plain.1[0], 0.2, 1e-9));
	let shrunk = extent(&shaped("prop cc \"C\"\n  box at=0,0,on size=.4,.4,2 mat=stone shrink=.5\n", "cc"));
	assert!(near(shrunk.1[0], 0.2, 1e-9));
	let bent = extent(&shaped("prop cc \"C\"\n  cylinder at=0,0,on radius=.05 height=2 mat=steel_dark sides=6 bend=90\n", "cc"));
	let radius = 2.0 / (PI / 2.0);
	assert!(near(bent.1[1], radius, 0.02) && near(bent.1[2], radius + 0.05, 0.02));
	let sideways = extent(&shaped("prop cc \"C\"\n  cylinder at=0,0,on radius=.05 height=2 mat=steel_dark sides=6 bend=90,90\n", "cc"));
	assert!(near(sideways.0[0], -radius, 0.02));
}

fn centres(built: &Built) -> Vec<(f64, f64)> {
	let faces = &built.parts[0].faces;
	let mut out: Vec<(f64, f64)> = (0..faces.len())
		.step_by(6)
		.map(|k| {
			let q: Vec<[f64; 3]> = faces[k..k + 6].iter().flat_map(|f| f.points.iter().copied()).collect();
			(round(q.iter().map(|p| p[0]).sum::<f64>() / 24.0, 2) + 0.0, round(q.iter().map(|p| p[1]).sum::<f64>() / 24.0, 2) + 0.0)
		})
		.collect();
	out.sort_by(|a, b| a.partial_cmp(b).unwrap());
	out.dedup();
	out
}

#[test]
fn a_grid_names_its_columns_and_rows() {
	let built = shaped("prop gg \"G\"\n  box at=0,0,0 size=.1 mat=wood grid 3x3 every .5,.5 as col,row when=col==row\n", "gg");
	assert_eq!(centres(&built), vec![(0.0, 0.0), (0.5, 0.5), (1.0, 1.0)]);
}

#[test]
fn nested_copies_keep_their_own_names() {
	let built = shaped(
		"
	prop shelves \"Shelves\"
	  group at=0,0,0 repeat 3 every 0,0,.5 as shelf {
	    box at=0,0,shelf*.5 size=.1,.1,.05+.05*item mat=wood repeat 2 every .3,0,0 as item
	  }
	",
		"shelves",
	);
	let faces = &built.parts[0].faces;
	let mut heights: Vec<f64> = (0..faces.len())
		.step_by(6)
		.map(|k| {
			let z: Vec<f64> = faces[k..k + 6].iter().flat_map(|f| f.points.iter().map(|p| p[2])).collect();
			round(z.iter().copied().fold(f64::NEG_INFINITY, f64::max) - z.iter().copied().fold(f64::INFINITY, f64::min), 3)
		})
		.collect();
	heights.sort_by(f64::total_cmp);
	heights.dedup();
	assert_eq!(heights, vec![0.05, 0.1]);
}

#[test]
fn if_blocks_and_continued_lines() {
	let built = shaped(
		"
	prop tidy \"Tidy\"
	  set big=1
	  if big == 1 and 2 > 1 {
	    box at=0,0,0 \\
	        size=.5 \\
	        mat=wood
	  }
	  if big == 0 {
	    box at=2,0,0 size=.5 mat=wood
	  }
	  box at=0,0,1 size=.1 mat=brass
	",
		"tidy",
	);
	assert_eq!(built.triangles(), 24);
	let p = Project::from_text("prop tidy \"Tidy\"\n  box at=0,0,0 \\\n    size=.5 \\\n    mat=nope\n  box at=0,0,0 size=.1 mat=nope2\n", "t.parts", Host::default());
	let errors = p.check().errors;
	assert!(errors.iter().any(|e| e.starts_with("t.parts:2:") && e.contains("nope")) && errors.iter().any(|e| e.starts_with("t.parts:5:")), "{errors:?}");
}

#[test]
fn an_option_a_shape_does_not_take_is_an_error() {
	let p = Project::from_text("prop aa \"A\"\n  torus at=0,0,0 radius=.3 radius_out=0 thick=.02 mat=rubber\n", "<text>", Host::default());
	let errors = p.check().errors;
	assert!(errors.len() == 1 && errors[0].contains("torus: no option radius_out=") && errors[0].contains("sides"), "{errors:?}");
}
