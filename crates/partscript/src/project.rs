//! A project: .parts files (plus the std parts) for a host, checked, built and written as .glb.
//!
//! ```no_run
//! use partscript::Project;
//! let mut project = Project::from_paths(&["props/"], partscript::Host::default()).unwrap();
//! let report = project.check();
//! let built = project.build("fruit_stall", &Default::default()).unwrap();
//! std::fs::write("fruit_stall.glb", &built.glb).unwrap();
//! ```

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use kitlib::bake::{bake, Baked};
use kitlib::geom::Part;
use kitlib::anim::{Quat, QUAT_IDENTITY};
use kitlib::gltf::{glb_bytes, glb_scene, Scene, SceneMesh, SceneNode};
use kitlib::maths::{sub, V3};
use kitlib::json::Json;
use kitlib::paths::{kind_colour, snap_markers, Marker};

use crate::building::Snap;
use crate::check::{check, prop_budget, Report};
use crate::compiler::{finish_parts, Compiler, Fail, Step};
use crate::host::Host;
use crate::lang::{colour_key, colour_mat, parse, parse_mod, split_top, Mod, PartScriptError, Program, Prop, Stmt};
use crate::textures::Recipe;

pub const STD: &str = include_str!("std.parts");

/// Seconds from a fixed point (none in a browser's WebAssembly, where there is no clock: always 0).
fn clock() -> f64 {
	#[cfg(not(target_family = "wasm"))]
	{
		use std::sync::OnceLock;
		static START: OnceLock<std::time::Instant> = OnceLock::new();
		START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
	}
	#[cfg(target_family = "wasm")]
	{
		0.0
	}
}

/// Reads an import: [(path, text)] for a file or a folder's .parts files (empty when there is none).
pub type Reader<'a> = &'a dyn Fn(&str) -> Vec<(String, String)>;

/// os.path.normpath, with / separators.
pub fn norm(path: &str) -> String {
	if path.is_empty() {
		return ".".into();
	}
	let initial = if path.starts_with("//") && !path.starts_with("///") {
		2
	} else if path.starts_with('/') {
		1
	} else {
		0
	};
	let mut parts: Vec<&str> = Vec::new();
	for comp in path.split('/') {
		if comp.is_empty() || comp == "." {
			continue;
		}
		if comp != ".." || (initial == 0 && parts.is_empty()) || parts.last() == Some(&"..") {
			parts.push(comp);
		} else if !parts.is_empty() {
			parts.pop();
		}
	}
	let joined = format!("{}{}", "/".repeat(initial), parts.join("/"));
	if joined.is_empty() {
		".".into()
	} else {
		joined
	}
}

fn dirname(path: &str) -> &str {
	match path.rfind('/') {
		Some(0) => "/",
		Some(i) => path[..i].trim_end_matches('/'),
		None => "",
	}
}

fn join(dir: &str, path: &str) -> String {
	if path.starts_with('/') || dir.is_empty() {
		path.to_string()
	} else if dir.ends_with('/') {
		format!("{dir}{path}")
	} else {
		format!("{dir}/{path}")
	}
}

/// Every .parts file under a folder, sorted as Python sorts paths (by their parts).
fn rglob(dir: &Path) -> Vec<PathBuf> {
	let mut out = Vec::new();
	let mut stack = vec![dir.to_path_buf()];
	while let Some(d) = stack.pop() {
		let Ok(entries) = std::fs::read_dir(&d) else { continue };
		for entry in entries.flatten() {
			let path = entry.path();
			if path.is_dir() {
				stack.push(path.clone());
			}
			if path.extension().is_some_and(|e| e == "parts") && path.is_file() {
				out.push(path);
			}
		}
	}
	out.sort_by(|a, b| a.components().collect::<Vec<_>>().cmp(&b.components().collect::<Vec<_>>()));
	out
}

