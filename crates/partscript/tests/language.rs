mod common;

use common::*;
use kitlib::json::Json;
use partscript::lang::{colour_key, split_statements, split_top, strip_comment};
use partscript::value::{Env, Value};
use partscript::{evaluate, parse, Host, Program, Project};

fn env(items: &[(&str, Value)]) -> Env {
	items.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

#[test]
fn numbers_expressions_and_names() {
	assert_eq!(evaluate("1.5", &Env::new()).unwrap(), 1.5);
	assert!((evaluate("w/2+.1", &env(&[("w", Value::Num(1.2))])).unwrap() - 0.7).abs() < 1e-9);
	assert!((evaluate("sin(30)*2", &Env::new()).unwrap() - 1.0).abs() < 1e-9);
	assert_eq!(evaluate("max(a,b)", &env(&[("a", Value::Int(1)), ("b", Value::str("a+2"))])).unwrap(), 3.0);
	assert!(evaluate("nope*2", &Env::new()).is_err());
	assert!(evaluate("__import__('os')", &Env::new()).is_err());
}

#[test]
fn inverse_trig_answers_in_degrees() {
	assert!((evaluate("atan(1.5/6)", &Env::new()).unwrap() - 14.036).abs() < 1e-3);
	assert!((evaluate("asin(.5)", &Env::new()).unwrap() - 30.0).abs() < 1e-9);
}

#[test]
fn split_top_respects_parentheses() {
	assert_eq!(split_top("max(1,2),0,w/2", ','), vec!["max(1,2)", "0", "w/2"]);
}

#[test]
fn statements_blocks_and_placeholders() {
	assert_eq!(split_statements("at 0,0,1 mx { b 0 1 wood ; c 0 .1 1 wood }"), vec!["at 0,0,1 mx {", "b 0 1 wood", "c 0 .1 1 wood", "}"]);
	// {v} is a variant placeholder, not a block.
	assert_eq!(split_statements("prop cone_{c} for c=a,b ; b 0 1 #{c}/plastic"), vec!["prop cone_{c} for c=a,b", "b 0 1 #{c}/plastic"]);
	assert_eq!(strip_comment("b 0 1 #aa3322 # a red box"), "b 0 1 #aa3322 ");
}

#[test]
fn copies_count_triangles() {
	let report = check(
		"
		prop legs_test \"Legs\"
		  b .4,.3,~ .05,.05,.7 wood mx my
		  c 0,0,.5 .1 .2 wood s=8 *3@.3,0,0
		  b 0,0,0 .1 wood *2x3@.2,.2
		",
	);
	assert!(report.errors.is_empty(), "{:?}", report.errors);
	// 4 boxes, 3 cylinders of 8 sides (16 + 12 cap tris), a 2x3 grid of boxes.
	assert_eq!(report.props[0].triangles, 4 * 12 + 3 * 28 + 6 * 12);
}

#[test]
fn variants_expand_every_combination() {
	let report = check(
		"
		prop crate_{c}_{s} \"Crate {c}\" for c=red,blue for s=small,big
		  b 0,0,~ .5 #aa3322
		",
	);
	let mut ids: Vec<String> = report.props.iter().map(|p| p.id.clone()).collect();
	ids.sort();
	assert_eq!(ids, vec!["ld_crate_blue_big", "ld_crate_blue_small", "ld_crate_red_big", "ld_crate_red_small"]);
}

#[test]
fn defs_params_and_std_parts() {
	let report = check(
		"
		def plank len=1 m=wood
		  b 0,0,~ len,.2,.03 m
		prop shelf_test \"Shelf\"
		  use plank len=2 m=#ffffff/paint
		  use table 0,0,0 w=1 top=#553322/wood
		  use crate 1,0,0 fill=none
		",
	);
	assert!(report.errors.is_empty(), "{:?}", report.errors);
	let bad = check(
		"
		prop use_test
		  use table color=red
		",
	);
	assert!(has(&bad.errors, "no parameter 'color'"), "{:?}", bad.errors);
}

#[test]
fn reserved_parameter_and_duplicate_options() {
	let report = check(
		"
		def wheelish r=.3
		  c 0,0,0 r .1 wood
		prop dup_opts
		  b 0 1 wood r=0,0,1 r=0,0,2
		",
	);
	assert!(has(&report.errors, "taken by use"), "{:?}", report.errors);
	assert!(has(&report.errors, "r= given twice"), "{:?}", report.errors);
}

#[test]
fn a_bad_line_does_not_sink_the_prop() {
	let report = check(
		"
		prop resilient \"Resilient\"
		  b 0 1 wood
		  frobnicate 1 2 3
		  b 0,0,1 1 wood
		",
	);
	assert_eq!(report.props.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), vec!["ld_resilient"]);
	assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
	assert!(report.errors[0].contains("unknown statement 'frobnicate'"));
}

