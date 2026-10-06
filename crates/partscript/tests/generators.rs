//! Generators: noise, terrain, drop, scatter on, break and rubble, taper, here.
mod common;

use std::collections::HashSet;

use common::*;
use kitlib::geom::Face;
use kitlib::noise::{noise, rough};
use kitlib::surface::SurfaceIndex;
use partscript::{Built, Host, Project};

fn build_demo(body: &str, defs: &str, seed: &str) -> Built {
	let seed = if seed.is_empty() { String::new() } else { format!(" seed={seed}") };
	let text = format!("{}prop demo \"Demo\" budget=20000{seed}\n{}", dedent(defs), dedent(body));
	let mut project = Project::from_text(&text, "t.parts", Host::default());
	let report = project.check();
	assert!(project.errors().is_empty() && report.errors.is_empty(), "{:?} {:?}", project.errors(), report.errors);
	project.build("demo", &no_glb()).unwrap()
}

fn demo(body: &str) -> Built {
	build_demo(body, "", "")
}

fn faces(built: &Built) -> Vec<Face> {
	built.parts.iter().flat_map(|p| p.faces.iter().cloned()).collect()
}

fn zs(faces: &[Face]) -> impl Iterator<Item = f64> + '_ {
	faces.iter().flat_map(|f| f.points.iter().map(|q| q[2]))
}

fn max(it: impl Iterator<Item = f64>) -> f64 {
	it.fold(f64::NEG_INFINITY, f64::max)
}

fn min(it: impl Iterator<Item = f64>) -> f64 {
	it.fold(f64::INFINITY, f64::min)
}

#[test]
fn noise_is_smooth_seeded_and_in_range() {
	let values: Vec<f64> = (0..200).map(|x| noise(x as f64 * 0.1, 0.0, 0.0, "a")).collect();
	assert!(values.iter().all(|v| (0.0..=1.0).contains(v)));
	assert!(values.windows(2).map(|w| (w[0] - w[1]).abs()).fold(0.0, f64::max) < 0.1);
	assert_eq!(noise(1.3, 2.1, 0.0, "a"), noise(1.3, 2.1, 0.0, "a"));
	assert_ne!(noise(1.3, 2.1, 0.0, "a"), noise(1.3, 2.1, 0.0, "b"));
	assert!((0.0..=1.0).contains(&rough(3.3, 1.2, 0.0, "", 4)));
}

#[test]
fn noise_in_expressions_follows_the_prop_seed() {
	let body = "  box size=.2,.2,.2+noise(i*.37,1) mat=wood repeat 6 every .3,0,0\n";
	let tops = |b: &Built| -> Vec<f64> { let f = faces(b); (0..6).map(|k| round(max(zs(&f[k * 6..k * 6 + 6])), 5)).collect() };
	let (one, again, other) = (demo(body), demo(body), build_demo(body, "", "7"));
	assert_eq!(tops(&one), tops(&again));
	assert_ne!(tops(&one), tops(&other));
}

#[test]
fn here_is_where_the_copy_stands() {
	let built = demo("  box size=.1,.1,.1+here.x mat=wood repeat 3 every 1,0,0\n");
	let f = faces(&built);
	let mut tops: Vec<f64> = (0..3).map(|k| round(max(zs(&f[k * 6..k * 6 + 6])), 4)).collect();
	tops.sort_by(f64::total_cmp);
	assert_eq!(tops, vec![0.1, 1.1, 2.1]);
}

#[test]
fn terrain_follows_its_height_and_marks_steep_faces() {
	let built = demo("  terrain size=4,4 cells=8 height=max(0,x)*2 mat=#4a6a2a/fabric steep=stone slope=40\n");
	let tops: Vec<Face> = faces(&built).into_iter().filter(|f| f.points.len() == 3).collect();
	assert_eq!(tops.len(), 8 * 8 * 2);
	let materials: HashSet<String> = tops.iter().map(|f| f.material.to_string()).collect();
	assert_eq!(materials.len(), 2);
	assert!((max(zs(&tops)) - 4.0).abs() < 1e-9);
}

#[test]
fn drop_lands_things_on_what_is_below() {
	let built = demo(
		"
		  terrain size=4,4 cells=4 height=1.5 mat=#4a6a2a/fabric
		  box at=0,0,5 size=.2,.2,.2 mat=wood drop=1
		  box at=0,0,0 size=.2,.2,.2 mat=wood drop=1
		",
	);
	let f = faces(&built);
	let boxes = &f[f.len() - 12..];
	assert!((min(zs(&boxes[..6])) - 1.5).abs() < 1e-9); // fell from above
	assert!((min(zs(&boxes[6..])) - 1.5).abs() < 1e-9); // rose out of the hill
}

#[test]
fn dropped_things_pile_up() {
	let built = demo("  box at=0,0,3 size=.3,.3,.3 mat=wood drop=1 repeat 3 every 0,0,1\n");
	let f = faces(&built);
	let mut bottoms: Vec<f64> = (0..3).map(|k| round(min(zs(&f[k * 6..k * 6 + 6])), 4)).collect();
	bottoms.sort_by(f64::total_cmp);
	assert_eq!(bottoms, vec![0.0, 0.3, 0.6]);
}