fn find(target: &str, texts: &HashMap<String, String>, reader: Option<Reader>) -> Result<Vec<(String, String)>, String> {
	for candidate in [target.to_string(), format!("{target}.parts")] {
		if let Some(text) = texts.get(&candidate) {
			return Ok(vec![(candidate, text.clone())]);
		}
		let prefix = format!("{}/", candidate.trim_end_matches('/'));
		let mut inside: Vec<&String> = texts.keys().filter(|n| n.starts_with(&prefix)).collect();
		if !inside.is_empty() {
			inside.sort();
			return Ok(inside.into_iter().map(|n| (n.clone(), texts[n].clone())).collect());
		}
	}
	if let Some(reader) = reader {
		let found = reader(target);
		return Ok(if found.is_empty() { reader(&format!("{target}.parts")) } else { found });
	}
	for candidate in [PathBuf::from(target), PathBuf::from(format!("{target}.parts"))] {
		if candidate.is_dir() {
			return rglob(&candidate)
				.into_iter()
				.map(|p| std::fs::read_to_string(&p).map(|t| (norm(&p.to_string_lossy()), t)).map_err(|e| e.to_string()))
				.collect();
		}
		if candidate.is_file() {
			let text = std::fs::read_to_string(&candidate).map_err(|e| e.to_string())?;
			return Ok(vec![(norm(&candidate.to_string_lossy()), text)]);
		}
	}
	Ok(vec![])
}

/// A program from (file name, text) pairs, after the std parts unless std is false, and every file their
/// import lines bring in (each once; one imported as NAME has its names as NAME.x).
pub fn load_program(sources: &[(String, String)], std: bool, reader: Option<Reader>) -> Program {
	let mut program = Program::default();
	if std {
		parse(STD, "std.parts", &mut program);
	}
	program.std = program.macros.keys().map(str::to_string).collect();
	let texts: HashMap<String, String> = sources.iter().map(|(n, t)| (norm(n), t.clone())).collect();
	let mut loaded: HashSet<(String, Option<String>)> = HashSet::new();
	for (name, text) in sources {
		loaded.insert((norm(name), None));
		parse(text, name, &mut program);
	}
	let mut paths: HashMap<String, String> = sources.iter().map(|(n, _)| (n.clone(), n.clone())).collect();
	let mut done = 0;
	while done < program.imports.len() {
		let (importer, line, path, alias) = program.imports[done].clone();
		done += 1;
		let outer = program.namespaces.get(&importer).cloned();
		let namespace = match (&outer, &alias) {
			(Some(o), Some(a)) => Some(format!("{o}.{a}")),
			(_, Some(a)) => Some(a.clone()),
			(o, None) => o.clone(),
		};
		let base = paths.get(&importer).cloned().unwrap_or_else(|| importer.clone());
		let target = norm(&join(dirname(&base), &path));
		let found = match find(&target, &texts, reader) {
			Ok(f) => f,
			Err(e) => {
				program.errors.push(PartScriptError::new(format!("import \"{path}\": {e}"), &importer, line));
				continue;
			}
		};
		if found.is_empty() {
			program.errors.push(PartScriptError::new(format!("import \"{path}\": no such file or folder ({target})"), &importer, line));
			continue;
		}
		for (real, text) in found {
			let key = (norm(&real), namespace.clone());
			if loaded.contains(&key) {
				continue;
			}
			loaded.insert(key);
			let label = match &namespace {
				Some(ns) => format!("{ns}:{real}"),
				None => real.clone(),
			};
			paths.insert(label.clone(), real.clone());
			if let Some(ns) = &namespace {
				program.namespaces.insert(&label, ns.clone());
			}
			if !program.imported.contains(&label) {
				program.imported.push(label.clone());
			}
			parse(&text, &label, &mut program);
		}
	}
	program
}

/// Every .parts file in paths (files, or folders searched recursively), sorted.
pub fn parts_files(paths: &[impl AsRef<Path>]) -> Vec<PathBuf> {
	let mut out = Vec::new();
	for path in paths {
		let path = path.as_ref();
		if path.is_dir() {
			out.extend(rglob(path));
		} else {
			out.push(path.to_path_buf());
		}
	}
	out
}

