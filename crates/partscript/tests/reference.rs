//! The Rust build against what the Python implementation made (tests/reference/*.json.gz): every example,
//! docs demo and golden shape, face by face.

use std::collections::HashMap;
use std::path::PathBuf;

use kitlib::json::Json;
use partscript::{BuildOptions, Built, Host, Project};

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

fn library() -> HashMap<String, String> {
	sources("library").into_iter().map(|(n, t)| (format!("../library/{n}"), t)).collect()
}

fn examples_project() -> Project {
	let std = partscript::load_program(&[], true, None);
	let mut names: Vec<&str> = std.macros.keys().collect();
	names.sort();
	let mut src = sources("examples");
	src.push(("std parts".into(), names.iter().map(|n| format!("prop std_{n} \"{n}\"\n  use {n}\n")).collect::<Vec<_>>().join("\n")));
	let lib = library();
	let reader = move |target: &str| lib.get(target).map(|t| vec![(target.to_string(), t.clone())]).unwrap_or_default();
	Project::new(src, Host::default(), true, Some(&reader))
}

/// Differences between a Rust build and the Python one (empty when they agree).
fn compare(built: &Built, want: &Json) -> Vec<String> {
	let mut out = Vec::new();
	let id = want.get("id").unwrap().as_str();
	let mut note = |m: String| out.push(format!("{id}: {m}"));
	if built.triangles() as i64 != want.get("triangles").unwrap().as_i64() {
		note(format!("triangles {} != {}", built.triangles(), want.get("triangles").unwrap().as_i64()));
	}
	let warnings: Vec<String> = want.get("warnings").unwrap().as_list().iter().map(|w| w.as_str().to_string()).collect();
	if built.warnings != warnings {
		note(format!("warnings {:?} != {:?}", built.warnings, warnings));
	}
	for (key, snaps) in [("snaps", &built.snaps), ("joints", &built.joints), ("links", &built.links)] {
		let mine: Vec<Json> = snaps.iter().map(|s| s.as_json()).collect();
		let theirs = want.get(key).unwrap().as_list();
		if mine.len() != theirs.len() {
			note(format!("{key}: {} != {}", mine.len(), theirs.len()));
		} else {
			for (a, b) in mine.iter().zip(theirs) {
				if !json_close(a, b, 2e-4) {
					note(format!("{key}: {} != {}", a.dumps(true), b.dumps(true)));
					break;
				}
			}
		}
	}
	let steps: Vec<(i64, i64)> = built.steps.iter().map(|s| (s.stmt.line as i64, s.faces.iter().map(|(_, a, b)| (b - a) as i64).sum())).collect();
	let want_steps: Vec<(i64, i64)> = want.get("steps").unwrap().as_list().iter().map(|s| (s.idx(0).as_i64(), s.idx(1).as_i64())).collect();
	if steps != want_steps {
		note(format!("steps {:?} != {:?}", &steps[..steps.len().min(8)], &want_steps[..want_steps.len().min(8)]));
	}
	let parts = want.get("parts").unwrap().as_list();
	if built.baked.len() != parts.len() {
		note(format!("parts {} != {}", built.baked.len(), parts.len()));
		return out;
	}
	for (baked, part) in built.baked.iter().zip(parts) {
		let materials: Vec<String> = part.get("materials").unwrap().as_list().iter().map(|m| m.as_str().to_string()).collect();
		if baked.materials != materials {
			note(format!("part {} materials {:?} != {:?}", baked.name, baked.materials, materials));
			continue;
		}
		let polys = part.get("polys").unwrap().as_list();
		if baked.polygons.len() != polys.len() {
			note(format!("part {} polygons {} != {}", baked.name, baked.polygons.len(), polys.len()));
			continue;
		}
		let mut bad = 0;
		for (k, (mine, theirs)) in baked.polygons.iter().zip(polys).enumerate() {
			let material = baked.materials.iter().position(|m| **m == *mine.face.material).unwrap() as i64;
			let co: Vec<f64> = mine.coords.iter().flat_map(|c| c.iter().copied()).collect();
			let uv: Vec<f64> = mine.uvs.iter().flat_map(|c| c.iter().copied()).collect();
			let tris: Vec<i64> = mine.triangles.iter().flat_map(|t| t.iter().map(|v| *v as i64)).collect();
			let floats = |key: &str| -> Vec<f64> { theirs.get(key).unwrap().as_list().iter().map(Json::as_f64).collect() };
			let ints = |key: &str| -> Vec<i64> { theirs.get(key).unwrap().as_list().iter().map(Json::as_i64).collect() };
			let close = |a: &[f64], b: &[f64], tol: f64| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol);
			let problem = if material != theirs.get("m").unwrap().as_i64() {
				Some(format!("material {} != {}", baked.materials[material as usize], materials[theirs.get("m").unwrap().as_i64() as usize]))
			} else if !close(&co, &floats("co"), 1e-4) {
				Some(format!("co {:?} != {:?}", co, floats("co")))
			} else if !close(&uv, &floats("uv"), 1e-4) {
				Some(format!("uv {:?} != {:?}", uv, floats("uv")))
			} else if !close(&mine.colours, &floats("tone"), 2e-3) {
				Some(format!("tone {:?} != {:?}", mine.colours, floats("tone")))
			} else if tris != ints("tris") {
				Some(format!("tris {:?} != {:?}", tris, ints("tris")))
			} else if mine.step != theirs.get("step").unwrap().as_i64() {
				Some(format!("step {} != {}", mine.step, theirs.get("step").unwrap().as_i64()))
			} else {
				None
			};
			if let Some(p) = problem {
				bad += 1;
				if bad <= 2 {
					note(format!("part {} polygon {k}: {p}", baked.name));
				}
			}
		}
		if bad > 2 {
			note(format!("part {}: {bad} polygons differ", baked.name));
		}
	}
	let gltf = kitlib::gltf::glb_json(&built.glb);
	let mine = Json::parse(&gltf).unwrap();
	let theirs = Json::parse(want.get("gltf").unwrap().as_str()).unwrap();
	if !json_close(&strip_offsets(mine), &strip_offsets(theirs), 2e-4) {
		note("glTF JSON differs".into());
	}
	out
}

