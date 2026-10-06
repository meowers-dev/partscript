//! Static check: names, materials, uses, copies and an estimate of triangles, without building.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use kitlib::json::Json;
use kitlib::py;

use crate::building as pb;
use crate::expr::{evaluate, interpolate};
use crate::host::Host;
use crate::lang::{
	colour_key, counts, every_material, expand_points, is_identifier_lower, is_snake_letter, parse_mod, split_top, Dressing, Mod, Program, Prop, Stmt,
	BUILD_OPS, SIDED, STYLE_LAYERS, USE_OPTIONS,
};
use crate::ordered::Ordered;
use crate::value::{Bounds, Env, Value};

const COMMON_OPTIONS: [&str; 22] = [
	"r", "when", "fade", "wobble", "twist", "bend", "shrink", "index", "jit", "hang", "along", "every", "fit", "closed", "joints", "corners", "smooth",
	"shade", "on", "drop", "facing", "sink",
];

fn shape_options(op: &str) -> Option<&'static [&'static str]> {
	Some(match op {
		"b" => &["skip", "taper", "lean", "from", "to", "break", "chunk", "core", "rubble"],
		"bb" => &["taper", "lean", "from", "to"],
		"bx" => &[],
		"c" => &["s", "rt", "ax", "arc", "caps", "capm", "from", "to"],
		"cone" => &["s", "ax", "from", "to"],
		"sph" => &["s", "rings"],
		"tube" => &["s", "ax"],
		"wedge" => &[],
		"lathe" => &["s", "arc", "cap"],
		"ext" => &["ax", "taper"],
		"pipe" => &["s", "arc", "taper"],
		"sweep" => &["prof", "arc", "open", "taper"],
		"face" => &["double"],
		"pan" => &["double"],
		"trim" => &["m"],
		"vault" => &["s"],
		"archwall" => &["open", "spring", "s"],
		"sign" => &["sub", "bg", "fg", "lit", "tex", "wrap", "sides", "mark", "accent", "printed", "double"],
		"torus" => &["s", "rings", "ax"],
		"frame" => &["hole", "hole_at"],
		"at" => &["s"],
		"chain" => &["at"],
		"link" => &[],
		"join" => &["reach", "radius", "mat", "s", "with", "bulge"],
		"row" => &["at", "gap", "over", "pack", "align"],
		"terrain" => &["cells", "height", "steep", "slope", "skirt"],
		_ => return None,
	})
}

fn long_name(op: &str) -> &str {
	match op {
		"b" => "box",
		"bb" => "bevel_box",
		"bx" => "box_between",
		"c" => "cylinder",
		"sph" => "sphere",
		"pan" => "panel",
		"ext" => "extrude",
		"at" => "group",
		"trim" => "decal",
		"archwall" => "arch_wall",
		"sign" => "label",
		other => other,
	}
}

const PLACING: [&str; 6] = ["on", "drop", "sink", "from", "to", "facing"];
pub const TRIANGLE_BUDGET: usize = 6000;
pub const PROP_BUDGET: usize = 2500;

/// A prop's triangle budget: budget=N, else a building's, a hero's (hero=1) or a prop's.
pub fn prop_budget(prop: &Prop) -> usize {
	if let Some(b) = prop.opt("budget") {
		if !b.is_empty() && b.chars().all(|c| c.is_ascii_digit()) {
			return b.parse().unwrap_or(usize::MAX);
		}
	}
	if prop.kind == "building" {
		return pb::BUILDING_BUDGET;
	}
	if prop.opt("hero").is_some_and(|h| !h.is_empty()) {
		TRIANGLE_BUDGET
	} else {
		PROP_BUDGET
	}
}

/// One prop in a check report.
#[derive(Clone, Debug)]
pub struct CheckedProp {
	pub id: String,
	pub title: String,
	pub subcategory: String,
	pub file: String,
	pub line: usize,
	pub triangles: usize,
	pub warnings: Vec<String>,
	pub kind: String,
}

/// What check() finds.
#[derive(Clone, Debug, Default)]
pub struct Report {
	pub prefix: String,
	pub props: Vec<CheckedProp>,
	pub errors: Vec<String>,
	pub warnings: Vec<String>,
	pub materials_checked: bool,
	pub styles: Vec<String>,
	pub dressing: Vec<String>,
}

impl Report {
	pub fn to_json(&self) -> Json {
		let props = self
			.props
			.iter()
			.map(|p| {
				let mut d = Json::dict();
				d.set("id", p.id.as_str())
					.set("title", p.title.as_str())
					.set("subcategory", p.subcategory.as_str())
					.set("file", p.file.as_str())
					.set("line", p.line)
					.set("triangles", p.triangles)
					.set("warnings", p.warnings.clone())
					.set("kind", p.kind.as_str());
				d
			})
			.collect();
		let mut out = Json::dict();
		out.set("prefix", self.prefix.as_str())
			.set("props", Json::List(props))
			.set("errors", self.errors.clone())
			.set("warnings", self.warnings.clone())
			.set("materials_checked", self.materials_checked)
			.set("styles", self.styles.clone())
			.set("dressing", self.dressing.clone());
		out
	}
}

fn origin_bounds() -> Value {
	Value::Bounds(Rc::new(Bounds::new("here", Some([0.0; 3]), Some([0.0; 3]), vec![], true)))
}