/// A node of a built prop: a part at its pivot, or a mark.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
	pub name: String,
	/// the part it hangs from (None: the prop's root)
	pub parent: Option<String>,
	/// where it sits, in prop space: a part's pivot (the origin when it has none), a mark's point
	pub at: V3,
	/// a mark's turn (parts sit unturned)
	pub rotation: Quat,
	/// the baked part it draws (an index into Built::baked); None for a mark or a part with no shapes
	pub baked: Option<usize>,
	pub mark: bool,
}

/// One prop, built.
pub struct Built {
	pub asset_id: String,
	/// kitlib Parts with faces, in prop space
	pub parts: Vec<Part>,
	/// per top-level statement, the faces it added
	pub steps: Vec<Step>,
	/// the prop's own snap points
	pub snaps: Vec<Snap>,
	/// where chained pieces meet, each chain's open end, and joined link ends
	pub joints: Vec<Snap>,
	/// link ends nothing joined
	pub links: Vec<Snap>,
	pub warnings: Vec<String>,
	pub baked: Vec<Baked>,
	/// every face's use chain ((file, line), ...); a face's TEXCOORD_1.y indexes it
	pub origins: Vec<Vec<(String, usize)>>,
	/// the node tree: every part (a pivot and parent when it has them) and every mark, in the order written
	pub nodes: Vec<Node>,
	/// whether the prop has pivots, parents or marks (its .glb is then a node tree under the prop's root)
	pub rigged: bool,
	pub glb: Vec<u8>,
	pub seconds: f64,
}

impl Built {
	/// The model's triangles (snap markers not counted).
	pub fn triangles(&self) -> usize {
		self.baked.iter().filter(|b| b.name != "snaps").map(|b| b.triangles()).sum()
	}
}

/// How to build: write the .glb, tag faces with their steps, add snap markers, deal the draws again.
#[derive(Clone, Debug)]
pub struct BuildOptions {
	pub glb: bool,
	pub steps: bool,
	pub snaps: bool,
	pub seed: Option<String>,
}

impl Default for BuildOptions {
	fn default() -> Self {
		BuildOptions { glb: true, steps: false, snaps: false, seed: None }
	}
}

/// A prop in a project's list.
#[derive(Clone, Debug)]
pub struct PropInfo {
	pub id: String,
	pub title: String,
	pub subcategory: String,
	pub kind: String,
	pub file: String,
	pub line: usize,
	pub imported: bool,
}

/// What write_all did.
#[derive(Default)]
pub struct Written {
	pub built: Vec<(String, usize, f64)>,
	pub errors: Vec<String>,
	pub warnings: Vec<String>,
	/// what stopped the build part-way (the props before it are written)
	pub fatal: Option<String>,
}

pub struct Project {
	pub host: Rc<Host>,
	pub sources: Vec<(String, String)>,
	pub program: Rc<Program>,
	compiler: Option<Compiler>,
}

impl Project {
	pub fn new(sources: Vec<(String, String)>, host: Host, std: bool, reader: Option<Reader>) -> Project {
		let program = load_program(&sources, std, reader);
		Project { host: Rc::new(host), sources, program: Rc::new(program), compiler: None }
	}

	/// Every .parts file in these files and folders.
	pub fn from_paths(paths: &[impl AsRef<Path>], host: Host) -> std::io::Result<Project> {
		let mut sources = Vec::new();
		for p in parts_files(paths) {
			sources.push((p.to_string_lossy().to_string(), std::fs::read_to_string(&p)?));
		}
		Ok(Project::new(sources, host, true, None))
	}

	/// One piece of text.
	pub fn from_text(text: &str, name: &str, host: Host) -> Project {
		Project::new(vec![(name.to_string(), text.to_string())], host, true, None)
	}