/// The glTF document without byte offsets and lengths (the PNGs inside are compressed differently).
fn strip_offsets(mut doc: Json) -> Json {
	for key in ["bufferViews", "buffers"] {
		if let Some(Json::List(items)) = doc.get_mut(key) {
			for item in items {
				item.remove("byteOffset");
				item.remove("byteLength");
			}
		}
	}
	doc
}

fn json_close(a: &Json, b: &Json, tol: f64) -> bool {
	match (a, b) {
		(Json::Float(_) | Json::Int(_), Json::Float(_) | Json::Int(_)) => (a.as_f64() - b.as_f64()).abs() <= tol,
		(Json::List(x), Json::List(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| json_close(p, q, tol)),
		(Json::Dict(x), Json::Dict(y)) => x.len() == y.len() && x.iter().zip(y).all(|((k1, v1), (k2, v2))| k1 == k2 && json_close(v1, v2, tol)),
		_ => a == b,
	}
}

fn report(problems: Vec<String>, total: usize) {
	if !problems.is_empty() {
		let shown: Vec<&String> = problems.iter().take(60).collect();
		panic!("{} problems over {total} models:\n{}", problems.len(), shown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n"));
	}
}

#[test]
fn examples_build_as_python_built_them() {
	let data = fixture("examples");
	let mut project = examples_project();
	assert_eq!(project.errors(), Vec::<String>::new());
	let models = data.get("models").unwrap().as_list();
	let mut problems = Vec::new();
	for want in models {
		let id = want.get("id").unwrap().as_str();
		let (name, seed) = match id.split_once('@') {
			Some((n, s)) => (n, Some(s.to_string())),
			None => (id, None),
		};
		match project.build(name, &BuildOptions { glb: true, steps: true, snaps: true, seed }) {
			Ok(built) => problems.extend(compare(&built, want)),
			Err(e) => problems.push(format!("{id}: {e}")),
		}
	}
	let polygons: usize = models.iter().map(|m| m.get("parts").unwrap().as_list().iter().map(|p| p.get("polys").unwrap().as_list().len()).sum::<usize>()).sum();
	eprintln!("compared {} models, {polygons} polygons", models.len());
	report(problems, models.len());
}

#[test]
fn golden_shapes_build_as_python_built_them() {
	let data = fixture("golden");
	let text = std::fs::read_to_string(root().join("tests/golden/shapes.parts")).unwrap();
	let mut project = Project::from_text(&text, "shapes.parts", Host::default());
	let mut problems = Vec::new();
	for want in data.as_list() {
		let id = want.get("id").unwrap().as_str();
		match project.build(id, &BuildOptions { glb: true, steps: true, snaps: true, seed: None }) {
			Ok(built) => problems.extend(compare(&built, want)),
			Err(e) => problems.push(format!("{id}: {e}")),
		}
	}
	report(problems, data.as_list().len());
}

#[test]
fn docs_demos_build_as_python_built_them() {
	let data = fixture("docs");
	let examples = sources("examples");
	let lib = library();
	let mut problems = Vec::new();
	let mut total = 0;
	for demo in data.as_list() {
		let name = demo.get("name").unwrap().as_str();
		let code = demo.get("code").unwrap().as_str();
		let mut src = examples.clone();
		src.push((name.to_string(), code.to_string()));
		let reader = |target: &str| lib.get(target).map(|t| vec![(target.to_string(), t.clone())]).unwrap_or_default();
		let mut project = Project::new(src, Host::default(), true, Some(&reader));
		let want_errors: Vec<String> = demo.get("errors").unwrap().as_list().iter().map(|e| e.as_str().to_string()).collect();
		if project.errors() != want_errors {
			problems.push(format!("{name}: errors {:?} != {:?}", project.errors(), want_errors));
			continue;
		}
		for want in demo.get("models").unwrap().as_list() {
			total += 1;
			let id = want.get("id").unwrap().as_str();
			let result = project.build(id, &BuildOptions { glb: true, steps: true, snaps: true, seed: None });
			match (result, want.get("error")) {
				(Ok(built), None) => problems.extend(compare(&built, want)),
				(Err(e), Some(w)) if e.to_string() == w.as_str() => {}
				(Err(e), w) => problems.push(format!("{name} {id}: {e} (python: {w:?})")),
				(Ok(_), Some(w)) => problems.push(format!("{name} {id}: built, python said {}", w.as_str())),
			}
		}
	}
	report(problems, total);
}
