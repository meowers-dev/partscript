//! What the ported tests share: dedented text, quick projects and builds, and box measurements.
#![allow(dead_code)]

use kitlib::json::Json;
use partscript::{BasicProvider, BuildOptions, Built, Host, Project, Report};

/// textwrap.dedent: the margin every non-blank line shares taken off; blank lines left empty.
pub fn dedent(text: &str) -> String {
	let lines: Vec<&str> = text.split('\n').collect();
	let margin = lines
		.iter()
		.filter(|l| !l.trim().is_empty())
		.map(|l| l.chars().take_while(|c| *c == ' ' || *c == '\t').collect::<String>())
		.reduce(|a, b| a.chars().zip(b.chars()).take_while(|(x, y)| x == y).map(|(x, _)| x).collect())
		.unwrap_or_default();
	lines
		.iter()
		.map(|l| if l.trim().is_empty() { "" } else { l.strip_prefix(margin.as_str()).unwrap_or(l) })
		.collect::<Vec<_>>()
		.join("\n")
}

pub fn host_with_prefix(prefix: &str) -> Host {
	Host::new(Box::new(BasicProvider::default()), prefix, None)
}

pub fn project(text: &str, file: &str) -> Project {
	Project::from_text(&dedent(text), file, Host::default())
}

pub fn check(text: &str) -> Report {
	Project::from_text(&dedent(text), "<t>", host_with_prefix("ld")).check()
}

pub fn no_glb() -> BuildOptions {
	BuildOptions { glb: false, ..Default::default() }
}

/// A project of text that checks clean, and one prop of it built (no .glb).
pub fn build(text: &str, name: &str) -> (Project, Built) {
	let mut p = Project::from_text(&dedent(text), "v.parts", Host::default());
	let report = p.check();
	assert!(p.errors().is_empty() && report.errors.is_empty(), "{:?} {:?}", p.errors(), report.errors);
	let built = p.build(name, &no_glb()).unwrap();
	(p, built)
}

pub fn round(x: f64, digits: usize) -> f64 {
	kitlib::py::round_to(x, digits)
}

fn measure(faces: &[kitlib::geom::Face]) -> Vec<(f64, f64)> {
	faces
		.chunks(6)
		.map(|six| {
			let pts: Vec<[f64; 3]> = six.iter().flat_map(|f| f.points.iter().copied()).collect();
			let x = kitlib::py::sum(pts.iter().map(|p| p[0])) / pts.len() as f64;
			let lo = pts.iter().map(|p| p[2]).fold(f64::INFINITY, f64::min);
			let hi = pts.iter().map(|p| p[2]).fold(f64::NEG_INFINITY, f64::max);
			(round(x, 4), round(hi - lo, 4))
		})
		.collect()
}

/// (x centre, height) of each box in the first part (6 faces each, in order).
pub fn boxes(built: &Built) -> Vec<(f64, f64)> {
	measure(&built.parts[0].faces)
}

/// (x centre, height) of the last count boxes of the first part.
pub fn boxes_last(built: &Built, count: usize) -> Vec<(f64, f64)> {
	let faces = &built.parts[0].faces;
	measure(&faces[faces.len() - 6 * count..])
}

/// The JSON chunk of a .glb, parsed, and its binary chunk.
pub fn read_glb(data: &[u8]) -> (Json, Vec<u8>) {
	assert_eq!(&data[..4], b"glTF");
	assert_eq!(u32::from_le_bytes(data[4..8].try_into().unwrap()), 2);
	assert_eq!(u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize, data.len());
	let json_len = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
	assert_eq!(&data[16..20], b"JSON");
	let document = Json::parse(std::str::from_utf8(&data[20..20 + json_len]).unwrap()).unwrap();
	let bin_len = u32::from_le_bytes(data[20 + json_len..24 + json_len].try_into().unwrap()) as usize;
	assert_eq!(&data[24 + json_len..28 + json_len], b"BIN\0");
	(document, data[28 + json_len..28 + json_len + bin_len].to_vec())
}

pub fn has(errors: &[String], needle: &str) -> bool {
	errors.iter().any(|e| e.contains(needle))
}

/// A fresh scratch folder under the system temp folder.
pub fn scratch(name: &str) -> std::path::PathBuf {
	static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
	let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
	let dir = std::env::temp_dir().join(format!("partscript-test-{}-{name}-{n}", std::process::id()));
	let _ = std::fs::remove_dir_all(&dir);
	std::fs::create_dir_all(&dir).unwrap();
	dir
}

/// Writes files (dedented) under root.
pub fn write(root: &std::path::Path, files: &[(&str, &str)]) {
	for (name, text) in files {
		let path = root.join(name);
		std::fs::create_dir_all(path.parent().unwrap()).unwrap();
		std::fs::write(path, dedent(text)).unwrap();
	}
}

pub fn sources(files: &[(&str, &str)]) -> Project {
	Project::new(files.iter().map(|(n, t)| (n.to_string(), t.to_string())).collect(), Host::default(), true, None)
}

pub fn materials(built: &Built) -> std::collections::HashSet<String> {
	built.parts.iter().flat_map(|p| p.faces.iter().map(|f| f.material.to_string())).collect()
}

/// (min, max) corners of the faces first..last across all parts, rounded to 4 places.
pub fn bounds(built: &Built, first: usize, last: Option<usize>) -> (Vec<f64>, Vec<f64>) {
	let faces: Vec<&kitlib::geom::Face> = built.parts.iter().flat_map(|p| p.faces.iter()).collect();
	let faces = &faces[first..last.unwrap_or(faces.len())];
	let points: Vec<[f64; 3]> = faces.iter().flat_map(|f| f.points.iter().copied()).collect();
	let lo = (0..3).map(|k| round(points.iter().map(|p| p[k]).fold(f64::INFINITY, f64::min), 4)).collect();
	let hi = (0..3).map(|k| round(points.iter().map(|p| p[k]).fold(f64::NEG_INFINITY, f64::max), 4)).collect();
	(lo, hi)
}

/// A demo prop of body (after defs), checked clean and built.
pub fn demo(body: &str, defs: &str) -> Built {
	let text = format!("{}prop demo \"Demo\"\n{}", dedent(defs), dedent(body));
	let mut p = Project::from_text(&text, "t.parts", Host::default());
	let report = p.check();
	assert!(p.errors().is_empty() && report.errors.is_empty(), "{:?} {:?}", p.errors(), report.errors);
	p.build("demo", &no_glb()).unwrap()
}

pub fn close(a: f64, b: f64) -> bool {
	(a - b).abs() < 1e-6
}