	/// Parse errors (each file:line: message).
	pub fn errors(&self) -> Vec<String> {
		self.program.errors.iter().map(|e| e.to_string()).collect()
	}

	pub fn props(&self) -> Vec<PropInfo> {
		self.program
			.props
			.iter()
			.map(|p| PropInfo {
				id: self.host.asset_id(&p.name),
				title: p.title.clone(),
				subcategory: p.subcategory.clone(),
				kind: p.kind.clone(),
				file: p.file.clone(),
				line: p.line,
				imported: self.program.is_imported(&p.file),
			})
			.collect()
	}

	pub fn prop(&self, name: &str) -> Result<Rc<Prop>, PartScriptError> {
		self.program
			.props
			.iter()
			.find(|p| p.name == name || self.host.asset_id(&p.name) == name)
			.cloned()
			.ok_or_else(|| PartScriptError::bare(format!("no prop {}", kitlib::py::repr_str(name))))
	}

	/// The static check (no building).
	pub fn check(&self) -> Report {
		check(&self.program, &self.host)
	}

	/// The compiler, made (and its materials registered) once.
	pub fn compiler(&mut self) -> Result<&mut Compiler, PartScriptError> {
		if self.compiler.is_none() {
			if !self.program.errors.is_empty() {
				let first: Vec<String> = self.program.errors.iter().take(10).map(|e| e.to_string()).collect();
				return Err(PartScriptError::bare(first.join("; ")));
			}
			let mut compiler = Compiler::new(self.program.clone(), self.host.clone());
			compiler.register_materials().map_err(PartScriptError::bare)?;
			self.compiler = Some(compiler);
		}
		Ok(self.compiler.as_mut().unwrap())
	}

