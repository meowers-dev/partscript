//! The documentation: every example builds, the reference names every statement and option, links lead somewhere.
mod common;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use partscript::check::{shape_options, COMMON_OPTIONS};
use partscript::lang::{alias, SHAPES};
use partscript::{Host, Project};

fn root() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn pages(dir: &Path) -> Vec<PathBuf> {
	let mut out = Vec::new();
	for entry in std::fs::read_dir(dir).unwrap() {
		let path = entry.unwrap().path();
		if path.is_dir() {
			out.extend(pages(&path));
		} else if path.extension().is_some_and(|e| e == "md") {
			out.push(path);
		}
	}
	out.sort();
	out
}

/// The ```parts blocks of a page.
pub fn parts_blocks(text: &str) -> Vec<String> {
	let mut out = Vec::new();
	let mut current: Option<Vec<&str>> = None;
	for line in text.lines() {
		if current.is_none() && line.starts_with("```parts") {
			current = Some(Vec::new());
		} else if line.starts_with("```") && current.is_some() {
			out.push(current.take().unwrap().join("\n") + "\n");
		} else if let Some(body) = &mut current {
			body.push(line);
		}
	}
	out
}

#[test]
fn every_example_in_the_docs_checks_and_builds() {
	let docs = root().join("docs");
	let mut examples: Vec<PathBuf> = std::fs::read_dir(root().join("examples")).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "parts")).collect();
	examples.sort();
	let sources: Vec<(String, String)> = examples.iter().map(|p| (p.to_string_lossy().to_string(), std::fs::read_to_string(p).unwrap())).collect();
	let mut count = 0;
	for page in pages(&docs) {
		let name = page.strip_prefix(&docs).unwrap().to_string_lossy().to_string();
		for code in parts_blocks(&std::fs::read_to_string(&page).unwrap()) {
			if !code.lines().any(|l| ["prop ", "def ", "building ", "kit ", "set "].iter().any(|w| l.starts_with(w))) {
				continue;
			}
			count += 1;
			let mut src = sources.clone();
			src.push((name.clone(), code.clone()));
			let mut project = Project::new(src, Host::default(), true, None);
			let errors: Vec<String> = project.errors().into_iter().filter(|e| e.contains(&name)).collect();
			assert!(errors.is_empty(), "{name}: {errors:?}");
			let report: Vec<String> = project.check().errors.into_iter().filter(|e| e.contains(&name)).collect();
			assert!(report.is_empty(), "{name}: {report:?}");
			for prop in project.props().into_iter().filter(|p| p.file == name) {
				assert!(project.build(&prop.id, &common::no_glb()).unwrap().triangles() > 0, "{name} {}", prop.id);
			}
		}
	}
	assert!(count > 50);
}

#[test]
fn the_reference_names_every_statement_and_option() {
	let text = std::fs::read_to_string(root().join("docs/reference/statements.md")).unwrap();
	let words: HashSet<&str> = text.split(|c: char| !(c.is_ascii_lowercase() || c == '_')).filter(|w| !w.is_empty()).collect();
	let long = |op: &str| -> String {
		for name in ["box", "bevel_box", "box_between", "cylinder", "sphere", "panel", "group", "extrude", "label", "decal", "arch_wall"] {
			if alias(name) == op && name.len() > op.len() {
				return name.to_string();
			}
		}
		op.to_string()
	};
	let mut missing: Vec<String> = SHAPES.iter().filter(|op| !op.is_empty()).filter(|op| !words.contains(long(op).as_str()) && !words.contains(**op)).map(|op| long(op)).collect();
	let option = |k: &str| -> String {
		match k {
			"r" => "turn",
			"s" => "sides",
			"rt" => "top_radius",
			"ax" => "axis",
			"jit" => "jitter",
			"capm" => "cap_mat",
			"index" => "as",
			other => other,
		}
		.to_string()
	};
	let ops = ["b", "bb", "bx", "c", "cone", "sph", "tube", "wedge", "lathe", "ext", "pipe", "sweep", "face", "pan", "trim", "vault", "archwall", "sign", "torus", "frame", "at", "chain", "link", "join", "row", "terrain"];
	for keys in ops.iter().map(|op| shape_options(op).unwrap()).chain(std::iter::once(&COMMON_OPTIONS[..])) {
		missing.extend(keys.iter().filter(|k| **k != "printed").map(|k| option(k)).filter(|k| !words.contains(k.as_str())));
	}
	missing.sort();
	missing.dedup();
	assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn links_between_pages_lead_somewhere() {
	for page in pages(&root().join("docs")) {
		let text = std::fs::read_to_string(&page).unwrap();
		let mut rest = text.as_str();
		while let Some(i) = rest.find("](") {
			rest = &rest[i + 2..];
			let end = rest.find(')').unwrap_or(rest.len());
			let target = &rest[..end];
			let file = target.split('#').next().unwrap();
			if file.ends_with(".md") && !file.contains(':') {
				assert!(page.parent().unwrap().join(file).exists(), "{} links to {file}", page.display());
			}
		}
	}
}
