//! Build speed. Every std part as a prop, the golden shapes and the examples, built N rounds.
//!
//!   cargo run --release -p partscript --example bench               # cold (textures made) and warm (textures cached) timings
//!   cargo run --release -p partscript --example bench -- --rounds 5

use std::path::{Path, PathBuf};
use std::time::Instant;

use partscript::{BuildOptions, Host, Project};

fn root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn workload() -> Vec<(String, String)> {
	let program = partscript::load_program(&[], true, None);
	let mut names: Vec<&str> = program.macros.keys().collect();
	names.sort();
	let std_props: Vec<String> = names.iter().map(|n| format!("prop std_{n} \"{n}\"\n  use {n}\n")).collect();
	let mut sources = vec![
		("std_props.parts".to_string(), std_props.join("\n")),
		("shapes.parts".to_string(), std::fs::read_to_string(root().join("tests/golden/shapes.parts")).unwrap()),
	];
	let mut examples: Vec<PathBuf> = std::fs::read_dir(root().join("examples")).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "parts")).collect();
	examples.sort();
	for path in examples {
		sources.push((path.to_string_lossy().to_string(), std::fs::read_to_string(&path).unwrap()));  // the full path: imports are read beside it
	}
	sources
}

/// (seconds, props built, triangles)
fn run(cache_dir: Option<PathBuf>, rounds: usize) -> (f64, usize, usize) {
	let started = Instant::now();
	let (mut count, mut triangles) = (0, 0);
	for _ in 0..rounds {
		let mut project = Project::new(workload(), Host::with_cache(cache_dir.clone()), true, None);
		for prop in project.props() {
			let built = project.build(&prop.id, &BuildOptions::default()).unwrap();
			count += 1;
			triangles += built.triangles();
		}
	}
	(started.elapsed().as_secs_f64(), count, triangles)
}

fn main() {
	let args: Vec<String> = std::env::args().collect();
	let rounds = args.iter().position(|a| a == "--rounds").and_then(|k| args.get(k + 1)).map_or(3, |r| r.parse().expect("--rounds N"));
	let cache = std::env::temp_dir().join(format!("partscript-bench-{}", std::process::id()));
	let (cold, count, triangles) = run(None, 1);
	println!("cold (no texture cache): {count} props, {triangles} triangles, {cold:.3} s, {:.2} ms/prop", cold / count as f64 * 1000.0);
	run(Some(cache.clone()), 1);
	let (warm, count, _) = run(Some(cache.clone()), rounds);
	println!("warm (textures cached):  {count} props, {warm:.3} s, {:.2} ms/prop", warm / count as f64 * 1000.0);
	let _ = std::fs::remove_dir_all(cache);
}