	/// One prop: its parts and steps, baked, and (glb) written to .glb bytes.
	pub fn build(&mut self, name: &str, options: &BuildOptions) -> Result<Built, PartScriptError> {
		let started = clock();
		kitlib::py::take_fault();
		let prop = self.prop(name)?;
		let asset_id = self.host.asset_id(&prop.name);
		let host = self.host.clone();
		let compiler = self.compiler()?;
		compiler.snap_sink = Some(Vec::new());
		compiler.joints = Vec::new();
		let warnings_before = compiler.warnings.len();
		let traced = compiler.trace(&prop, &asset_id, options.seed.as_deref());
		let snaps = compiler.snap_sink.take().unwrap_or_default();
		let marks = std::mem::take(&mut compiler.marks);
		let (parts, raw_steps) = match traced {
			Ok(r) => r,
			Err(Fail::Script(e)) => return Err(e),
			Err(Fail::Value(m)) => return Err(PartScriptError::bare(m)),
			Err(Fail::Skip) => return Err(PartScriptError::bare("a material of none left the prop empty")),
		};
		let mut face_steps: Vec<Vec<i64>> = parts.iter().map(|p| vec![-1; p.faces.len()]).collect();
		for (index, step) in raw_steps.iter().enumerate() {
			for &(part, first, end) in &step.faces {
				for slot in &mut face_steps[part][first..end] {
					*slot = index as i64;
				}
			}
		}
		let rigged = !marks.is_empty() || parts.iter().any(|p| p.pivot.is_some() || p.parent.is_some());
		// every part's node (a rig keeps a part with no shapes as an empty node, for its children to hang from)
		let declared: Vec<(String, Option<V3>, Option<String>, bool)> =
			parts.iter().map(|p| (p.name.clone(), p.pivot, p.parent.clone(), !p.faces.is_empty())).collect();
		let (mut parts, face_steps): (Vec<Part>, Vec<Vec<i64>>) = parts.into_iter().zip(face_steps).filter(|(p, _)| !p.faces.is_empty()).unzip();
		finish_parts(&prop, &mut parts);
		let ao_height = match prop.opt("ao") {
			Some(a) if !a.is_empty() && a != "auto" => crate::expr::py_float(a).unwrap_or(1.4),
			_ => 1.4,
		};
		let mut warnings: Vec<String> = compiler.warnings[warnings_before..].to_vec();
		let joints = compiler.joints.clone();
		let links = compiler.last_links.clone();
		let mut baked: Vec<Baked> = parts.iter().zip(&face_steps).map(|(p, s)| bake(p, ao_height, Some(s))).collect();
		let mut interned: HashMap<Vec<(String, usize)>, usize> = HashMap::new();
		let mut origins: Vec<Vec<(String, usize)>> = Vec::new();
		for b in &mut baked {
			for polygon in &mut b.polygons {
				let key: Vec<(String, usize)> = polygon.face.origin.iter().map(|(f, l)| (f.to_string(), *l)).collect();
				let next = interned.len();
				let index = *interned.entry(key.clone()).or_insert_with(|| {
					origins.push(key);
					next
				});
				polygon.origin = index;
			}
		}
		let triangles: usize = baked.iter().map(|b| b.triangles()).sum();
		let budget = prop_budget(&prop);
		if triangles > budget {
			warnings.push(format!("{asset_id}: {triangles} triangles, over the {budget} budget"));
		}
		if options.snaps && (!snaps.is_empty() || !joints.is_empty() || !links.is_empty()) {
			let markers: Vec<Marker> = snaps.iter().chain(&joints).chain(&links).map(|s| Marker { pos: s.pos, dir: s.dir, kind: s.kind.clone() }).collect();
			let mut material_of = |kind: &str| marker_material(&host, kind);
			baked.push(bake(&snap_markers(&markers, &mut material_of, 0.06), 0.0, None));
		}
		let mut nodes = Vec::new();
		let mut drawn = 0;
		for (name, pivot, parent, has_faces) in &declared {
			if !has_faces && !(rigged && (pivot.is_some() || parent.is_some() || declared.iter().any(|d| d.2.as_deref() == Some(name.as_str())))) {
				continue;
			}
			let index = if *has_faces {
				drawn += 1;
				Some(drawn - 1)
			} else {
				None
			};
			nodes.push(Node { name: name.clone(), parent: parent.clone(), at: pivot.unwrap_or([0.0; 3]), rotation: QUAT_IDENTITY, baked: index, mark: false });
		}
		for mark in &marks {
			nodes.push(Node { name: mark.name.clone(), parent: mark.on.clone(), at: mark.at, rotation: mark.rotation, baked: None, mark: true });
		}
		if rigged {
			warnings.extend(pivot_warnings(&asset_id, &nodes, &parts));
		}
		let mut glb = Vec::new();
		if options.glb {
			let materials = host.materials.borrow().clone();
			let textures = |name: &str| host.texture_png(name);
			let (data, missing) = if rigged {
				glb_scene(&baked, &rig_scene(&asset_id, &nodes), &materials, &textures, options.steps, "")
			} else {
				glb_bytes(&asset_id, &baked, &materials, &textures, options.steps, "")
			};
			glb = data;
			warnings.extend(missing.iter().map(|name| format!("{asset_id}: texture {name} could not be made; shown grey")));
		}
		// a fault outside any line was an exception nothing caught: the build stops
		match kitlib::py::take_fault() {
			Some(kitlib::py::Fault::Value(message)) => return Err(PartScriptError::fatal(format!("ValueError: {message}"))),
			Some(kitlib::py::Fault::Fatal(message)) => return Err(PartScriptError::fatal(message)),
			None => {}
		}
		Ok(Built { asset_id, parts, steps: raw_steps, snaps, joints, links, warnings, baked, origins, nodes, rigged, glb, seconds: clock() - started })
	}

