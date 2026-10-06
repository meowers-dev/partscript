//! The preview site's models: every example and std part as site/models/<id>.glb + index.json, and the
//! playground's examples. Each model is step-tagged (TEXCOORD_1.x = the source statement of each face),
//! and index.json gives each prop its source, the line of every step and its "built from" tree, so the
//! page can light up what any line or any part it uses adds.

use std::collections::HashMap;
use std::path::PathBuf;

use kitlib::json::Json;
use partscript::project::STD;
use partscript::{BuildOptions, Host, Project};

use crate::root;

const ORDER: [&str; 13] = [
	"ruins.parts", "showpieces.parts", "layout.parts", "tavern.parts", "fairground.parts", "buildings.parts", "paths.parts", "graveyard.parts", "garden.parts",
	"street.parts", "market.parts", "cafe.parts", "library.parts",
];

fn group(file: &str) -> &str {
	match file {
		"fairground.parts" => "Fairground",
		"ruins.parts" => "Ruins (generators)",
		"showpieces.parts" => "Showpieces (detail tools)",
		"layout.parts" => "Layout (placing against names)",
		"tavern.parts" => "Tavern (imported libraries)",
		"buildings.parts" => "Buildings",
		"paths.parts" => "Paths and fences (snapping)",
		"graveyard.parts" => "Graveyard (procedural)",
		"garden.parts" => "Garden (variety)",
		"street.parts" => "Street",
		"market.parts" => "Market",
		"cafe.parts" => "Cafe",
		"library.parts" => "Library",
		"std parts" => "Standard parts",
		other => other,
	}
}

fn parts_in(dir: &str) -> Vec<PathBuf> {
	let mut files: Vec<PathBuf> = std::fs::read_dir(root().join(dir)).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "parts")).collect();
	files.sort();
	files
}

/// The library the examples import ("../library/x.parts" from an example's folder).
pub fn library() -> Vec<(String, String)> {
	parts_in("library").into_iter().map(|p| (format!("../library/{}", p.file_name().unwrap().to_string_lossy()), std::fs::read_to_string(&p).unwrap())).collect()
}

/// A prop's or def's lines in its file, from its header to the next unindented line.
fn block(text: &str, line: usize) -> String {
	let lines: Vec<&str> = partscript::lang::splitlines(text).into_iter().skip(line - 1).collect();
	let mut out = vec![lines[0]];
	for row in &lines[1..] {
		if !row.is_empty() && !row.starts_with(char::is_whitespace) {
			if row.trim() == "end" {
				out.push(row);
			}
			break;
		}
		out.push(row);
	}
	while out.last().is_some_and(|l| l.trim().is_empty()) {
		out.pop();
	}
	out.join("\n")
}

pub fn build() -> Result<(), String> {
	let std_program = partscript::load_program(&[], true, None);
	let mut std_names: Vec<&str> = std_program.macros.keys().collect();
	std_names.sort();
	let mut examples = parts_in("examples");
	examples.sort_by_key(|p| ORDER.iter().position(|o| *o == p.file_name().unwrap().to_string_lossy()).unwrap_or(9));
	let mut sources: Vec<(String, String)> = examples.iter().map(|p| (p.file_name().unwrap().to_string_lossy().to_string(), std::fs::read_to_string(p).unwrap())).collect();
	sources.push(("std parts".into(), std_names.iter().map(|n| format!("prop std_{n} \"{n}\"\n  use {n}\n")).collect::<Vec<_>>().join("\n")));
	let library: HashMap<String, String> = library().into_iter().collect();
	let mut texts: HashMap<String, String> = sources.iter().cloned().collect();
	texts.insert("std.parts".into(), STD.to_string());
	let lib = library.clone();
	let reader = move |target: &str| lib.get(target).map(|t| vec![(target.to_string(), t.clone())]).unwrap_or_default();
	let mut project = Project::new(sources, Host::default(), true, Some(&reader));
	if !project.errors().is_empty() {
		return Err(project.errors().join("\n"));
	}
	for label in &project.program.imported {
		let path = label.split_once(':').map(|(_, p)| p).unwrap_or(label);
		texts.insert(label.clone(), library[path].clone());
	}
	let out = root().join("site/models");
	std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
	for entry in std::fs::read_dir(&out).map_err(|e| e.to_string())? {
		let path = entry.map_err(|e| e.to_string())?.path();
		if path.extension().is_some_and(|e| e == "glb") {
			std::fs::remove_file(path).map_err(|e| e.to_string())?;
		}
	}
	let mut rows = Vec::new();
	for prop in project.props().into_iter().filter(|p| !p.imported) {
		let built = project.build(&prop.id, &BuildOptions { glb: true, steps: true, snaps: true, seed: None }).map_err(|e| e.to_string())?;
		std::fs::write(out.join(format!("{}.glb", built.asset_id)), &built.glb).map_err(|e| e.to_string())?;
		let mut row = Json::dict();
		row.set("id", built.asset_id.as_str())
			.set("title", prop.title.as_str())
			.set("group", group(&prop.file))
			.set("file", prop.file.as_str())
			.set("line", prop.line)
			.set("triangles", built.triangles())
			.set("snaps", built.snaps.len() + built.joints.len())
			.set("ms", (built.seconds * 10000.0).round() / 10.0)
			.set("source", block(&texts[&prop.file], prop.line))
			.set("steps", built.steps.iter().map(|s| s.stmt.line).collect::<Vec<_>>())
			.set("origins", built.origins.iter().map(|o| o.iter().map(|(f, l)| format!("{f}:{l}")).collect::<Vec<_>>().join("/")).collect::<Vec<_>>())
			.set("tree", project.uses(&prop.id));
		rows.push(row);
	}
	// Source of everything a tree can name (defs, std parts, props), for clicking through.
	let mut blocks = Json::dict();
	let entry = |file: &str, line: usize| {
		let mut d = Json::dict();
		d.set("file", file).set("line", line).set("source", block(&texts[file], line));
		d
	};
	for (name, m) in project.program.macros.iter() {
		blocks.set(name, entry(&m.file, m.line));
	}
	for p in &project.program.props {
		blocks.set(&p.name, entry(&p.file, p.line));
	}
	let mut index = Json::dict();
	index.set("props", Json::List(rows.clone())).set("blocks", blocks);
	std::fs::write(out.join("index.json"), index.dumps_indent(1)).map_err(|e| e.to_string())?;
	println!("{} models -> {}", rows.len(), out.display());
	playground(&examples, &library)
}

/// What the playground (play.html) needs besides its WebAssembly: the examples and the library they import.
fn playground(examples: &[PathBuf], library: &HashMap<String, String>) -> Result<(), String> {
	let shelf = root().join("site/examples");
	std::fs::create_dir_all(&shelf).map_err(|e| e.to_string())?;
	for path in examples {
		std::fs::write(shelf.join(path.file_name().unwrap()), std::fs::read_to_string(path).unwrap()).map_err(|e| e.to_string())?;
	}
	let names: Vec<String> = examples.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
	std::fs::write(shelf.join("index.json"), Json::from(names).dumps(false)).map_err(|e| e.to_string())?;
	let mut lib: Vec<(&String, &String)> = library.iter().collect();
	lib.sort();
	let lib = Json::Dict(lib.into_iter().map(|(k, v)| (k.clone(), Json::Str(v.clone()))).collect());
	std::fs::write(shelf.join("library.json"), lib.dumps(false)).map_err(|e| e.to_string())?;
	println!("playground: {} examples (and site/pkg/partscript.wasm from build.sh)", examples.len());
	Ok(())
}
