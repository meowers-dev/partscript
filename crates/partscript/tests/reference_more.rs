//! Everything besides geometry against the Python reference: the check report, buildings as data, the
//! built-from trees, the formatter, texture pixels, expressions, label text and numpy's random streams.

use std::collections::HashMap;
use std::path::PathBuf;

use kitlib::json::Json;
use partscript::expr::{evaluate, interpolate};
use partscript::value::{Env, Value};
use partscript::{BasicProvider, Host, Project, Recipe, TextureProvider};

fn root() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> Json {
	let data = std::fs::read(root().join(format!("tests/reference/{name}.json.gz"))).unwrap();
	Json::parse(&String::from_utf8(kitlib::gunzip(&data).unwrap()).unwrap()).unwrap()
}

fn sources(dir: &str) -> Vec<(String, String)> {
	let mut files: Vec<PathBuf> = std::fs::read_dir(root().join(dir)).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == "parts")).collect();
	files.sort();
	files.iter().map(|p| (p.file_name().unwrap().to_string_lossy().to_string(), std::fs::read_to_string(p).unwrap())).collect()
}

fn examples_project() -> Project {
	let std = partscript::load_program(&[], true, None);
	let mut names: Vec<&str> = std.macros.keys().collect();
	names.sort();
	let mut src = sources("examples");
	src.push(("std parts".into(), names.iter().map(|n| format!("prop std_{n} \"{n}\"\n  use {n}\n")).collect::<Vec<_>>().join("\n")));
	let lib: HashMap<String, String> = sources("library").into_iter().map(|(n, t)| (format!("../library/{n}"), t)).collect();
	let reader = move |target: &str| lib.get(target).map(|t| vec![(target.to_string(), t.clone())]).unwrap_or_default();
	Project::new(src, Host::default(), true, Some(&reader))
}