	/// Build and write out_dir/<id>.glb.
	pub fn write(&mut self, name: &str, out_dir: &Path, options: &BuildOptions) -> Result<Built, PartScriptError> {
		let built = self.build(name, options)?;
		std::fs::create_dir_all(out_dir).map_err(|e| PartScriptError::bare(e.to_string()))?;
		std::fs::write(out_dir.join(format!("{}.glb", built.asset_id)), &built.glb).map_err(|e| PartScriptError::bare(e.to_string()))?;
		Ok(built)
	}

	/// Every prop (not buildings) as out_dir/<id>.glb.
	pub fn write_all(&mut self, out_dir: &Path, only: Option<&[String]>, seed: Option<&str>) -> Written {
		let mut out = Written::default();
		let props: Vec<Rc<Prop>> = self.program.props.clone();
		let options = BuildOptions { seed: seed.map(str::to_string), ..Default::default() };
		for prop in props {
			let id = self.host.asset_id(&prop.name);
			if prop.kind == "building" || self.program.is_imported(&prop.file) {
				continue;
			}
			if let Some(only) = only {
				if !only.iter().any(|o| *o == prop.name || *o == id) {
					continue;
				}
			}
			match self.write(&prop.name, out_dir, &options) {
				Ok(built) => {
					out.built.push((built.asset_id.clone(), built.triangles(), (built.seconds * 10000.0).round() / 10000.0));
					out.warnings.extend(built.warnings);
				}
				Err(e) if e.fatal => {
					out.fatal = Some(e.to_string());
					break;
				}
				Err(e) => out.errors.push(e.to_string()),
			}
		}
		out
	}

	/// A building as placement data, in Y-up space.
	pub fn building(&mut self, name: &str) -> Result<Json, PartScriptError> {
		let prop = self.prop(name)?;
		if prop.kind != "building" {
			return Err(PartScriptError::new(format!("{name} is a prop, not a building"), &prop.file, prop.line));
		}
		let compiler = self.compiler()?;
		let plan = match compiler.building_plan(&prop) {
			Ok(p) => p,
			Err(Fail::Script(e)) => return Err(e),
			Err(Fail::Value(m)) => return Err(PartScriptError::bare(m)),
			Err(Fail::Skip) => return Err(PartScriptError::bare("skip")),
		};
		let asset_of = |piece: &str| compiler.asset_of(piece);
		Ok(crate::building::export_building(&plan, &prop.title, &asset_of))
	}

	/// What a prop or part is built from, all the way down.
	pub fn uses(&self, name: &str) -> Json {
		let root = if self.program.macros.contains(name) { name.to_string() } else { self.prop(name).map(|p| p.name.clone()).unwrap_or_else(|_| name.to_string()) };
		self.node(&root, 1, 0, &mut Vec::new(), "")
	}

	fn node(&self, target: &str, count: i64, at: usize, stack: &mut Vec<String>, file: &str) -> Json {
		let host = self.host.clone();
		let m = self.program.macro_(target, file);
		let prop = if m.is_none() { self.program.find_prop(target, file, &|p: &Prop| vec![p.name.clone(), host.asset_id(&p.name)]) } else { None };
		let (kind, body, file, line, name) = if let Some(m) = &m {
			(if m.file == "std.parts" { "std" } else { "def" }, m.body.clone(), m.file.clone(), m.line, m.name.clone())
		} else if let Some(p) = &prop {
			("prop", p.body.clone(), p.file.clone(), p.line, p.name.clone())
		} else {
			let kind = if self.host.foreign_parts(target).is_some() { "host" } else { "missing" };
			(kind, vec![], String::new(), 0, target.to_string())
		};
		let mut out = Json::dict();
		out.set("name", name).set("kind", kind).set("file", file.as_str()).set("line", line).set("count", count).set("at", at);
		let mut children = Vec::new();
		if !stack.iter().any(|s| s == target) {
			stack.push(target.to_string());
			for stmt in walk(&body) {
				if stmt.op == "use" && !stmt.args.is_empty() {
					children.push(self.node(&stmt.args[0], copies(&stmt), stmt.line, stack, &stmt.file));
				}
			}
			stack.pop();
		}
		out.set("children", Json::List(children));
		out
	}
}

