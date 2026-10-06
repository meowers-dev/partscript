//! The readable form parses to the same statements as the terse one.
mod common;

use std::rc::Rc;

use partscript::lang::{parse_statement, Stmt};
use partscript::{fmt, parse, BuildOptions, Host, Program, Project};

type Shape = (String, Vec<String>, Vec<(String, String)>, Vec<String>);

fn shape(s: &Stmt) -> Shape {
	let mut opts: Vec<(String, String)> = s.opts.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
	opts.sort();
	(s.op.clone(), s.args.clone(), opts, s.mods.clone())
}

#[test]
fn both_forms_parse_the_same() {
	for (terse, readable) in [
		("b 0,0,~ .4,.3,.2 wood", "box at=0,0,on size=.4,.3,.2 mat=wood"),
		("b 0,0,~.8 .4 wood r=0,0,45", "box at=0,0,on(.8) size=.4 material=wood turn=0,0,45"),
		("bb 0,0,~ .4,.4,.05 .015 rubber", "bevel_box at=0,0,on size=.4,.4,.05 bevel=.015 mat=rubber"),
		("c 0,0,~ .16 .64 #e25a1a/plastic rt=.03 s=12", "cylinder at=0,0,on radius=.16 height=.64 mat=#e25a1a/plastic top_radius=.03 sides=12"),
		("sph 0,0,1 .2 brass s=6 rings=3", "sphere at=0,0,1 radius=.2 mat=brass sides=6 rings=3"),
		("sign 0,-.05,2 1.6 .3 \"THE HOPE\" sub=\"FREE HOUSE\"", "sign at=0,-.05,2 width=1.6 height=.3 text=\"THE HOPE\" sub=\"FREE HOUSE\""),
		("ext 0,0,0 1.6 paint -1:0 1:0 0:1 ax=y", "extrude at=0,0,0 width=1.6 mat=paint -1:0 1:0 0:1 axis=y"),
		("ext 0,0,0 1.6 paint -1:0 1:0 0:1 ax=y", "extrude at=0,0,0 width=1.6 mat=paint points=\"-1:0 1:0 0:1\" axis=y"),
		("pipe steel_dark .03 -1,0,1 1,0,1", "pipe mat=steel_dark radius=.03 -1,0,1 1,0,1"),
		("use table 0,0,0 w=1 top=wood", "use table at=0,0,0 w=1 top=wood"),
		("use flower 0,0,.12 *40~1.6,.75,.09 s=rand(.8,1.2)", "use flower at=0,0,.12 scatter 40 over 1.6,.75 apart .09 scale=rand(.8,1.2)"),
		("use rock *5~1.2", "use rock scatter 5 within 1.2"),
		("use rock *5~1.2,0,.3", "use rock scatter 5 within 1.2 apart .3"),
		("b 0,0,0 .1 wood *5@.3,0,0 mx my", "box at=0,0,0 size=.1 mat=wood repeat 5 every .3,0,0 mirror xy"),
		("b 0,0,0 .1 wood *(n)@.3,0,0", "box at=0,0,0 size=.1 mat=wood repeat n every .3,0,0"),
		("b 0,0,0 .1 wood *4x3@.2,.2", "box at=0,0,0 size=.1 mat=wood grid 4x3 every .2,.2"),
		("b .14,0,3.3 .025 wood *6%(360/6)", "box at=.14,0,3.3 size=.025 mat=wood ring 6"),
		("b .14,0,3.3 .025 wood *6%60", "box at=.14,0,3.3 size=.025 mat=wood ring 6 step 60"),
		("at 0,0,1 r=0,0,45", "group at=0,0,1 turn=0,0,45"),
	] {
		assert_eq!(shape(&parse_statement(terse, "t", 1).unwrap()), shape(&parse_statement(readable, "t", 1).unwrap()), "{readable}");
	}
}

#[test]
fn readable_mistakes_say_what_to_do() {
	for (text, message) in [
		("box 0,0,0 .4 wood at=0,0,0", "also given by position"),
		("box at=0,0,0 mat=wood", "size= is missing"),
		("box at=0 size=.1 mat=wood turn=0,0,1 r=0,0,2", "same option"),
		("box at=0 size=.1 mat=wood sides=6", "sides= is for round shapes"),
		("box at=0 size=.1 mat=wood mirror q", "mirror x"),
	] {
		let error = parse_statement(text, "t", 1).err().unwrap();
		assert!(error.message.contains(message), "{text}: {}", error.message);
	}
}

#[test]
fn a_readable_prop_builds_like_the_terse_one() {
	let mut terse = Project::from_text("prop cone \"Cone\"\n  c 0,0,~ .16 .64 #e25a1a/plastic rt=.03 s=12\n  bb 0,0,~ .42,.42,.05 .015 rubber\n", "<text>", Host::default());
	let mut readable = Project::from_text(
		"prop cone \"Cone\"\n  cylinder at=0,0,on radius=.16 height=.64 mat=#e25a1a/plastic top_radius=.03 sides=12\n  bevel_box at=0,0,on size=.42,.42,.05 bevel=.015 mat=rubber\n",
		"<text>",
		Host::default(),
	);
	assert_eq!(terse.build("cone", &BuildOptions::default()).unwrap().glb, readable.build("cone", &BuildOptions::default()).unwrap().glb);
}

fn walk(body: &[Rc<Stmt>], out: &mut Vec<Shape>) {
	for s in body {
		out.push(shape(s));
		if let Some(block) = &s.block {
			walk(block, out);
		}
	}
}

fn statements(program: &Program) -> Vec<(String, Vec<Shape>)> {
	let mut out = Vec::new();
	for prop in &program.props {
		let mut body = Vec::new();
		walk(&prop.body, &mut body);
		out.push((prop.name.clone(), body));
	}
	let mut macros: Vec<(&str, &Rc<partscript::lang::Macro>)> = program.macros.iter().collect();
	macros.sort_by_key(|(n, _)| *n);
	for (name, m) in macros {
		let mut body = Vec::new();
		walk(&m.body, &mut body);
		out.push((name.to_string(), body));
	}
	out
}

fn parsed(text: &str, name: &str) -> Program {
	let mut program = Program::default();
	parse(text, name, &mut program);
	program
}

#[test]
fn fmt_round_trips_every_example() {
	let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
	let mut files: Vec<(String, String)> = std::fs::read_dir(root.join("examples"))
		.unwrap()
		.map(|e| e.unwrap().path())
		.filter(|p| p.extension().is_some_and(|e| e == "parts"))
		.map(|p| (p.file_name().unwrap().to_string_lossy().to_string(), std::fs::read_to_string(&p).unwrap()))
		.collect();
	files.sort();
	files.push(("std.parts".into(), partscript::project::STD.to_string()));
	files.push(("shapes.parts".into(), std::fs::read_to_string(root.join("tests/golden/shapes.parts")).unwrap()));
	for (name, text) in files {
		let original = statements(&parsed(&text, &name));
		let long = fmt::readable(&text);
		assert!(parsed(&long, &name).errors.is_empty(), "{name}: {:?}", parsed(&long, &name).errors);
		assert_eq!(statements(&parsed(&long, &name)), original, "{name}");
		assert_eq!(statements(&parsed(&fmt::terse(&long), &name)), original, "{name}");
		assert_eq!(fmt::readable(&long), long);
	}
}
