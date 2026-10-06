mod common;

use std::path::PathBuf;

use partscript::lang::{join_continued, splitlines};
use partscript::{BuildOptions, Host, Project};

fn root() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn examples_check_and_build_clean() {
	let mut project = Project::from_paths(&[root().join("examples")], Host::default()).unwrap();
	assert!(project.errors().is_empty());
	let report = project.check();
	assert!(report.errors.is_empty() && report.warnings.is_empty(), "{:?} {:?}", report.errors, report.warnings);
	for prop in project.props() {
		let built = project.build(&prop.id, &BuildOptions::default()).unwrap();
		assert!(built.triangles() > 0 && built.warnings.is_empty(), "{} {:?}", prop.id, built.warnings);
	}
}

fn joined(text: &str) -> String {
	join_continued(&splitlines(text)).into_iter().flatten().collect::<Vec<_>>().join("\n")
}

/// Every PartScript block in the README is lines of an example file, in order (it may leave lines out).
#[test]
fn readme_snippets_come_from_the_examples() {
	let mut files: Vec<PathBuf> = std::fs::read_dir(root().join("examples")).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "parts")).collect();
	files.sort();
	let text = files.iter().map(|p| joined(&std::fs::read_to_string(p).unwrap())).collect::<Vec<_>>().join("\n");
	let lines: Vec<&str> = text.lines().collect();
	let readme = std::fs::read_to_string(root().join("README.md")).unwrap();
	let mut blocks = Vec::new();
	let mut current: Option<(String, Vec<&str>)> = None;
	for line in readme.lines() {
		if let Some(lang) = line.strip_prefix("```") {
			match current.take() {
				Some((lang, body)) => {
					let body = body.join("\n");
					if lang.is_empty() && (body.trim_start().starts_with("prop") || body.trim_start().starts_with("def")) {
						blocks.push(body);
					}
				}
				None => current = Some((lang.to_string(), Vec::new())),
			}
		} else if let Some((_, body)) = &mut current {
			body.push(line);
		}
	}
	assert!(blocks.len() >= 4);
	for block in blocks {
		let mut at = 0;
		for line in joined(block.trim_matches('\n')).lines() {
			if line.trim().is_empty() {
				continue;
			}
			let found = lines[at..].iter().position(|l| *l == line);
			assert!(found.is_some(), "README line not in the examples (in order): {line:?}");
			at += found.unwrap() + 1;
		}
	}
}
