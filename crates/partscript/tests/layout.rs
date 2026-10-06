//! Readable placement: named shapes, anchors, on=, row/stack and from=/to=.
mod common;

use common::*;
use partscript::{fmt, Host, Project};

#[test]
fn on_sits_a_shape_on_a_named_one() {
	let built = demo(
		"
		  desk = box at=1,0,on size=1.2,.6,.75 mat=wood
		  box on=desk at=.2,.1 size=.1,.1,.2 mat=wood
		",
		"",
	);
	assert_eq!(bounds(&built, 6, None), (vec![1.15, 0.05, 0.75], vec![1.25, 0.15, 0.95]));
}

#[test]
fn anchors_are_numbers_and_points() {
	let built = demo(
		"
		  desk = box at=0,0,on size=1,.5,.8 mat=wood
		  box at=desk.right+.1,desk.y,on(desk) size=.1 mat=wood
		  box at=desk.top_left_front size=.02 mat=wood
		",
		"",
	);
	assert_eq!(bounds(&built, 6, Some(12)), (vec![0.55, -0.05, 0.8], vec![0.65, 0.05, 0.9]));
	assert_eq!(bounds(&built, 12, None), (vec![-0.51, -0.26, 0.79], vec![-0.49, -0.24, 0.81]));
}

#[test]
fn anchors_read_in_the_space_of_the_line_that_asks() {
	let built = demo(
		"
		  desk = box at=2,0,on size=1,1,.8 mat=wood
		  group at=1,0,0 turn=0,0,90 {
		    box at=0,0,on(desk.top) size=.1 mat=wood
		  }
		",
		"",
	);
	let (lo, hi) = bounds(&built, 6, None);
	assert_eq!((lo[2], hi[2]), (0.8, 0.9));
}

#[test]
fn a_def_sees_the_named_shapes_of_the_line_that_uses_it() {
	let built = demo(
		"
		  shelf = box at=0,0,on(1) size=1,.3,.04 mat=wood
		  use vase at=-.3,0,0
		",
		"
		def vase
		  cylinder at=0,0,on(shelf) radius=.05 height=.2 mat=wood
		",
	);
	assert_eq!(bounds(&built, 6, None).0[2], 1.04);
}

#[test]
fn row_sets_copies_end_to_end() {
	let built = demo(
		"
		  row x gap=.1 pack=start {
		    box at=0,0,on size=.2,.2,.2 mat=wood repeat 3
		  }
		",
		"",
	);
	assert_eq!(bounds(&built, 0, None), (vec![0.0, -0.1, 0.0], vec![0.8, 0.1, 0.2]));
}

#[test]
fn row_over_spreads_and_align_lines_up() {
	let built = demo(
		"
		  row x over=2 align=back {
		    box at=0,0,on size=.2,.1,.3 mat=wood
		    box at=0,0,on size=.2,.4,.3 mat=wood
		    box at=0,0,on size=.2,.2,.3 mat=wood
		  }
		",
		"",
	);
	assert_eq!(bounds(&built, 0, None), (vec![-1.0, -0.4, 0.0], vec![1.0, 0.0, 0.3]));
	assert!(close(bounds(&built, 6, Some(12)).0[0], -0.1));
}

#[test]
fn stack_piles_up_from_the_floor() {
	let built = demo(
		"
		  stack {
		    box size=.5,.4,.3 mat=wood repeat 3
		    cylinder radius=.1 height=.2 mat=wood
		  }
		",
		"",
	);
	assert!(close(bounds(&built, 0, None).1[2], 1.1));
}

#[test]
fn named_row_and_things_on_it() {
	let built = demo(
		"
		  books = row x gap=0 {
		    box size=.05,.2,.25 mat=wood repeat 4
		  }
		  box at=books.right+.05,0,on size=.1 mat=wood
		",
		"",
	);
	assert!(close(bounds(&built, 24, None).0[0], 0.1));
}

#[test]
fn beam_between_two_points() {
	let built = demo("  box from=0,0,0 to=3,4,0 size=.1 mat=wood\n  cylinder from=0,0,0 to=0,0,2 radius=.05 mat=wood\n", "");
	let (lo, hi) = bounds(&built, 0, Some(6));
	assert!(hi[0] - lo[0] > 2.9 && hi[1] - lo[1] > 3.9 && close(hi[2] - lo[2], 0.1));
	assert!(close(bounds(&built, 6, None).1[2], 2.0));
}

#[test]
fn use_from_to_lays_a_def_along_and_gives_it_length() {
	let built = demo(
		"  use plank from=1,1,0 to=1,3,0\n",
		"
		def plank
		  box at=length/2,0,0 size=length,.1,.02 mat=wood
		",
	);
	let (lo, hi) = bounds(&built, 0, None);
	assert!(close(lo[1], 1.0) && close(hi[1], 3.0) && close(hi[0] - lo[0], 0.1));
}

#[test]
fn mistakes_say_what_to_do() {
	for (body, message) in [
		("  box on=desk size=.1 mat=wood\n", "no shape of that name"),
		("  desk = box at=0 size=1 mat=wood\n  box at=desk.middle,0,0 size=.1 mat=wood\n", "a named shape gives"),
		("  row q {\n    box at=0 size=.1 mat=wood\n  }\n", "the axis it runs along"),
	] {
		let project = Project::from_text(&format!("prop demo \"Demo\"\n{body}"), "t.parts", Host::default());
		assert!(has(&project.check().errors, message), "{:?}", project.check().errors);
	}
}

#[test]
fn fmt_keeps_the_readable_form() {
	for line in [
		"desk = box at=1,0,on size=1.4,.7,.75 mat=wood",
		"box size=.4 mat=wood",
		"box on=desk at=.2,.1 size=.1 mat=wood",
		"cylinder from=desk.top to=1,1,2 radius=.02 mat=steel_dark",
		"box from=0,0,0 to=1,0,0 size=.03 mat=brass",
		"use lamp on=desk at=.3,.1",
		"stack gap=0 {",
		"books = row x on=desk at=-.3,.2 gap=.004 align=back {",
	] {
		assert_eq!(fmt::readable(&format!("  {line}")).trim(), line);
		assert_eq!(fmt::readable(&fmt::terse(&format!("  {line}"))).trim(), line);
	}
}