fn walk(body: &[Rc<Stmt>]) -> Vec<Rc<Stmt>> {
	let mut out = Vec::new();
	for stmt in body {
		out.push(stmt.clone());
		if let Some(block) = &stmt.block {
			out.extend(walk(block));
		}
	}
	out
}

/// Copies a line makes when its counts are plain numbers (*3, *2x4, *6%60, mx), else 1 per unknown count.
fn copies(stmt: &Stmt) -> i64 {
	let mut count = 1i64;
	for m in &stmt.mods {
		if matches!(m.as_str(), "mx" | "my" | "mz") {
			count *= 2;
			continue;
		}
		let head = m[1..].split('@').next().unwrap().split('%').next().unwrap().split('~').next().unwrap().split('^').next().unwrap();
		for item in split_top(head, 'x') {
			count *= if !item.is_empty() && item.chars().all(|c| c.is_ascii_digit()) { item.parse::<i64>().unwrap_or(1) } else { 1 };
		}
		let _ = parse_mod(m).map(|m| matches!(m, Mod::Repeat(..)));
	}
	count
}

/// The scene a rigged prop writes: a root node named for the asset, then each part's node at its pivot
/// (its corners written from there) under its parent's, and each mark as an empty node.
pub fn rig_scene(asset_id: &str, nodes: &[Node]) -> Scene {
	let mut scene = Scene::default();
	scene.nodes.push(SceneNode::new(asset_id, None));
	let index_of = |name: &str| nodes.iter().position(|n| !n.mark && n.name == name);
	for (k, node) in nodes.iter().enumerate() {
		// a parent that is not a part, or a circle of parents, leaves the node on the root (check reports both)
		let mut parent = node.parent.as_deref().and_then(index_of);
		let mut seen = vec![k];
		let mut walk = parent;
		while let Some(p) = walk {
			if seen.contains(&p) {
				parent = None;
				break;
			}
			seen.push(p);
			walk = nodes[p].parent.as_deref().and_then(index_of);
		}
		let base = parent.map(|p| nodes[p].at).unwrap_or([0.0; 3]);
		let mut scene_node = SceneNode::new(&node.name, Some(parent.map(|p| p + 1).unwrap_or(0)));
		scene_node.translation = sub(node.at, base);
		scene_node.rotation = node.rotation;
		scene_node.mesh = node.baked.map(|b| SceneMesh::Part { baked: b, origin: node.at });
		scene.nodes.push(scene_node);
	}
	scene
}

/// A pivot well away from its part's shapes is most likely a typo (a sign, a decimal point).
fn pivot_warnings(asset_id: &str, nodes: &[Node], parts: &[Part]) -> Vec<String> {
	let mut out = Vec::new();
	for node in nodes {
		let Some(b) = node.baked else { continue };
		if node.at == [0.0; 3] {
			continue;
		}
		let (lo, hi) = parts[b].bounds();
		let reach = (0..3).map(|k| hi[k] - lo[k]).fold(0.0f64, f64::max) * 0.5 + 0.05;
		let outside = (0..3).map(|k| (lo[k] - node.at[k]).max(node.at[k] - hi[k]).max(0.0)).fold(0.0f64, f64::max);
		if outside > reach {
			out.push(format!("{asset_id}: part {}'s pivot is {} m outside its shapes", node.name, kitlib::py::round_to(outside, 3)));
		}
	}
	out
}

/// A lit colour material for snap markers of a kind (matching kinds share a colour).
fn marker_material(host: &Host, kind: &str) -> String {
	let (key, hex, finish) = colour_key(&host.prefix, &format!("#{}/glow", kind_colour(kind))).unwrap().unwrap();
	if !host.has_material(&key) {
		host.add_material(&key, colour_mat(&key, &hex, &finish), Some(Recipe::Surface { finish: finish.clone(), hex: hex.clone() }));
	}
	key
}