#[test]
fn materials_colours_and_unknowns() {
	let report = check(
		"
		prop mats_test
		  b 0 1 #aa3322/metal
		  b 0 1 wood|steel_dark
		  b 0 1 #aa3322/velvet
		  b 0 1 unobtainium
		",
	);
	let errors = report.errors.join(" ");
	assert!(errors.contains("velvet"));
	assert!(errors.contains("unobtainium"));
	assert_eq!(report.errors.len(), 2, "{:?}", report.errors);
	assert_eq!(colour_key("ld", "#AA3322/metal").unwrap().unwrap(), ("ld_xaa3322_metal".into(), "aa3322".into(), "metal".into()));
}

#[test]
fn uses_of_missing_parts_fail() {
	let report = check(
		"
		prop use_missing
		  use nothing_called_this
		",
	);
	assert!(has(&report.errors, "nothing_called_this"));
}

#[test]
fn styles_and_dressing() {
	let report = check(
		"
		prop fruit_stall \"Stall\"
		  use stall
		style test_market exterior=1
		  wall fruit_stall repeat=1,2 hero=1 gap=1.2
		  decals grime crack
		dressing clutter fruit_stall:0.4 missing_piece
		",
	);
	assert!(report.styles.contains(&"test_market".to_string()));
	assert_eq!(report.errors, vec!["dressing clutter: no piece 'missing_piece'"]);
	let mut program = Program::default();
	parse("style s\n  wall a repeat=1,3 hero=1 front=b\n", "<t>", &mut program);
	let want = Json::parse(r#"[{"piece": "a", "repeat": [1, 3], "hero": true, "front": "b"}]"#).unwrap();
	assert_eq!(program.styles.get("s").unwrap().get("wall").unwrap(), &want);
}

#[test]
fn std_parts_check_clean() {
	let project = Project::new(vec![], Host::default(), true, None);
	assert!(project.errors().is_empty());
	assert!(project.check().errors.is_empty());
}

#[test]
fn an_unindented_set_is_file_scope() {
	let report = check(
		"
		prop st_a \"A\"
		  b 0,0,0 .1 wood
		set k=3
		prop st_b \"B\"
		  b 0,0,0 .1 wood *(k)@.2,0,0
		",
	);
	assert!(report.errors.is_empty(), "{:?}", report.errors);
	assert_eq!(report.props.iter().map(|p| p.triangles).collect::<Vec<_>>(), vec![12, 36]);
}

#[test]
fn hang_bends_a_row_not_a_grid() {
	assert!(check(
		"
		prop hg_a \"Bunting\"
		  b 0,0,2 .1 wood *9@.4,0,0 hang=.25
		"
	)
	.errors
	.is_empty());
	let errors = check(
		"
		prop hg_b \"Grid\"
		  b 0,0,2 .1 wood *3x3@.4,.4 hang=.25
		",
	)
	.errors;
	assert!(has(&errors, "hang="), "{errors:?}");
}

#[test]
fn placement_options_are_checked_before_building() {
	let errors = check(
		"
		prop ro_a \"Ramp\"
		  b 0,0,0 1 wood r=0,nope(1),0
		  b 0,0,0 1 wood r=0,atan(.25),0
		",
	)
	.errors;
	assert_eq!(errors.len(), 1, "{errors:?}");
	assert!(errors[0].contains("nope"));
}

#[test]
fn sign_textures_are_powers_of_two() {
	let errors = check(
		"
		prop sg_a \"Sign\"
		  sign 0,0,1 1 .8 \"HI\" tex=128x96
		  sign 0,0,2 1 .8 \"HO\" tex=128x128
		",
	)
	.errors;
	assert_eq!(errors.len(), 1, "{errors:?}");
	assert!(errors[0].contains("powers of two"));
}

#[test]
fn trim_needs_a_decal_sheet() {
	let errors = check(
		"
		prop tr_a \"Trim\"
		  trim 0,-.01,1 .2 .2 knob
		",
	)
	.errors;
	assert!(has(&errors, "no decal sheets"), "{errors:?}");
}