#[test]
fn drop_lean_tilts_with_the_ground() {
	let built = demo(
		"
		  face #4a6a2a/fabric -2,-2,0 2,-2,2 2,2,2 -2,2,0
		  box at=0,0,4 size=.2,.2,.6 mat=wood drop=lean
		",
	);
	let f = faces(&built);
	let top: Vec<[f64; 3]> = f[1..].iter().flat_map(|f| f.points.iter().copied()).filter(|q| q[2] > 1.3).collect();
	assert!(!top.is_empty() && min(top.iter().map(|q| q[0])) < -0.1); // leaning back with the slope
}

#[test]
fn scatter_on_grows_on_the_faces_that_face_the_way_asked() {
	let built = demo(
		"
		  block = box size=2,2,1 mat=stone
		  box size=.04 mat=wood scatter 30 on block facing up
		",
	);
	let f = faces(&built);
	let moss = &f[6..];
	assert_eq!(moss.len(), 30 * 6);
	for face in moss.iter().step_by(6) {
		assert!((min(face.points.iter().map(|q| q[2])) - 1.0).abs() < 0.05);
	}
}

#[test]
fn scatter_on_side_faces_stands_copies_out_of_them() {
	let built = demo(
		"
		  wall = box size=2,.2,2 mat=stone
		  box size=.05,.05,.3 mat=wood scatter 10 on wall facing side
		",
	);
	let f = faces(&built);
	let ivy = &f[6..];
	for k in 0..10 {
		let points: Vec<[f64; 3]> = ivy[k * 6..k * 6 + 6].iter().flat_map(|f| f.points.iter().copied()).collect();
		let extent = |a: usize| max(points.iter().map(|q| q[a])) - min(points.iter().map(|q| q[a]));
		assert!((extent(0).max(extent(1)) - 0.3).abs() < 0.01);
	}
}

#[test]
fn break_knocks_chunks_out_and_rubble_falls() {
	let whole = demo("  box size=3,.3,2.4 mat=stone\n");
	let broken = demo("  box size=3,.3,2.4 mat=stone break=.4 chunk=.3 core=brick\n");
	let rubble = demo("  box size=3,.3,2.4 mat=stone break=.4 chunk=.3 core=brick rubble=.5\n");
	assert!(faces(&broken).len() > faces(&whole).len());
	assert!(max(zs(&faces(&broken))) <= 2.4 + 1e-6);
	assert!(faces(&rubble).len() > faces(&broken).len());
	let again = demo("  box size=3,.3,2.4 mat=stone break=.4 chunk=.3 core=brick rubble=.5\n");
	let firsts = |b: &Built| -> Vec<[f64; 3]> { faces(b).iter().map(|f| f.points[0]).collect() };
	assert_eq!(firsts(&again), firsts(&rubble));
}

#[test]
fn taper_narrows_a_pipe_toward_its_end() {
	let built = demo("  pipe mat=wood radius=.2 0,0,0 0,0,2 taper=.25 sides=6\n");
	let points: Vec<[f64; 3]> = faces(&built).iter().flat_map(|f| f.points.iter().copied()).collect();
	let at = |z: f64| max(points.iter().filter(|q| (q[2] - z).abs() < 1e-6).map(|q| (q[0] * q[0] + q[1] * q[1]).sqrt()));
	assert!((at(0.0) - 0.2).abs() < 1e-9 && (at(2.0) - 0.05).abs() < 1e-9);
}

#[test]
fn a_def_parameter_wins_over_a_placing_option() {
	let built = build_demo(
		"  use vine at=0,0,2 drop=.5\n",
		"
		def vine drop=.8
		  pipe mat=wood radius=.01 0,0,0 0,0,-drop sides=3
		",
		"",
	);
	assert!((min(zs(&faces(&built))) - 1.5).abs() < 1e-9);
}

#[test]
fn surface_index_finds_the_highest_surface_under_a_point() {
	let polygons = vec![
		vec![[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0]],
		vec![[-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]],
	];
	let ground = SurfaceIndex::new(polygons.iter(), 0.5);
	assert_eq!(ground.below(0.0, 0.0, 5.0).unwrap().0, 1.0);
	assert_eq!(ground.below(0.0, 0.0, 0.5).unwrap().0, 0.0);
	assert!(ground.below(3.0, 3.0, 5.0).is_none());
}

#[test]
fn mistakes_say_what_to_do() {
	for (body, message) in [
		("  box size=.1 mat=wood scatter 5 on nothing\n", "no shape of that name"),
		("  box size=.1 mat=wood drop=2\n", "drop=2"),
		("  box size=1 mat=wood break=1.5\n", "break=1.5"),
		("  box size=1 mat=wood core=brick\n", "go with break="),
		("  terrain size=4,4 cells=200 mat=wood\n", "cells=200"),
	] {
		let project = Project::from_text(&format!("prop demo \"Demo\"\n{body}"), "t.parts", Host::default());
		let errors = project.check().errors;
		assert!(has(&errors, message), "{body} {errors:?}");
	}
}