fn json_close(a: &Json, b: &Json, tol: f64) -> bool {
	match (a, b) {
		(Json::Float(_) | Json::Int(_), Json::Float(_) | Json::Int(_)) => (a.as_f64() - b.as_f64()).abs() <= tol,
		(Json::List(x), Json::List(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| json_close(p, q, tol)),
		(Json::Dict(x), Json::Dict(y)) => x.len() == y.len() && x.iter().zip(y).all(|((k1, v1), (k2, v2))| k1 == k2 && json_close(v1, v2, tol)),
		_ => a == b,
	}
}

fn first_difference(a: &Json, b: &Json, path: &str) -> Option<String> {
	match (a, b) {
		(Json::List(x), Json::List(y)) if x.len() == y.len() => x.iter().zip(y).enumerate().find_map(|(i, (p, q))| first_difference(p, q, &format!("{path}[{i}]"))),
		(Json::Dict(x), Json::Dict(y)) if x.len() == y.len() => {
			x.iter().zip(y).find_map(|((k1, v1), (k2, v2))| if k1 != k2 { Some(format!("{path}: key {k1} != {k2}")) } else { first_difference(v1, v2, &format!("{path}.{k1}")) })
		}
		_ if json_close(a, b, 1e-6) => None,
		_ => Some(format!("{path}: {} != {}", a.dumps(true).chars().take(300).collect::<String>(), b.dumps(true).chars().take(300).collect::<String>())),
	}
}

#[test]
fn check_report_matches() {
	let data = fixture("examples");
	let project = examples_project();
	let report = project.check().to_json();
	if let Some(d) = first_difference(&report, data.get("check").unwrap(), "check") {
		panic!("{d}");
	}
}

#[test]
fn props_buildings_and_trees_match() {
	let data = fixture("examples");
	let mut project = examples_project();
	let props: Vec<Json> = project
		.props()
		.iter()
		.map(|p| {
			let mut d = Json::dict();
			d.set("id", p.id.as_str()).set("title", p.title.as_str()).set("subcategory", p.subcategory.as_str()).set("kind", p.kind.as_str()).set("file", p.file.as_str()).set("line", p.line).set("imported", p.imported);
			d
		})
		.collect();
	if let Some(d) = first_difference(&Json::List(props), data.get("props").unwrap(), "props") {
		panic!("{d}");
	}
	for (id, want) in data.get("buildings").unwrap().as_dict() {
		let got = project.building(id).unwrap();
		if let Some(d) = first_difference(&got, want, id) {
			panic!("{d}");
		}
	}
	for (id, want) in data.get("uses").unwrap().as_dict() {
		let got = project.uses(id);
		if let Some(d) = first_difference(&got, want, id) {
			panic!("{d}");
		}
	}
}

#[test]
fn formatter_matches() {
	let data = fixture("fmt");
	let mut problems = Vec::new();
	for row in data.as_list() {
		let (name, text) = (row.get("name").unwrap().as_str(), row.get("text").unwrap().as_str());
		for (key, got) in [("readable", partscript::fmt::readable(text)), ("terse", partscript::fmt::terse(text))] {
			let want = row.get(key).unwrap().as_str();
			if got != want {
				let line = got.lines().zip(want.lines()).position(|(a, b)| a != b).unwrap_or(0);
				problems.push(format!("{name} {key} line {}: {:?} != {:?}", line + 1, got.lines().nth(line), want.lines().nth(line)));
			}
		}
	}
	assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn textures_match_pixel_for_pixel() {
	let data = fixture("textures");
	let provider = BasicProvider::default();
	let mut problems = Vec::new();
	let mut total = 0;
	for (name, row) in data.as_dict() {
		let recipe_json = row.get("recipe").unwrap();
		let recipe = match recipe_json.idx(0).as_str() {
			"surface" => Recipe::Surface { finish: recipe_json.idx(1).as_str().into(), hex: recipe_json.idx(2).as_str().into() },
			"sign" => Recipe::Sign(recipe_json.idx(1).clone()),
			_ => Recipe::Other(recipe_json.clone()),
		};
		let image = provider.make(&recipe).unwrap();
		let shape: Vec<i64> = row.get("shape").unwrap().as_list().iter().map(Json::as_i64).collect();
		if [image.height as i64, image.width as i64, image.channels as i64] != shape[..] {
			problems.push(format!("{name}: shape {}x{}x{} != {shape:?}", image.height, image.width, image.channels));
			continue;
		}
		let want = base64(row.get("pixels").unwrap().as_str());
		total += want.len();
		let differ = image.pixels.iter().zip(&want).filter(|(a, b)| a != b).count();
		if differ > 0 {
			problems.push(format!("{name} ({}): {differ} of {} values differ", recipe_json.dumps(true), want.len()));
		}
	}
	assert!(problems.is_empty(), "{} of {total} values:\n{}", problems.len(), problems.join("\n"));
}

fn base64(text: &str) -> Vec<u8> {
	let table = |c: u8| -> u32 {
		match c {
			b'A'..=b'Z' => (c - b'A') as u32,
			b'a'..=b'z' => (c - b'a' + 26) as u32,
			b'0'..=b'9' => (c - b'0' + 52) as u32,
			b'+' => 62,
			_ => 63,
		}
	};
	let bytes: Vec<u8> = text.bytes().filter(|c| *c != b'=').collect();
	let mut out = Vec::new();
	for chunk in bytes.chunks(4) {
		let mut n = 0u32;
		for (i, c) in chunk.iter().enumerate() {
			n |= table(*c) << (18 - 6 * i);
		}
		for i in 0..chunk.len() - 1 {
			out.push((n >> (16 - 8 * i)) as u8);
		}
	}
	out
}

#[test]
fn expressions_match() {
	let data = fixture("basics");
	let mut env = Env::new();
	env.insert("a".into(), Value::Num(2.0));
	env.insert("b".into(), Value::Num(-3.5));
	env.insert("w".into(), Value::Num(1.2));
	env.insert("i".into(), Value::Int(3));
	env.insert("k".into(), Value::Int(0));
	env.insert("name".into(), Value::str("1.5+a"));
	env.insert("mat".into(), Value::str("wood"));
	env.insert("__seed__".into(), Value::str("s|x.parts:1:0"));
	let mut problems = Vec::new();
	for row in data.get("expressions").unwrap().as_list() {
		let text = row.get("text").unwrap().as_str();
		let got = evaluate(text, &env);
		match (got, row.get("value"), row.get("error")) {
			(Ok(v), Some(Json::Str(s)), _) if (s == "inf" && v == f64::INFINITY) || (s == "-inf" && v == f64::NEG_INFINITY) => {}
			(Ok(v), Some(want), _) if want.as_f64() == v => {}
			(Err(e), _, Some(want)) => {
				let want = want.as_str();
				// messages that quote Python's own syntax tree are only checked for what they start with
				let ok = if want.starts_with("unsupported in an expression") { e.starts_with("unsupported in an expression") } else { e == want };
				if !ok {
					problems.push(format!("{text:?}: error {e:?} != {want:?}"));
				}
			}
			(got, want, err) => problems.push(format!("{text:?}: {got:?} != {want:?} / {err:?}")),
		}
	}
	for pair in data.get("interpolate").unwrap().as_list() {
		let (text, want) = (pair.idx(0).as_str(), pair.idx(1).as_str());
		let got = interpolate(text, &env).unwrap();
		if got != want {
			problems.push(format!("interpolate {text:?}: {got:?} != {want:?}"));
		}
	}
	assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn numpy_streams_match() {
	let data = fixture("basics");
	for row in data.get("numpy").unwrap().as_list() {
		let seed = row.get("seed").unwrap().as_i64() as u64;
		let mut g = partscript::NumpyGenerator::new(seed);
		for v in row.get("f32").unwrap().as_list() {
			assert_eq!(g.random_f32() as f64, v.as_f64(), "seed {seed} f32");
		}
		let mut g = partscript::NumpyGenerator::new(seed);
		for v in row.get("f64").unwrap().as_list() {
			assert_eq!(g.random_f64(), v.as_f64(), "seed {seed} f64");
		}
	}
}