/// Static check: names, materials, uses, copies, estimated triangles.
pub fn check(program: &Program, host: &Host) -> Report {
	let prefix = host.prefix.clone();
	let assets = host.known_assets();
	let materials = host.known_materials();
	let native = host.native_props();
	let mut errors: Vec<String> = program.errors.iter().map(|e| e.to_string()).collect();
	let mut warnings: Vec<String> = Vec::new();
	let mut seen: HashMap<String, String> = HashMap::new();
	let defined: HashSet<String> = program.props.iter().map(|p| p.name.clone()).collect();
	let mut props_out: Vec<CheckedProp> = Vec::new();
	for (name, m) in program.macros.iter() {
		if m.file != "std.parts" && program.std.iter().any(|s| s == name) {
			warnings.push(format!("{}:{}: def {name} replaces the std part {name}", m.file, m.line));
		}
	}
	for prop in &program.props {
		let asset_id = host.asset_id(&prop.name);
		if let Some(before) = seen.get(&asset_id) {
			errors.push(format!("{}:{}: {asset_id} defined twice (also {before})", prop.file, prop.line));
		}
		seen.insert(asset_id.clone(), format!("{}:{}", prop.file, prop.line));
		let bare = host.bare(&asset_id);
		if program.macro_(&prop.name, &prop.file).is_some() {
			let kind = if program.std.iter().any(|s| *s == prop.name) { "std part" } else { "def" };
			warnings.push(format!("{}:{}: prop {} has the name of a {kind}; 'use {}' draws the {kind}, not this prop", prop.file, prop.line, prop.name, prop.name));
		}
		if native.contains(&bare) && !matches!(prop.opt("replace"), Some("1") | Some("true")) {
			errors.push(format!("{}:{}: {asset_id} is already one of the host's props; rename it or add replace=1 to remodel it", prop.file, prop.line));
		}
		for (key, low, high) in [("px", 16.0, 512.0), ("ao", 0.0, 10.0)] {
			let value = prop.opt(key).unwrap_or("");
			if !value.is_empty() && value != "auto" {
				let ok = crate::expr::py_float(value).is_some_and(|v| low <= v && v <= high);
				if !ok {
					errors.push(format!("{}:{}: {key}={value}: auto or a number from {} to {}", prop.file, prop.line, py::g(low), py::g(high)));
				}
			}
		}
		let mut context = Ctx { program, host, assets: &assets, materials: materials.as_ref(), defined: &defined, errors: vec![], warnings: vec![], building: prop.kind == "building" };
		let mut env = program.env_of(&prop.file);
		env.insert("i".into(), Value::Int(0));
		env.insert("here".into(), origin_bounds());
		let mut tris = match context.estimate(&prop.body, &env, 0) {
			Ok(t) => t,
			Err(e) => {
				errors.push(e);
				0
			}
		};
		if prop.kind == "building" {
			tris += check_building(program, prop, host, &mut context);
		}
		let mut prop_warnings = context.warnings.clone();
		let budget = prop_budget(prop);
		if tris > budget {
			prop_warnings.push(format!("about {tris} triangles, over the {budget} prop budget (hero=1 allows {TRIANGLE_BUDGET}, budget=N sets it)"));
		}
		errors.extend(context.errors.clone());
		warnings.extend(prop_warnings.iter().map(|w| format!("{asset_id}: {w}")));
		props_out.push(CheckedProp {
			id: asset_id,
			title: prop.title.clone(),
			subcategory: prop.subcategory.clone(),
			file: prop.file.clone(),
			line: prop.line,
			triangles: tris,
			warnings: prop_warnings,
			kind: prop.kind.clone(),
		});
	}
	let mut known: HashSet<String> = assets.keys().cloned().collect();
	known.extend(props_out.iter().map(|p| p.id.clone()));
	known.extend(native.iter().map(|n| host.asset_id(n)));
	let piece_ok = |name: &str| known.contains(name) || known.contains(&host.asset_id(name));
	for (name, style) in program.styles.iter() {
		let (file, line) = program.style_lines.get(name).cloned().unwrap_or_default();
		for (layer, entries) in style.as_dict() {
			if !STYLE_LAYERS.contains(&layer.as_str()) || layer == "decals" {
				continue;
			}
			let Json::List(entries) = entries else { continue };
			for entry in entries {
				let pieces: Vec<String> = match entry {
					Json::Str(s) => vec![s.clone()],
					Json::Dict(_) => ["piece", "front", "around", "beside", "above", "facing"]
						.iter()
						.filter_map(|k| match entry.get(k) {
							Some(Json::Str(s)) => Some(s.clone()),
							_ => None,
						})
						.collect(),
					_ => vec![],
				};
				for piece in pieces {
					if !piece_ok(&piece) {
						errors.push(format!("{file}:{line}: style {name}: no piece {} ({layer})", py::repr_str(&piece)));
					}
				}
			}
		}
	}
	for (bucket, items) in program.dressing.iter() {
		let pieces: Vec<String> = match items {
			Dressing::List(list) => list.iter().map(|(n, _)| n.clone()).collect(),
			Dressing::Named(named) => named.values().cloned().collect(),
		};
		for piece in pieces {
			if !piece_ok(&piece) {
				errors.push(format!("dressing {bucket}: no piece {}", py::repr_str(&piece)));
			}
		}
	}
	let mut styles: Vec<String> = program.styles.keys().map(str::to_string).collect();
	styles.sort();
	let mut dressing: Vec<String> = program.dressing.keys().map(str::to_string).collect();
	dressing.sort();
	Report { prefix, props: props_out, errors, warnings, materials_checked: materials.is_some(), styles, dressing }
}

/// Walks statements without building to count triangles and find bad names.
pub struct Ctx<'a> {
	program: &'a Program,
	host: &'a Host,
	assets: &'a HashMap<String, usize>,
	materials: Option<&'a HashSet<String>>,
	#[allow(dead_code)]
	defined: &'a HashSet<String>,
	pub errors: Vec<String>,
	pub warnings: Vec<String>,
	building: bool,
}

impl Ctx<'_> {
	fn fail(&mut self, stmt: &Stmt, message: impl Into<String>) {
		self.errors.push(format!("{}: {}", stmt.where_(), message.into()));
	}

	fn material(&mut self, stmt: &Stmt, token: &str, env: &Env) {
		for value in every_material(token, env) {
			if value == "none" {
				continue;
			}
			if value.starts_with('#') {
				match colour_key(&self.host.prefix, &value) {
					Ok(None) => self.fail(stmt, format!("colour {}: #rrggbb or #rrggbb/finish", py::repr_str(&value))),
					Err(e) => self.fail(stmt, e),
					Ok(Some(_)) => {}
				}
				continue;
			}
			if let Some(materials) = self.materials {
				if !materials.contains(&value) && !materials.contains(&self.host.asset_id(&value)) {
					self.fail(stmt, format!("unknown material {} (partscript materials lists them; or use #rrggbb/finish)", py::repr_str(&value)));
				}
			}
		}
	}

	fn number(&mut self, stmt: &Stmt, text: &str, env: &Env) -> f64 {
		if let Some(rest) = text.strip_prefix('~') {
			if matches!(env.get(rest), Some(Value::Bounds(_))) {
				return 0.0;
			}
		}
		let trimmed = text.trim_start_matches('~');
		match evaluate(if trimmed.is_empty() { "0" } else { trimmed }, env) {
			Ok(v) => v,
			Err(e) => {
				self.fail(stmt, e);
				0.0
			}
		}
	}

	fn vec(&mut self, stmt: &Stmt, text: &str, env: &Env, n: usize) -> Vec<f64> {
		if n == 3 && !text.contains(',') {
			match crate::compiler::anchor_point(text, env) {
				Err(e) => {
					self.fail(stmt, e);
					return vec![0.0; 3];
				}
				Ok(Some(point)) => return point.to_vec(),
				Ok(None) => {}
			}
		}
		let mut parts = split_top(text, ',');
		if parts.len() == 1 {
			parts = vec![parts[0].clone(); n];
		}
		if parts.len() != n {
			self.fail(stmt, format!("{}: expected {n} numbers", py::repr_str(text)));
			return vec![0.0; n];
		}
		parts.iter().map(|p| self.number(stmt, p, env)).collect()
	}

	fn copies(&mut self, stmt: &Stmt, env: &Env) -> usize {
		let mut count: usize = 1;
		for token in &stmt.mods {
			match parse_mod(token) {
				Some(Mod::Repeat(c, step)) => {
					match counts(&c, env) {
						Ok(cs) => {
							for n in &cs {
								count = count.saturating_mul(*n as usize);
							}
							if let Some(step) = &step {
								let n = if cs.len() > 1 { cs.len() } else { 3 };
								self.vec(stmt, step, env, n);
							}
							if let Some(hang) = stmt.opt("hang") {
								if cs.len() > 1 {
									self.fail(stmt, "hang= bends a single *N@ row (not a grid)");
								}
								self.number(stmt, hang, env);
							}
						}
						Err(e) => {
							self.fail(stmt, e);
						}
					}
				}
				Some(Mod::Ring(c, step)) => {
					match counts(&c, env) {
						Ok(cs) => count = count.saturating_mul(cs[0] as usize),
						Err(e) => self.fail(stmt, e),
					}
					self.number(stmt, &step, env);
				}
				Some(Mod::Scatter(c, spec)) => {
					match counts(&c, env) {
						Ok(cs) => count = count.saturating_mul(cs[0] as usize),
						Err(e) => self.fail(stmt, e),
					}
					let values = split_top(&spec, ',');
					if !(1..=3).contains(&values.len()) {
						self.fail(stmt, format!("scatter {token}: *N~R (a disc), *N~W,D (an area) or *N~W,D,GAP"));
					}
					for value in values {
						self.number(stmt, &value, env);
					}
				}
				Some(Mod::On(c, spec)) => {
					match counts(&c, env) {
						Ok(cs) => count = count.saturating_mul(cs[0] as usize),
						Err(e) => self.fail(stmt, e),
					}
					let (name, gap) = spec.split_once(',').unwrap_or((&spec, ""));
					if !matches!(env.get(name), Some(Value::Bounds(_))) {
						self.fail(stmt, format!("scatter on {name}: no shape of that name above this line (name one: {name} = box ...)"));
					}
					if !gap.is_empty() {
						self.number(stmt, gap, env);
					}
					let facing = stmt.opt("facing").unwrap_or("any");
					if !matches!(facing, "up" | "down" | "side" | "any") {
						self.fail(stmt, format!("facing={facing}: up, side, down or any"));
					}
				}
				None if matches!(token.as_str(), "mx" | "my" | "mz") => count = count.saturating_mul(2),
				None => {}
			}
		}
		if stmt.opts.contains("facing") && !stmt.mods.iter().any(|m| m.contains('^')) {
			self.fail(stmt, "facing= goes with scatter N on NAME (which of NAME's faces the copies grow on)");
		}
		if stmt.opts.contains("along") {
			count = count.saturating_mul(self.along(stmt, env));
		}
		if count > 400 {
			self.fail(stmt, format!("{count} copies; keep arrays under 400"));
		}
		count
	}

	/// How many copies along="P P P" makes (and its mistakes).
	fn along(&mut self, stmt: &Stmt, env: &Env) -> usize {
		let along = stmt.opt("along").unwrap();
		let line = env.get(along).map(Value::text).unwrap_or_else(|| along.to_string());
		let mut points = Vec::new();
		let mut bad = false;
		for p in crate::lang::words(&line) {
			let n = split_top(p, ',').len();
			if n == 2 || n == 3 {
				points.push(self.vec(stmt, p, env, n));
			} else {
				self.fail(stmt, format!("along point {}: x,y or x,y,z", py::repr_str(p)));
				bad = true;
			}
		}
		if bad {
			return 1;
		}
		let flag = |key: &str| matches!(stmt.opt(key), Some("1") | Some("true"));
		let every = match stmt.opt("every") {
			Some(e) => self.number(stmt, e, env),
			None => 0.0,
		};
		match kitlib::paths::path_frames(&points, every, flag("fit"), flag("corners"), flag("closed"), flag("joints")) {
			Ok(frames) => frames.len(),
			Err(e) => {
				self.fail(stmt, e);
				1
			}
		}
	}

	pub fn estimate(&mut self, body: &[Rc<Stmt>], env: &Env, depth: usize) -> Result<usize, String> {
		if depth > 8 {
			return Err("use/at nested more than 8 deep (a def using itself?)".into());
		}
		let mut total = 0usize;
		let mut env = env.clone();
		env.entry("here".into()).or_insert_with(origin_bounds);
		for stmt in body {
			match self.statement(stmt, &mut env, depth) {
				Ok(t) => total = total.saturating_add(t),
				Err(e) => self.errors.push(e),
			}
		}
		Ok(total)
	}

	fn statement(&mut self, stmt: &Rc<Stmt>, env: &mut Env, depth: usize) -> Result<usize, String> {
		let (op, mut a) = (stmt.op.as_str(), stmt.args.clone());
		let m = if op == "use" && !a.is_empty() { self.program.macro_(&a[0], &stmt.file) } else { None };
		let o: Ordered<String> = match &m {
			Some(m) if PLACING.iter().any(|k| m.params.contains(k)) => {
				stmt.opts.iter().filter(|(k, _)| !(PLACING.contains(k) && m.params.contains(k))).map(|(k, v)| (k.to_string(), v.clone())).collect()
			}
			_ => stmt.opts.clone(),
		};
		match op {
			"set" => {
				for (k, v) in o.iter() {
					env.insert(k.to_string(), Value::str(v));
				}
				return Ok(0);
			}
			"snap" => {
				if a.len() < 3 {
					self.fail(stmt, format!("'snap' needs a name, a position and a direction: {}", pb::SNAP_USAGE));
					return Ok(0);
				}
				self.vec(stmt, &a[1], env, 3);
				for (key, text) in [("direction", Some(a[2].clone())), ("up", o.get("up").cloned())] {
					if let Some(text) = text {
						let env2 = env.clone();
						let mut errs = Vec::new();
						let result = pb::parse_dir(&text, &mut |v: &str| match evaluate(v, &env2) {
							Ok(x) => Ok(x),
							Err(e) => {
								errs.push(e);
								Ok(0.0)
							}
						});
						for e in errs {
							self.fail(stmt, e);
						}
						if let Err(e) = result {
							self.fail(stmt, format!("snap {key}: {e}"));
						}
					}
				}
				let word_ok = |w: &str| w.len() <= 31 && is_snake_letter(w);
				if !word_ok(&a[0]) || o.get("kind").is_some_and(|k| !word_ok(k)) {
					self.fail(stmt, format!("snap names and kinds are lower_snake_case words: {}", pb::SNAP_USAGE));
				}
				return Ok(0);
			}
			_ => {}
		}
		if BUILD_OPS.contains(&op) {
			if !self.building {
				self.fail(stmt, format!("'{op}' belongs in a building (building NAME \"Title\" kit=SET), not a prop"));
			}
			return Ok(0);
		}
		match op {
			"link" => {
				if a.len() < 3 {
					self.fail(stmt, "link KIND at=P toward=DIR: an open end of kind KIND at P, pointing DIR (+x -y up ... or x,y,z)");
					return Ok(0);
				}
				if !(a[0].len() <= 31 && is_snake_letter(&a[0])) {
					self.fail(stmt, format!("link kind {}: a lower_snake_case word (rail, pipe, cable...)", py::repr_str(&a[0])));
				}
				self.copies(stmt, env);
				self.vec(stmt, &a[1], env, 3);
				let env2 = env.clone();
				if let Err(e) = pb::parse_dir(&a[2], &mut |v: &str| Ok(evaluate(v, &env2).unwrap_or(0.0))) {
					self.fail(stmt, format!("link toward: {e}"));
				}
				return Ok(0);
			}
			"join" => {
				if a.is_empty() {
					self.fail(stmt, "join KIND [reach=.6] [radius=.025] [mat=M] [sides=6] [bulge=B] [with=DEF]");
					return Ok(0);
				}
				for key in ["reach", "radius", "bulge"] {
					if let Some(v) = o.get(key) {
						self.number(stmt, v, env);
					}
				}
				if let Some(mat) = o.get("mat") {
					self.material(stmt, mat, env);
				}
				if let Some(with) = o.get("with") {
					if self.program.macro_(with, &stmt.file).is_none() {
						self.fail(stmt, format!("join with={with}: no def of that name (it draws the bridge along +X, length long)"));
					}
				}
				return Ok(0);
			}
			"chain" => {
				let mut names = Vec::new();
				for token in &a {
					let (name, count) = token.split_once('*').unwrap_or((token, ""));
					let n = if count.is_empty() { 1 } else { self.number(stmt, count, env) as i64 };
					for _ in 0..n.max(0) {
						names.push(name.to_string());
					}
				}
				if names.is_empty() {
					self.fail(stmt, "chain needs pieces: chain path_straight path_curve*2 ...");
				}
				let mut sum = 0usize;
				for name in &names {
					sum = sum.saturating_add(self.use_triangles(stmt, name, env, depth));
				}
				return Ok(sum.saturating_mul(self.copies(stmt, env)));
			}
			"part" | "size" | "card" => {
				if op == "card" {
					let mut unknown: Vec<&str> = o.keys().filter(|k| !["what", "where", "pairs", "look", "avoid", "notes"].contains(k)).collect();
					if !unknown.is_empty() {
						unknown.sort();
						self.fail(stmt, format!("card fields: what where pairs look avoid notes (not {})", unknown.join(", ")));
					}
				}
				return Ok(0);
			}
			_ => {}
		}
		let n = self.copies(stmt, env);
		if o.contains("sink") && !o.contains("drop") {
			self.fail(stmt, "sink= goes with drop= (how far a dropped thing settles into the ground)");
		} else if let Some(s) = o.get("sink") {
			self.number(stmt, s, env);
		}
		if let Some(d) = o.get("drop") {
			if !matches!(d.as_str(), "1" | "true" | "lean") {
				self.fail(stmt, format!("drop={d}: 1 (fall onto what is below) or lean (and tilt with the ground)"));
			}
		}
		if let Some(t) = o.get("taper") {
			if op == "pipe" || op == "sweep" {
				let v = self.number(stmt, t, env);
				if !(0.0..=20.0).contains(&v) {
					self.fail(stmt, format!("taper={t}: the size at the end of the line, 1 = unchanged (0-20)"));
				}
			}
		}
		if op == "b" && o.contains("break") {
			let v = self.number(stmt, o.get("break").unwrap(), env);
			if !(0.0..=1.0).contains(&v) {
				self.fail(stmt, format!("break={}: the share knocked out, 0-1", o.get("break").unwrap()));
			}
			if let Some(c) = o.get("chunk") {
				if self.number(stmt, c, env) <= 0.02 {
					self.fail(stmt, format!("chunk={c}: the size of the pieces it breaks into, in metres (over 2 cm)"));
				}
			}
			if let Some(c) = o.get("core") {
				self.material(stmt, c, env);
			}
			if let Some(r) = o.get("rubble") {
				self.number(stmt, r, env);
			}
		} else if ["chunk", "core", "rubble"].iter().any(|k| o.contains(k)) && op == "b" {
			self.fail(stmt, "chunk=, core= and rubble= go with break= (box ... break=.3 chunk=.25 core=brick rubble=.5)");
		}
		if let Some(on) = o.get("on") {
			let name = on.split('.').next().unwrap();
			if !matches!(env.get(name), Some(Value::Bounds(_))) {
				self.fail(stmt, format!("on={on}: no shape of that name above this line (name one: {on} = box ...)"));
			}
		}
		if !stmt.name.is_empty() {
			env.insert(stmt.name.clone(), Value::Bounds(Rc::new(Bounds::new(&stmt.name, Some([0.0; 3]), Some([1.0; 3]), vec![], false))));
		}
		for key in ["from", "to"] {
			if let Some(v) = o.get(key) {
				if op != "bx" {
					self.vec(stmt, v, env, 3);
				}
			}
		}
		if let Some(own) = shape_options(op) {
			let unknown: Vec<&str> = o.keys().filter(|k| !own.contains(k) && !COMMON_OPTIONS.contains(k)).collect();
			if let Some(first) = unknown.first() {
				let long = |k: &str| -> String {
					match k {
						"s" => (if op == "at" { "scale" } else { "sides" }).into(),
						"ax" => "axis".into(),
						"rt" => "top_radius".into(),
						"capm" => "cap_mat".into(),
						"r" => "turn".into(),
						"jit" => "jitter".into(),
						other => other.into(),
					}
				};
				let mut takes: Vec<String> = own.iter().filter(|k| **k != "printed").map(|k| long(k)).collect();
				takes.sort();
				let mut common: Vec<String> = COMMON_OPTIONS.iter().filter(|k| **k != "index").map(|k| long(k)).collect();
				common.sort();
				let takes = if takes.is_empty() { "none of its own".to_string() } else { takes.join(", ") };
				self.fail(stmt, format!("{}: no option {first}= (it takes {takes}; any line takes {})", long_name(op), common.join(", ")));
			}
		}
		if let Some(index) = o.get("index") {
			let names: Vec<&str> = index.split(',').collect();
			if !names.iter().all(|n| is_identifier_lower(n)) || names.len() > 3 {
				self.fail(stmt, format!("as {index}: one name (as k) or a grid's column,row[,layer] (as col,row)"));
			}
			for name in names {
				env.insert(name.to_string(), Value::Int(0));
			}
		}
		let with_i = |env: &Env| {
			let mut e = env.clone();
			e.insert("i".into(), Value::Int(0));
			e
		};
		for (key, low, high) in [("twist", -3600.0, 3600.0), ("shrink", 0.0, 10.0)] {
			if let Some(v) = o.get(key) {
				let x = self.number(stmt, v, &with_i(env));
				if !(low <= x && x <= high) {
					let what = if key == "twist" { "degrees of turn at the top" } else { "the size at the top, 1 = unchanged" };
					self.fail(stmt, format!("{key}={v}: {what}"));
				}
			}
		}
		if let Some(b) = o.get("bend") {
			self.vec(stmt, b, &with_i(env), if b.contains(',') { 2 } else { 1 });
		}
		if let Some(s) = o.get("smooth") {
			let v = self.number(stmt, s, env);
			if !(1.0..=32.0).contains(&v) {
				self.fail(stmt, format!("smooth={s}: pieces between each pair of points, 1-32"));
			}
		}
		if let Some(when) = o.get("when") {
			let mentions = {
				let words: Vec<&str> = when.split(|c: char| !crate::lang::is_word_char(c)).collect();
				words.contains(&"i") || when.contains("rand(") || words.contains(&"here")
			};
			if !mentions && self.number(stmt, when, env) == 0.0 {
				return Ok(0);
			}
			self.number(stmt, when, &with_i(env));
		}
		if let Some(w) = o.get("wobble") {
			self.vec(stmt, w, &with_i(env), 3);
		}
		if let Some(f) = o.get("fade") {
			let v = self.number(stmt, f, &with_i(env));
			if !(0.0..=1.0).contains(&v) {
				self.fail(stmt, format!("fade={f}: the tone at the base, 0-1 (1 = no fade)"));
			}
		}
		for key in ["r", "s"] {
			if let Some(v) = o.get(key) {
				if !(key == "s" && SIDED.contains(&op)) {
					self.vec(stmt, v, env, 3);
				}
			}
		}
		let need = match op {
			"b" | "bx" | "sph" | "wedge" => 3,
			"bb" | "c" | "cone" | "pan" | "trim" | "sign" | "torus" => 4,
			"tube" | "frame" => 5,
			_ => 0,
		};
		if a.len() < need {
			self.fail(stmt, format!("'{op}' needs {need} arguments, got {}: {}", a.len(), usage(op)));
			return Ok(0);
		}
		let int = |ctx: &mut Self, text: &str, env: &Env| ctx.number(stmt, text, env) as i64;
		let tris: i64 = match op {
			"b" | "bb" => {
				if let Some(t) = o.get("taper") {
					if !self.vec(stmt, t, env, 2).iter().all(|v| (0.0..=4.0).contains(v)) {
						self.fail(stmt, format!("taper={t}: the top face's scale in x,y, each 0-4 (1 = straight, 0 = a ridge or point)"));
					}
				}
				if let Some(l) = o.get("lean") {
					if !l.contains(',') {
						self.fail(stmt, format!("lean={l}: dx,dy, how far the top face shifts in metres (lean=0,-.1 leans toward the front)"));
					} else {
						self.vec(stmt, l, env, 2);
					}
				}
				self.vec(stmt, &a[0], env, 3);
				self.vec(stmt, &a[1], env, 3);
				if op == "b" {
					self.material(stmt, &a[2], env);
					12 - 2 * o.get("skip").map(|s| s.split(',').filter(|x| !x.is_empty()).count()).unwrap_or(0) as i64
				} else {
					self.material(stmt, &a[3], env);
					if self.number(stmt, &a[2], env) > 0.0 { 44 } else { 12 }
				}
			}
			"bx" => {
				self.vec(stmt, &a[0], env, 3);
				self.vec(stmt, &a[1], env, 3);
				self.material(stmt, &a[2], env);
				12
			}
			"c" | "cone" => {
				self.vec(stmt, &a[0], env, 3);
				self.number(stmt, &a[1], env);
				self.number(stmt, &a[2], env);
				self.material(stmt, &a[3], env);
				let sides = int(self, o.get("s").map(String::as_str).unwrap_or(if op == "c" { "10" } else { "8" }), env);
				if !(3..=48).contains(&sides) {
					self.fail(stmt, format!("s={sides}: 3-48 sides (6-16 is the house style)"));
				}
				sides * 2 + if o.get("caps").map(String::as_str) == Some("0") { 0 } else { (sides - 2) * 2 }
			}
			"torus" => {
				self.vec(stmt, &a[0], env, 3);
				self.number(stmt, &a[1], env);
				self.number(stmt, &a[2], env);
				self.material(stmt, &a[3], env);
				int(self, o.get("s").map(String::as_str).unwrap_or("16"), env) * int(self, o.get("rings").map(String::as_str).unwrap_or("6"), env) * 2
			}
			"frame" => {
				self.vec(stmt, &a[0], env, 3);
				for k in 1..4 {
					self.number(stmt, &a[k], env);
				}
				self.material(stmt, &a[4], env);
				for key in ["hole", "hole_at"] {
					if let Some(v) = o.get(key) {
						self.vec(stmt, v, env, 2);
					}
				}
				48
			}
			"sph" => {
				self.vec(stmt, &a[0], env, 3);
				self.number(stmt, &a[1], env);
				self.material(stmt, &a[2], env);
				int(self, o.get("s").map(String::as_str).unwrap_or("10"), env) * int(self, o.get("rings").map(String::as_str).unwrap_or("6"), env) * 2
			}
			"tube" => {
				self.vec(stmt, &a[0], env, 3);
				for k in 1..4 {
					self.number(stmt, &a[k], env);
				}
				self.material(stmt, &a[4], env);
				int(self, o.get("s").map(String::as_str).unwrap_or("12"), env) * 8
			}
			"wedge" => {
				self.vec(stmt, &a[0], env, 3);
				self.vec(stmt, &a[1], env, 3);
				self.material(stmt, &a[2], env);
				8
			}
			"lathe" => {
				if a.len() < 4 {
					self.fail(stmt, format!("'lathe' needs a centre, a material and 2+ r:z points: {}", usage(op)));
					return Ok(0);
				}
				self.vec(stmt, &a[0], env, 3);
				self.material(stmt, &a[1], env);
				for point in &a[2..] {
					if !point.contains(':') {
						self.fail(stmt, format!("lathe point {}: r:z", py::repr_str(point)));
					}
				}
				(a.len() as i64 - 3) * int(self, o.get("s").map(String::as_str).unwrap_or("16"), env) * 2
			}
			"ext" => {
				if a.len() < 6 {
					self.fail(stmt, format!("'ext' needs a centre, a width, a material and 3+ u:v outline points: {}", usage(op)));
					return Ok(0);
				}
				self.vec(stmt, &a[0], env, 3);
				self.number(stmt, &a[1], env);
				self.material(stmt, &a[2], env);
				for point in &a[3..] {
					let pair: Vec<&str> = point.split(':').collect();
					if pair.len() != 2 {
						self.fail(stmt, format!("outline point {}: u:v (two numbers joined by a colon, such as -.4:.2)", py::repr_str(point)));
					} else {
						self.number(stmt, pair[0], env);
						self.number(stmt, pair[1], env);
					}
				}
				if !matches!(o.get("ax").map(String::as_str).unwrap_or("x"), "x" | "y" | "z") {
					self.fail(stmt, format!("ax={}: x (side profile y:z), y (front profile x:z) or z (plan x:y)", o.get("ax").unwrap()));
				}
				if let Some(t) = o.get("taper") {
					let v = self.number(stmt, t, env);
					if !(0.0..=4.0).contains(&v) {
						self.fail(stmt, format!("taper={t}: the width's scale at the top of the outline, 0-4"));
					}
				}
				4 * (a.len() as i64 - 3) - 4
			}
			"vault" | "archwall" => {
				if a.len() < 5 {
					let what = if op == "vault" { "C W D RISE M" } else { "C W H T M" };
					self.fail(stmt, format!("'{op}' needs {what}: {}", usage(op)));
					return Ok(0);
				}
				self.vec(stmt, &a[0], env, 3);
				for k in 1..4 {
					self.number(stmt, &a[k], env);
				}
				self.material(stmt, &a[4], env);
				let s = int(self, o.get("s").map(String::as_str).unwrap_or("12"), env);
				if op == "vault" { s * 2 } else { s * 6 + 28 }
			}
			"pipe" | "sweep" if o.contains("arc") => {
				self.material(stmt, a.first().map(String::as_str).unwrap_or(""), env);
				let arc: Vec<f64> = split_top(o.get("arc").unwrap(), ',').iter().map(|v| self.number(stmt, v, env)).collect();
				if !(arc.len() == 5 || arc.len() == 6) {
					self.fail(stmt, "arc=cx,cz,r,a0,a1[,n]");
				}
				let steps = if arc.len() == 6 { arc[5] as i64 } else { 12 };
				let profile = if op == "sweep" && o.contains("prof") {
					o.get("prof").unwrap().split(',').count() as i64
				} else {
					int(self, o.get("s").map(String::as_str).unwrap_or("6"), env)
				};
				steps * profile * 2
			}
			"pipe" => {
				let mut all: Vec<String> = a.iter().take(2).cloned().collect();
				all.extend(expand_points(&a[2.min(a.len())..], env));
				a = all;
				if a.len() < 4 {
					self.fail(stmt, format!("'pipe' needs a material, a radius and 2+ points: {}", usage(op)));
					return Ok(0);
				}
				self.material(stmt, &a[0], env);
				self.number(stmt, &a[1], env);
				for point in &a[2..] {
					self.vec(stmt, point, env, 3);
				}
				(a.len() as i64 - 3) * int(self, o.get("smooth").map(String::as_str).unwrap_or("1"), env) * int(self, o.get("s").map(String::as_str).unwrap_or("6"), env) * 2 + 8
			}
			"sweep" => {
				if a.len() < 3 || !o.contains("prof") {
					self.fail(stmt, format!("'sweep' needs a material, prof= and 2+ points: {}", usage(op)));
					return Ok(0);
				}
				self.material(stmt, &a[0], env);
				let profile = o.get("prof").unwrap().split(',').count() as i64;
				(a.len() as i64 - 2) * int(self, o.get("smooth").map(String::as_str).unwrap_or("1"), env) * profile * 2
			}
			"face" => {
				if a.len() < 4 {
					self.fail(stmt, format!("'face' needs a material and 3+ points: {}", usage(op)));
					return Ok(0);
				}
				self.material(stmt, &a[0], env);
				for point in &a[1..] {
					self.vec(stmt, point, env, 3);
				}
				(a.len() as i64 - 3) * if matches!(o.get("double").map(String::as_str), Some("1") | Some("true")) { 2 } else { 1 }
			}
			"pan" | "trim" => {
				self.vec(stmt, &a[0], env, 3);
				self.number(stmt, &a[1], env);
				self.number(stmt, &a[2], env);
				if op == "pan" {
					self.material(stmt, &a[3], env);
				} else {
					let mut cells: Vec<String> = self.host.atlases().iter().flat_map(|atlas| atlas.cells.iter().map(|(n, _)| n.clone())).collect();
					cells.sort();
					cells.dedup();
					if !cells.contains(&a[3]) {
						if cells.is_empty() {
							self.fail(stmt, "trim: the texture provider has no decal sheets (use pan with a material instead)");
						} else {
							self.fail(stmt, format!("trim cell {}: one of {}", py::repr_str(&a[3]), cells.join(", ")));
						}
					}
				}
				if o.get("double").is_some_and(|d| !d.is_empty()) { 4 } else { 2 }
			}
			"sign" => {
				self.vec(stmt, &a[0], env, 3);
				self.number(stmt, &a[1], env);
				self.number(stmt, &a[2], env);
				if !a[3].starts_with('"') {
					self.fail(stmt, "sign text goes in quotes: sign 0,-.05,2 1.6 .3 \"THE HOPE & ANCHOR\"");
				}
				for text in [a[3].clone(), o.get("sub").cloned().unwrap_or_default()] {
					if let Err(e) = interpolate(&text, &with_i(env)) {
						self.fail(stmt, format!("label text: {e}"));
					}
				}
				let tex = o.get("tex").map(String::as_str).unwrap_or("256x32");
				let parts: Vec<&str> = tex.split('x').collect();
				let ok = parts.len() == 2 && parts.iter().all(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_digit()) && [16, 32, 64, 128, 256].contains(&t.parse::<i64>().unwrap_or(0)));
				if !ok {
					self.fail(stmt, format!("tex={}: WxH, each 16, 32, 64, 128 or 256 (textures are powers of two)", o.get("tex").map(String::as_str).unwrap_or("None")));
				}
				let mut t = 2;
				if let Some(wrap) = o.get("wrap") {
					let radius = self.number(stmt, wrap, env);
					let sides = int(self, o.get("sides").map(String::as_str).unwrap_or("12"), env);
					if radius <= 0.0 || self.number(stmt, &a[2], env) <= 0.0 || !(3..=48).contains(&sides) {
						self.fail(stmt, "wrapped sign needs positive radius/height and 3-48 sides");
					}
					if !matches!(o.get("mark").map(String::as_str).unwrap_or("bolt"), "bolt" | "orbit" | "star" | "wave") {
						self.fail(stmt, "wrapped sign mark: bolt, orbit, star or wave");
					}
					if let Some(accent) = o.get("accent") {
						let hex = accent.strip_prefix('#').unwrap_or("");
						if !(accent.starts_with('#') && hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit())) {
							self.fail(stmt, "wrapped sign accent must be #rrggbb");
						}
					}
					t = sides * 2;
				}
				t
			}
			"use" => {
				if a.is_empty() {
					self.fail(stmt, format!("'use' needs a name: {}", usage(op)));
					return Ok(0);
				}
				if a.len() > 1 {
					self.vec(stmt, &a[1], env, 3);
				}
				self.use_triangles(stmt, &a[0].clone(), env, depth) as i64
			}
			"at" => {
				if let Some(at) = a.first() {
					self.vec(stmt, at, env, 3);
				}
				self.estimate(stmt.block.as_deref().unwrap_or(&[]), env, depth + 1)? as i64
			}
			"terrain" => {
				self.vec(stmt, &a[0], env, 3);
				self.material(stmt, &a[2], env);
				let size = split_top(&a[1], ',');
				for value in &size {
					self.number(stmt, value, env);
				}
				if !(size.len() == 1 || size.len() == 2) {
					self.fail(stmt, format!("terrain size={}: W,D (metres across and back)", a[1]));
				}
				let cells: Vec<i64> = split_top(o.get("cells").map(String::as_str).unwrap_or("16"), ',').iter().map(|v| self.number(stmt, v, env) as i64).collect();
				if !cells.iter().all(|c| (1..=96).contains(c)) || cells.len() > 2 {
					self.fail(stmt, format!("cells={}: N or N,M corners a side, 1-96", o.get("cells").map(String::as_str).unwrap_or("None")));
				}
				let mut xy = env.clone();
				xy.insert("x".into(), Value::Num(0.0));
				xy.insert("y".into(), Value::Num(0.0));
				self.number(stmt, o.get("height").map(String::as_str).unwrap_or("0"), &xy);
				if let Some(s) = o.get("steep") {
					self.material(stmt, s, env);
				}
				if let Some(s) = o.get("slope") {
					let v = self.number(stmt, s, env);
					if !(0.0 < v && v < 90.0) {
						self.fail(stmt, format!("slope={s}: degrees, 0-90 (faces steeper than this take steep=)"));
					}
				}
				let (c0, c1) = (cells[0], cells[cells.len() - 1]);
				c0 * c1 * 2 + if matches!(o.get("skirt").map(String::as_str), Some("0") | Some("false")) { 0 } else { (c0 + c1) * 4 + 2 }
			}
			"row" => {
				if let Some(axis) = a.first() {
					if !matches!(axis.trim_start_matches(['+', '-']), "x" | "y" | "z") {
						self.fail(stmt, format!("row {axis}: the axis it runs along, x, y or z (-x runs the other way); stack is row z"));
					}
				}
				for key in ["gap", "over"] {
					if let Some(v) = o.get(key) {
						self.number(stmt, v, env);
					}
				}
				if let Some(at) = o.get("at") {
					self.vec(stmt, at, env, 3);
				}
				if let Some(pack) = o.get("pack") {
					if !matches!(pack.as_str(), "start" | "centre" | "center" | "end") {
						self.fail(stmt, format!("pack={pack}: start, centre or end"));
					}
				}
				for word in o.get("align").map(|s| s.split(',').filter(|w| !w.is_empty()).map(str::to_string).collect::<Vec<_>>()).unwrap_or_default() {
					if !matches!(word.as_str(), "left" | "right" | "front" | "back" | "bottom" | "top" | "centre" | "center") {
						self.fail(stmt, format!("align={word}: left right front back bottom top or centre (align=back,bottom)"));
					}
				}
				for child in stmt.block.iter().flatten() {
					if !child.name.is_empty() || matches!(child.op.as_str(), "part" | "snap" | "join" | "link" | "chain") || BUILD_OPS.contains(&child.op.as_str()) {
						let what = if child.name.is_empty() { py::repr_str(&child.op) } else { format!("{} = ...", child.name) };
						self.fail(child, format!("{what} can't go in a row or stack (name the row itself: books = row x ...)"));
					}
				}
				self.estimate(stmt.block.as_deref().unwrap_or(&[]), env, depth + 1)? as i64
			}
			_ => 0,
		};
		Ok((tris.max(0) as usize).saturating_mul(n))
	}

	pub fn use_triangles(&mut self, stmt: &Stmt, name: &str, env: &Env, depth: usize) -> usize {
		if let Some(m) = self.program.macro_(name, &stmt.file) {
			let mut local = env.clone();
			for (k, v) in self.program.env_of(&m.file) {
				local.insert(k, v);
			}
			for (k, v) in m.params.iter() {
				local.insert(k.to_string(), Value::str(v));
			}
			if stmt.opts.contains("from") {
				local.insert("length".into(), Value::Num(1.0));
			}
			for (key, value) in stmt.opts.iter() {
				if USE_OPTIONS.contains(&key) || (PLACING.contains(&key) && !m.params.contains(key)) {
					continue;
				}
				if !m.params.contains(key) {
					let takes = if m.params.is_empty() { "none".to_string() } else { m.params.keys().collect::<Vec<_>>().join(", ") };
					self.fail(stmt, format!("'{name}' has no parameter {} (it takes {takes})", py::repr_str(key)));
				}
				local.insert(key.to_string(), crate::lang::arg_value(value, env));
			}
			return match self.estimate(&m.body, &local, depth + 1) {
				Ok(t) => t,
				Err(e) => {
					self.errors.push(e);
					0
				}
			};
		}
		let asset = if name.contains("__") { name.to_string() } else { self.host.asset_id(name) };
		if let Some(t) = self.assets.get(&asset).or_else(|| self.assets.get(name)) {
			return *t;
		}
		let host = self.host;
		if let Some(own) = self.program.find_prop(name, &stmt.file, &|p: &Prop| vec![p.name.clone(), host.asset_id(&p.name)]) {
			let mut env = self.program.env_of(&own.file);
			env.insert("i".into(), Value::Int(0));
			return match self.estimate(&own.body, &env, depth + 1) {
				Ok(t) => t,
				Err(e) => {
					self.errors.push(e);
					0
				}
			};
		}
		if self.host.native_props().contains(&self.host.bare(&asset)) {
			self.warnings.push(format!("{}: {asset} is not built yet; counting 0 triangles", stmt.where_()));
			return 0;
		}
		if let Some(parts) = self.host.foreign_parts(name) {
			return parts.iter().map(|p| p.triangle_count()).sum();
		}
		if stmt.called {
			self.fail(stmt, format!("unknown statement '{name}' (not a shape, a def, a std part or a prop; partscript ref lists the shapes)"));
		} else {
			self.fail(stmt, format!("'use {name}': no def, std part or prop of that name (partscript ref lists the std parts)"));
		}
		0
	}
}

/// The reference line for a statement, for error messages.
pub fn usage(op: &str) -> String {
	for line in crate::reference::REFERENCE.lines() {
		let stripped = crate::expr::py_strip(line);
		if stripped.starts_with(&format!("{op} ")) || line.contains(&format!("  {op} ")) {
			return stripped.to_string();
		}
	}
	op.to_string()
}

/// A building's kit and room plan, without building: errors into context, triangles of what it places.
fn check_building(program: &Program, prop: &Prop, host: &Host, context: &mut Ctx) -> usize {
	let mut env = program.env_of(&prop.file);
	env.insert("i".into(), Value::Int(0));
	let kit = match pb::resolve_kit(program, prop.opt("kit").unwrap_or(""), &host.base_kit(), &prop.file) {
		Ok(k) => k,
		Err(e) => {
			context.errors.push(format!("{}:{}: {e}", prop.file, prop.line));
			return 0;
		}
	};
	let mut number = |text: &str| evaluate(text, &env);
	let result = pb::plan(prop, kit.clone(), &mut number);
	context.errors.extend(result.errors.clone());
	context.warnings.extend(result.warnings.clone());
	let mut total = 0usize;
	let mut counted: HashMap<String, usize> = HashMap::new();
	for placement in &result.placements {
		let (piece, stmt) = (&placement.piece, &placement.stmt);
		if placement.role == "fill" {
			let Some(m) = program.macro_(piece, &stmt.file) else {
				context.errors.push(format!("{}: fill={piece}: no def of that name (a room's fill names a def that furnishes it)", stmt.where_()));
				continue;
			};
			let mut local = env.clone();
			for (k, v) in program.env_of(&m.file) {
				local.insert(k, v);
			}
			for (k, v) in m.params.iter() {
				local.insert(k.to_string(), Value::str(v));
			}
			local.insert("i".into(), Value::Int(0));
			let fill = placement.fill.as_ref().unwrap();
			for (k, v) in &fill.env {
				if k == "__seed__" {
					continue;
				}
				local.insert(
					k.clone(),
					match v {
						pb::FillValue::Num(n) => Value::Num(*n),
						pb::FillValue::Int(n) => Value::Int(*n),
						pb::FillValue::Text(t) => Value::str(t),
					},
				);
			}
			if let Some((k, _)) = fill.env.iter().find(|(k, _)| !m.params.contains(k) && !pb::FILL_VARIABLES.contains(&k.as_str()) && k != "__seed__") {
				let takes = if m.params.is_empty() { "none".to_string() } else { m.params.keys().collect::<Vec<_>>().join(", ") };
				context.errors.push(format!("{}: fill={piece}: no parameter {} (it takes {takes})", stmt.where_(), py::repr_str(k)));
			}
			match context.estimate(&m.body, &local, 1) {
				Ok(t) => total += t,
				Err(e) => context.errors.push(e),
			}
			continue;
		}
		if !counted.contains_key(piece) {
			let own = program.find_prop(piece, &stmt.file, &|p: &Prop| vec![p.name.clone(), host.asset_id(&p.name), host.bare(&p.name)]);
			let count = if let Some(own) = own {
				let mut e = program.env_of(&own.file);
				e.insert("i".into(), Value::Int(0));
				match context.estimate(&own.body, &e, 1) {
					Ok(t) => t,
					Err(err) => {
						context.errors.push(err);
						0
					}
				}
			} else if program.macro_(piece, &stmt.file).is_some() || program.props.iter().any(|p| p.kind == "building" && p.name == *piece) {
				context.errors.push(format!("{}: {} ({}) is a def or a building; a building places props", stmt.where_(), py::repr_str(piece), placement.role));
				0
			} else {
				let before = context.errors.len();
				let probe = Stmt::new("use", vec![piece.clone()], Ordered::new(), vec![], &stmt.file, stmt.line);
				let t = context.use_triangles(&probe, piece, &env, 1);
				if context.errors.len() > before {
					let what = if placement.role != "attach" && placement.role != "place" { format!("kit {} piece", kit.name) } else { placement.role.clone() };
					context.errors.truncate(before);
					context.errors.push(format!("{}: {what} {}: no prop or built piece of that name", stmt.where_(), py::repr_str(piece)));
				}
				t
			};
			counted.insert(piece.clone(), count);
		}
		total += counted[piece];
	}
	for (_, entries) in kit.snaps.iter() {
		for entry in entries {
			if entry.tokens.len() < 3 {
				context.errors.push(format!("{}:{}: {}", entry.file, entry.line, pb::SNAP_USAGE));
			}
		}
	}
	total
}
