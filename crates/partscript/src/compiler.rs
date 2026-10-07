//! The compiler: a Program's props into kitlib Parts (faces, materials, snaps), for a Host.
//!
//! It also notes which faces each top-level statement of a prop adds (its steps), for previews that light
//! up what a line made.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use kitlib::py::PyMath;
use kitlib::geom::{bounds_of, empty_origin, Face, Mat, Origin, Part};
use kitlib::json::Json;
use kitlib::maths::{self, add, cross, dot, euler, length, normalized, scale, sub, M3, M4, V3};
use kitlib::paths::{path_frames, smooth_points, Frame};
use kitlib::py::{self, PyRandom};
use kitlib::ruin::broken_box;
use kitlib::surface::{spread, SurfaceIndex};

use crate::building::{self as pb, Kit, Placement, Plan, Snap};
use crate::expr::{evaluate, evaluate_number, interpolate, is_dynamic, pick_options};
use crate::host::Host;
use crate::lang::{
	arg_value, colour_key, colour_mat, counts, expand_points, is_identifier_lower, material_items, parse_mod, sign_key, split_top, unquote, Macro, Mod,
	PartScriptError, Program, Prop, Stmt, BUILD_OPS, USE_OPTIONS,
};
use crate::ordered::Ordered;
use crate::textures::Recipe;
use crate::value::{env_frame, Bounds, Env, Value};

/// Options that place a line, unless a def it uses takes them itself.
pub const PLACING: [&str; 6] = ["on", "drop", "sink", "from", "to", "facing"];

pub const SIDES: [(&str, V3); 6] = [
	("top", [0.0, 0.0, 1.0]),
	("bottom", [0.0, 0.0, -1.0]),
	("front", [0.0, -1.0, 0.0]),
	("back", [0.0, 1.0, 0.0]),
	("left", [-1.0, 0.0, 0.0]),
	("right", [1.0, 0.0, 0.0]),
];

fn side_normal(side: &str) -> Option<V3> {
	SIDES.iter().find(|(s, _)| *s == side).map(|(_, n)| *n)
}

/// What goes wrong while compiling.
#[derive(Debug)]
pub enum Fail {
	/// reported as it is
	Script(PartScriptError),
	/// a ValueError: reported with the line's op, file and line
	Value(String),
	/// a material of none: the shape is left out
	Skip,
}

impl From<PartScriptError> for Fail {
	fn from(e: PartScriptError) -> Fail {
		Fail::Script(e)
	}
}

impl From<String> for Fail {
	fn from(e: String) -> Fail {
		Fail::Value(e)
	}
}

impl From<&str> for Fail {
	fn from(e: &str) -> Fail {
		Fail::Value(e.to_string())
	}
}

pub type CResult<T> = Result<T, Fail>;

/// int(x) for a float, as Python does it: towards zero; NaN is an error for the line, and infinity
/// (an OverflowError the original never caught) ends the build.
/// The ZeroDivisionError Python met dividing a turn into 0 steps (`k / steps for k in range(steps + 1)`):
/// the original never caught it, so the build stops.
fn zero_steps(steps: i64) -> CResult<()> {
	if steps == 0 {
		return Err(Fail::Script(PartScriptError::fatal("ZeroDivisionError: float division by zero")));
	}
	Ok(())
}

pub fn py_int(v: f64) -> CResult<i64> {
	if v.is_nan() {
		return Err(Fail::Value("cannot convert float NaN to integer".into()));
	}
	if v.is_infinite() {
		return Err(Fail::Script(PartScriptError::fatal("OverflowError: cannot convert float infinity to integer")));
	}
	Ok(v.trunc() as i64)
}

/// A piece a building or a chain places: its faces in its own space, the snaps it declares, its open ends.
pub struct Piece {
	pub faces: Vec<Face>,
	pub snaps: Ordered<Snap>,
	pub links: Vec<Snap>,
}

/// What a top-level statement of a prop added.
pub struct Step {
	pub stmt: Rc<Stmt>,
	pub note: String,
	/// (part index, first face, end)
	pub faces: Vec<(usize, usize, usize)>,
}

/// The prop (or the def standing in for one) a body runs for.
#[derive(Clone)]
pub struct Ctx {
	pub name: String,
	pub file: String,
	pub line: usize,
	pub kind: String,
	pub prop: Option<Rc<Prop>>,
}

impl Ctx {
	pub fn of_prop(prop: &Rc<Prop>) -> Ctx {
		Ctx { name: prop.name.clone(), file: prop.file.clone(), line: prop.line, kind: prop.kind.clone(), prop: Some(prop.clone()) }
	}

	fn of_macro(name: &str, file: &str, line: usize) -> Ctx {
		Ctx { name: name.to_string(), file: file.to_string(), line, kind: "prop".into(), prop: None }
	}
}

pub struct Compiler {
	pub program: Rc<Program>,
	pub host: Rc<Host>,
	prefix: String,
	colours: Ordered<(String, String)>,
	signs: Ordered<Json>,
	foreign: HashMap<String, Rc<Vec<Face>>>,
	building: HashSet<String>,
	/// where chained pieces met, for snap markers
	pub joints: Vec<Snap>,
	/// open ends waiting for a join, in this build's space
	pub links: Vec<Snap>,
	join_rules: Vec<(Rc<Stmt>, Env)>,
	/// the open ends a finished build left, for whatever places it
	pub last_links: Vec<Snap>,
	foreign_links: HashMap<String, Vec<Snap>>,
	/// snap statements land here while it is Some
	pub snap_sink: Option<Vec<Snap>>,
	plans: HashMap<usize, Rc<Plan>>,
	pieces: HashMap<String, Rc<Piece>>,
	pub warnings: Vec<String>,
	path: Origin,
	fallen: Option<(M4, V3, Vec<(V3, V3)>)>,
	steps: Vec<Step>,
}

fn origin_plus(path: &Origin, file: &Rc<str>, line: usize) -> Origin {
	let mut out = (**path).clone();
	out.push((file.clone(), line));
	Rc::new(out)
}

impl Compiler {
	pub fn new(program: Rc<Program>, host: Rc<Host>) -> Compiler {
		Compiler {
			prefix: host.prefix.clone(),
			program,
			host,
			colours: Ordered::new(),
			signs: Ordered::new(),
			foreign: HashMap::new(),
			building: HashSet::new(),
			joints: Vec::new(),
			links: Vec::new(),
			join_rules: Vec::new(),
			last_links: Vec::new(),
			foreign_links: HashMap::new(),
			snap_sink: None,
			plans: HashMap::new(),
			pieces: HashMap::new(),
			warnings: Vec::new(),
			path: empty_origin(),
			fallen: None,
			steps: Vec::new(),
		}
	}

	pub fn new_part(&self, name: &str) -> Part {
		Part::new(name, self.host.materials.clone())
	}

	// ------------------------------------------------------------ buildings and snaps
	/// The prop a name used in file means (its own namespace's first), by name or asset id.
	pub fn program_prop(&self, name: &str, file: &str) -> Option<Rc<Prop>> {
		let host = self.host.clone();
		self.program.find_prop(name, file, &|p: &Prop| vec![p.name.clone(), host.asset_id(&p.name), host.bare(&p.name)])
	}

	/// What a name used in file resolves to, as one name for caches.
	fn key(&self, name: &str, file: &str) -> String {
		if let Some(m) = self.program.macro_(name, file) {
			return m.name.clone();
		}
		self.program_prop(name, file).map(|p| p.name.clone()).unwrap_or_else(|| name.to_string())
	}

	/// The asset a building places for a piece.
	pub fn asset_of(&self, name: &str) -> String {
		if name.contains("__") {
			return name.to_string();
		}
		let prop = self.program_prop(name, "");
		let id = self.host.asset_id(prop.as_ref().map(|p| p.name.as_str()).unwrap_or(name));
		self.host.placed_id(&id, prop.is_some())
	}

	/// (faces in the piece's own space, snaps it declares, its open ends) for a prop, a def or a host asset.
	pub fn piece(&mut self, name: &str, stmt: Option<&Stmt>) -> CResult<Rc<Piece>> {
		let file = stmt.map(|s| s.file.to_string()).unwrap_or_else(|| "<building>".into());
		let name = self.key(name, &file);
		if let Some(p) = self.pieces.get(&name) {
			return Ok(p.clone());
		}
		let saved = self.snap_sink.replace(Vec::new());
		let result: CResult<(Vec<Face>, Vec<Snap>)> = (|| {
			let prop = self.program_prop(&name, "");
			let macro_ = self.program.macro_(&name, "");
			if let Some(prop) = prop {
				let asset = self.host.asset_id(&prop.name);
				let parts = self.build_used(&prop, &asset)?;
				let faces = parts.iter().flat_map(|p| p.faces.iter().cloned()).collect();
				Ok((faces, self.last_links.clone()))
			} else if let Some(m) = macro_ {
				let mut scratch = vec![self.new_part(&name)];
				let scope = self.open_links();
				let home = self.program.env_of(&m.file);
				let mut env = home.clone();
				for (k, v) in m.params.iter() {
					env.insert(k.to_string(), arg_value(v, &home));
				}
				let run = self.run(&m.body, &env, &mut scratch, &M4::IDENTITY, 1, &Ctx::of_macro(&name, &m.file, m.line));
				let links = self.close_links(scope, &mut scratch);
				run?;
				let links = links?;
				Ok((scratch.swap_remove(0).faces, links))
			} else {
				let stmt_default = Stmt::new("use", vec![name.clone()], Ordered::new(), vec![], "<building>", 0);
				let faces = self.foreign_faces(&name, stmt.unwrap_or(&stmt_default))?;
				Ok(((*faces).clone(), self.foreign_links.get(&name).cloned().unwrap_or_default()))
			}
		})();
		let sink = std::mem::replace(&mut self.snap_sink, saved).unwrap_or_default();
		let (faces, links) = result?;
		let mut snaps = Ordered::new();
		for s in sink {
			snaps.insert(&s.name.clone(), s);
		}
		let piece = Rc::new(Piece { faces, snaps, links });
		self.pieces.insert(name, piece.clone());
		Ok(piece)
	}

	/// Every snap of a piece: its bounds', its role's in the kit, the kit's snap lines for it, its own.
	pub fn piece_snaps(&mut self, name: &str, role: &str, kit: &Kit) -> CResult<Ordered<Snap>> {
		let piece = self.piece(name, None)?;
		let mut snaps: Ordered<Snap> = Ordered::new();
		if !piece.faces.is_empty() {
			let (lo, hi) = bounds_of(piece.faces.iter());
			for (k, s) in pb::bounds_snaps(lo, hi).iter() {
				snaps.insert(k, s.clone());
			}
		}
		let mut roles = vec![role.to_string()];
		roles.extend(kit.roles_of(name));
		for kit_role in roles {
			for s in pb::role_snaps(&kit_role, kit) {
				snaps.insert(&s.name.clone(), s);
			}
		}
		let env = self.program.env_of(&kit.file);
		if let Some(entries) = kit.snaps.get(name) {
			for entry in entries {
				let snap = self.snap(&entry.tokens, &Ordered::new(), &env, "")?;
				snaps.insert(&snap.name.clone(), snap);
			}
		}
		for (k, s) in piece.snaps.iter() {
			snaps.insert(k, s.clone());
		}
		Ok(snaps)
	}

	fn snap(&self, args: &[String], opts: &Ordered<String>, env: &Env, suffix: &str) -> CResult<Snap> {
		let tokens: Vec<&String> = args.iter().filter(|t| !t.contains('=')).collect();
		let mut merged: Ordered<String> = Ordered::new();
		for t in args.iter().filter(|t| t.contains('=')) {
			let (k, v) = t.split_once('=').unwrap();
			merged.insert(k, v.to_string());
		}
		for (k, v) in opts.iter() {
			merged.insert(k, v.clone());
		}
		if tokens.len() < 3 {
			return Err(Fail::Value("list index out of range".into()));
		}
		let mut number = |text: &str| evaluate(text, env);
		let direction = pb::parse_dir(tokens[2], &mut number)?;
		let up = match merged.get("up") {
			Some(u) => pb::parse_dir(u, &mut number)?,
			None => pb::default_up(direction),
		};
		let pos = self.vec(tokens[1], env, 3)?;
		Ok(Snap::new(&format!("{}{suffix}", tokens[0]), [pos[0], pos[1], pos[2]], direction, up, merged.get("kind").map(String::as_str).unwrap_or("any")))
	}

	/// The plan of a building, with its attachments placed (once a compile).
	pub fn building_plan(&mut self, prop: &Rc<Prop>) -> CResult<Rc<Plan>> {
		if let Some(p) = self.plans.get(&prop.uid) {
			return Ok(p.clone());
		}
		let env = self.program.env_of(&prop.file);
		let at = |e: String| Fail::Script(PartScriptError::new(e, &prop.file, prop.line));
		let kit = pb::resolve_kit(&self.program, prop.opt("kit").unwrap_or(""), &self.host.base_kit(), &prop.file).map_err(at)?;
		let mut number = |text: &str| evaluate(text, &env);
		let mut result = pb::plan(prop, kit.clone(), &mut number);
		if let Some(first) = result.errors.first() {
			return Err(Fail::Script(PartScriptError::bare(first.clone())));
		}
		let resolved = pb::resolve(&mut result, &mut |name: &str, role: &str| self.piece_snaps(name, role, &kit));
		match resolved {
			Ok(()) => {}
			Err(pb::ResolveError::Build(message)) => return Err(at(message)),
			Err(pb::ResolveError::Piece(fail)) => return Err(fail),
		}
		self.warnings.extend(result.warnings.clone());
		let plan = Rc::new(result);
		self.plans.insert(prop.uid, plan.clone());
		Ok(plan)
	}

	/// One placed piece of a building into the current part (a room's fill= runs its def there).
	fn place(&mut self, placement: &Placement, parts: &mut Vec<Part>, frame: &M4) -> CResult<()> {
		let matrix = M4(placement.matrix.unwrap());
		if placement.role == "fill" {
			let Some(m) = self.program.macro_(&placement.piece, &placement.stmt.file) else {
				return Err(PartScriptError::new(
					format!("fill={}: no def of that name (a room's fill names a def that furnishes it)", placement.piece),
					&placement.stmt.file,
					placement.stmt.line,
				)
				.into());
			};
			let home = self.program.env_of(&m.file);
			let mut env = home.clone();
			for (k, v) in m.params.iter() {
				env.insert(k.to_string(), arg_value(v, &home));
			}
			let fill = placement.fill.as_ref().unwrap();
			for (k, v) in &fill.env {
				let value = match v {
					pb::FillValue::Num(n) => Value::Num(*n),
					pb::FillValue::Int(n) => Value::Int(*n),
					pb::FillValue::Text(t) if k == "__seed__" => Value::str(t),
					pb::FillValue::Text(t) => arg_value(t, &env),
				};
				env.insert(k.clone(), value);
			}
			let inner_path = origin_plus(&self.path, &placement.stmt.file, placement.stmt.line);
			let saved = std::mem::replace(&mut self.path, inner_path);
			let before: Vec<usize> = parts.iter().map(|p| p.faces.len()).collect();
			let room = frame.mul(&matrix);
			let run = self.run(&m.body, &env, parts, &room, 1, &Ctx::of_macro(&m.name, &m.file, m.line));
			self.path = saved;
			run?;
			let faces: Vec<Face> = since(parts, &before).into_iter().cloned().collect();
			self.check_fill(placement, &env, &faces, &room, &m);
			return Ok(());
		}
		let piece = self.piece(&placement.piece, Some(&placement.stmt))?;
		let where_ = frame.mul(&matrix);
		let origin = origin_plus(&self.path, &placement.stmt.file, placement.stmt.line);
		append(parts.last_mut().unwrap(), &piece.faces, &where_, &origin);
		let moved = moved(&piece.links, &where_)?;
		self.links.extend(moved);
		Ok(())
	}

	/// Warn when a room's furnishing reaches outside the room or stands in a doorway.
	fn check_fill(&mut self, placement: &Placement, env: &Env, faces: &[Face], room: &M4, m: &Macro) {
		if faces.is_empty() {
			return;
		}
		let (ox, oy, oz) = (room.0[0][3], room.0[1][3], room.0[2][3]);
		let num = |k: &str| env.get(k).and_then(Value::as_number).unwrap_or(0.0);
		let (w, d, h) = (num("w"), num("d"), num("h"));
		let points: Vec<V3> = faces.iter().flat_map(|f| f.points.iter().map(|p| [p[0] - ox, p[1] - oy, p[2] - oz])).collect();
		let mut out = Vec::new();
		for (k, axis, size) in [(0, "x", w), (1, "y", d), (2, "z", h)] {
			let low = py::min_iter(points.iter().map(|p| p[k])).unwrap_or(f64::INFINITY);
			let high = py::max_iter(points.iter().map(|p| p[k])).unwrap_or(f64::NEG_INFINITY);
			if low < -0.35 || high > size + 0.35 {
				out.push(format!("{axis} {low:.2}..{high:.2} m (the room is 0..{size:.2})"));
			}
		}
		let fill = placement.fill.as_ref().unwrap();
		let level = env.get("level").map(Value::text).unwrap_or_default();
		let label = format!("{}: fill={} ({}, storey {level})", placement.stmt.where_(), placement.piece, fill.room);
		if !out.is_empty() {
			self.warnings.push(format!("{label} reaches outside the room: {}", out.join(", ")));
		}
		for (side, along) in &fill.doors {
			let zone = if side == "s" || side == "n" {
				(along - 0.5, along + 0.5, if side == "s" { 0.0 } else { d - 0.9 }, if side == "s" { 0.9 } else { d })
			} else {
				(if side == "w" { 0.0 } else { w - 0.9 }, if side == "w" { 0.9 } else { w }, along - 0.5, along + 0.5)
			};
			let mut blocking: Vec<String> = Vec::new();
			for f in faces {
				let xs: Vec<f64> = f.points.iter().map(|p| p[0] - ox).collect();
				let ys: Vec<f64> = f.points.iter().map(|p| p[1] - oy).collect();
				let zs: Vec<f64> = f.points.iter().map(|p| p[2] - oz).collect();
				let max = |v: &[f64]| py::max_iter(v.iter().copied()).unwrap_or(f64::NEG_INFINITY);
				let min = |v: &[f64]| py::min_iter(v.iter().copied()).unwrap_or(f64::INFINITY);
				if max(&zs) < 0.05 || min(&zs) > 2.0 {
					continue;
				}
				if min(&xs) < zone.1 && max(&xs) > zone.0 && min(&ys) < zone.3 && max(&ys) > zone.2 {
					let item = f.origin.iter().skip(self.path.len() + 1).find(|o| *o.0 == *m.file);
					let what = item.map(|o| format!("line {}", o.1)).unwrap_or_else(|| "it".into());
					if !blocking.contains(&what) {
						blocking.push(what);
					}
				}
			}
			if !blocking.is_empty() {
				blocking.sort();
				self.warnings.push(format!("{label}: {} stands in the doorway on its {side} wall ({along:.2} m along)", blocking.join(", ")));
			}
		}
	}

	fn record_snaps(&mut self, stmt: &Stmt, env: &Env, frame: &M4, copies: &[M4]) -> CResult<()> {
		for (index, matrix) in copies.iter().enumerate() {
			let suffix = if copies.len() > 1 { format!("_{}", index + 1) } else { String::new() };
			let snap = self.snap(&stmt.args, &stmt.opts, env, &suffix)?;
			let m = frame.mul(matrix);
			let moved = snap.moved(&m.0)?;
			if let Some(sink) = &mut self.snap_sink {
				sink.push(moved);
			}
		}
		Ok(())
	}

	// ------------------------------------------------------------ materials
	/// Every colour and sign the files name, as materials. A colour with no such finish anywhere stops
	/// the whole build (as it always did: the error names it).
	pub fn register_materials(&mut self) -> Result<(), String> {
		let program = self.program.clone();
		for prop in &program.props {
			let env: HashMap<String, String> = program.env_of(&prop.file).iter().map(|(k, v)| (k.clone(), v.text())).collect();
			self.scan(&prop.body, env)?;
		}
		for m in program.macros.values() {
			for value in m.params.values() {
				self.colour(value)?;
			}
			let mut env: HashMap<String, String> = program.env_of(&m.file).iter().map(|(k, v)| (k.clone(), v.text())).collect();
			for (k, v) in m.params.iter() {
				env.insert(k.to_string(), v.clone());
			}
			self.scan(&m.body, env)?;
		}
		for (key, (hex, finish)) in self.colours.iter() {
			self.host.add_material(key, colour_mat(key, hex, finish), Some(Recipe::Surface { finish: finish.clone(), hex: hex.clone() }));
		}
		let signs: Vec<(String, Json)> = self.signs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
		for (key, spec) in signs {
			self.register_sign(&key, &spec);
		}
		Ok(())
	}

	fn register_sign(&self, key: &str, spec: &Json) {
		let lit = spec.get("lit").map(Json::as_f64).unwrap_or(0.0);
		let mut mat = Mat::new(key, 1.0);
		mat.emission = if lit > 0.0 { key.to_string() } else { String::new() };
		mat.emission_strength = lit;
		mat.ao = false;
		self.host.add_material(key, mat, Some(Recipe::Sign(spec.clone())));
	}

	fn colour(&mut self, token: &str) -> Result<(), String> {
		for option in pick_options(token) {
			for item in option.split('|') {
				if item.starts_with('#') {
					if let Some((key, hex, finish)) = colour_key(&self.prefix, item)? {
						self.colours.insert(&key, (hex, finish));
					}
				}
			}
		}
		Ok(())
	}

	fn scan(&mut self, body: &[Rc<Stmt>], mut env: HashMap<String, String>) -> Result<(), String> {
		for stmt in body {
			if stmt.op == "set" {
				for (k, v) in stmt.opts.iter() {
					env.insert(k.to_string(), v.clone());
				}
			}
			for token in stmt.args.iter().chain(stmt.opts.values()) {
				for option in pick_options(token) {
					self.colour(&option)?;
					for item in option.split('|') {
						if let Some(value) = env.get(item).cloned() {
							self.colour(&value)?;
						}
					}
				}
			}
			if stmt.op == "sign" && stmt.args.len() >= 4 {
				let dynamic = format!("{}{}", stmt.args[3], stmt.opt("sub").unwrap_or(""));
				if !is_dynamic(&dynamic) {
					let spec = sign_spec(stmt, None)?;
					self.signs.insert(&sign_key(&self.prefix, &spec), spec);
				}
			}
			if let Some(block) = &stmt.block {
				self.scan(block, env.clone())?;
			}
		}
		Ok(())
	}

	fn material(&self, stmt: &Stmt, token: &str, env: &Env, copy: usize) -> CResult<String> {
		let items = material_items(token, env);
		let offset = env.get("__copy__").and_then(Value::as_number).unwrap_or(0.0) as i64;
		let index = (copy as i64 + offset).rem_euclid(items.len() as i64) as usize;
		let value = &items[index];
		if value == "none" {
			return Err(Fail::Skip);
		}
		if value.starts_with('#') {
			return match colour_key(&self.prefix, value) {
				Ok(Some((key, _, _))) => Ok(key),
				Ok(None) => Err(PartScriptError::new(format!("colour {}", py::repr_str(value)), &stmt.file, stmt.line).into()),
				Err(e) => Err(Fail::Value(e)),
			};
		}
		self.host
			.find_material(value)
			.ok_or_else(|| PartScriptError::new(format!("unknown material {}", py::repr_str(value)), &stmt.file, stmt.line).into())
	}

	// ------------------------------------------------------------ build
	/// A prop built from its own top-level statements: its parts and steps.
	pub fn trace(&mut self, prop: &Rc<Prop>, asset_id: &str, seed: Option<&str>) -> CResult<(Vec<Part>, Vec<Step>)> {
		if self.building.contains(asset_id) {
			return Err(PartScriptError::new(format!("{asset_id} uses itself (through use)"), &prop.file, prop.line).into());
		}
		self.building.insert(asset_id.to_string());
		let outer_steps = std::mem::take(&mut self.steps);
		let outer_path = std::mem::replace(&mut self.path, empty_origin());
		let mut parts = vec![self.new_part(asset_id)];
		let scope = self.open_links();
		let result: CResult<()> = (|| {
			if prop.kind == "building" {
				self.building_plan(prop)?;
			}
			let mut env = self.program.env_of(&prop.file);
			env.insert("__seed__".into(), Value::str(seed.unwrap_or_else(|| prop.opt("seed").unwrap_or(""))));
			self.run(&prop.body, &env, &mut parts, &M4::IDENTITY, 0, &Ctx::of_prop(prop))
		})();
		self.building.remove(asset_id);
		let steps = std::mem::replace(&mut self.steps, outer_steps);
		self.path = outer_path;
		let closed = self.close_links(scope, &mut parts);
		result?;
		self.last_links = closed?;
		if !parts.iter().any(|p| !p.faces.is_empty()) {
			return Err(PartScriptError::new(format!("{asset_id} has no shapes"), &prop.file, prop.line).into());
		}
		Ok((parts, steps))
	}

	/// Another prop pulled in with use: built whole, its joints its own.
	fn build_used(&mut self, prop: &Rc<Prop>, asset_id: &str) -> CResult<Vec<Part>> {
		let saved = std::mem::take(&mut self.joints);
		let result = self.trace(prop, asset_id, None);
		self.joints = saved;
		let (parts, _) = result?;
		Ok(parts.into_iter().filter(|p| !p.faces.is_empty()).collect())
	}

	/// The body of a prop or def, into parts.
	pub fn run(&mut self, body: &[Rc<Stmt>], env: &Env, parts: &mut Vec<Part>, frame: &M4, depth: usize, prop: &Ctx) -> CResult<()> {
		if depth == 0 {
			return self.run_traced(body, env, parts, frame, prop);
		}
		self.run_plain(body, env, parts, frame, depth, prop)
	}

	/// A prop's own lines: each top-level statement noted as a step (a building's, piece by piece).
	fn run_traced(&mut self, body: &[Rc<Stmt>], env: &Env, parts: &mut Vec<Part>, frame: &M4, prop: &Ctx) -> CResult<()> {
		let mut env = env.clone();
		for stmt in body {
			if stmt.op == "set" {
				for (k, v) in stmt.opts.iter() {
					let value = arg_value(v, &env);
					env.insert(k.to_string(), value);
				}
				continue;
			}
			if BUILD_OPS.contains(&stmt.op.as_str()) && prop.kind == "building" {
				let plan = self.building_plan(prop.prop.as_ref().unwrap())?;
				for index in plan.for_stmt(stmt) {
					let placement = &plan.placements[index];
					let before: Vec<usize> = parts.iter().map(|p| p.faces.len()).collect();
					self.place(placement, parts, frame)?;
					let label = Rc::new(Stmt::new("use", vec![placement.piece.clone(), placement.role.clone()], Ordered::new(), vec![], &stmt.file, stmt.line));
					let faces = added(parts, &before);
					self.steps.push(Step { stmt: label, note: format!("{} ({})", placement.piece, placement.role.replace("wall:", "wall ")), faces });
				}
				continue;
			}
			let before: Vec<usize> = parts.iter().map(|p| p.faces.len()).collect();
			self.run_plain(std::slice::from_ref(stmt), &env, parts, frame, 0, prop)?;
			if !stmt.name.is_empty() {
				env.insert(stmt.name.clone(), Value::Bounds(Rc::new(bounds(&stmt.name, parts, &before))));
			}
			if stmt.op == "size" || stmt.op == "card" {
				continue;
			}
			let faces = added(parts, &before);
			self.steps.push(Step { stmt: stmt.clone(), note: String::new(), faces });
		}
		Ok(())
	}

	fn run_plain(&mut self, body: &[Rc<Stmt>], env: &Env, parts: &mut Vec<Part>, frame: &M4, depth: usize, prop: &Ctx) -> CResult<()> {
		if depth > 16 {
			return Err(PartScriptError::new("use/at nested more than 16 deep (a def that uses itself needs a when= to stop it)", &prop.file, prop.line).into());
		}
		let mut env = env.clone();
		env.insert("__frame__".into(), Value::Frame(Rc::new(*frame)));
		// Python stopped at a fault before it drew anything after: the statement that raised it reports it
		if let Some(fault) = py::take_fault() {
			py::raise(fault);
			return Err(Fail::Value(String::new()));
		}
		for stmt in body {
			let result = self.statement(stmt, &mut env, parts, frame, depth, prop);
			let result = match py::take_fault() {
				Some(py::Fault::Value(message)) => Err(Fail::Value(message)),
				Some(py::Fault::Fatal(message)) => Err(Fail::Script(PartScriptError::fatal(message))),
				None => result,
			};
			match result {
				Ok(()) => {}
				Err(Fail::Value(message)) => return Err(PartScriptError::new(format!("{}: {message}", stmt.op), &stmt.file, stmt.line).into()),
				Err(other) => return Err(other),
			}
		}
		Ok(())
	}

	fn statement(&mut self, stmt: &Rc<Stmt>, env: &mut Env, parts: &mut Vec<Part>, frame: &M4, depth: usize, prop: &Ctx) -> CResult<()> {
		let (op, a) = (stmt.op.as_str(), &stmt.args);
		let o = self.placing(stmt);
		match op {
			"set" => {
				for (k, v) in o.iter() {
					let value = arg_value(v, env);
					env.insert(k.to_string(), value);
				}
				return Ok(());
			}
			"size" | "card" => return Ok(()),
			_ => {}
		}
		if BUILD_OPS.contains(&op) {
			if prop.kind != "building" {
				return Err(PartScriptError::new(format!("'{op}' belongs in a building, not a prop"), &stmt.file, stmt.line).into());
			}
			if depth == 0 {
				let plan = self.building_plan(prop.prop.as_ref().unwrap())?;
				for index in plan.for_stmt(stmt) {
					self.place(&plan.placements[index], parts, frame)?;
				}
			}
			return Ok(());
		}
		match op {
			"snap" => {
				if self.snap_sink.is_some() {
					let copies = self.copies(stmt, env, false)?;
					self.record_snaps(stmt, env, frame, &copies)?;
				}
				return Ok(());
			}
			"link" => {
				let copies = self.copies(stmt, env, false)?;
				for (index, matrix) in copies.iter().enumerate() {
					let cenv = self.copy_env(stmt, env, index)?;
					if let Some(when) = o.get("when") {
						if !evaluate_number(when, &cenv)?.truthy() {
							continue;
						}
					}
					let mut number = |text: &str| evaluate(text, &cenv);
					let direction = pb::parse_dir(arg(a, 2)?, &mut number)?;
					let at = self.vec(arg(a, 1)?, &cenv, 3)?;
					let end = Snap::new("link", [at[0], at[1], at[2]], direction, pb::default_up(direction), arg(a, 0)?);
					self.links.push(end.moved(&frame.mul(matrix).0)?);
				}
				return Ok(());
			}
			"join" => {
				self.join_rules.push((stmt.clone(), env.clone()));
				return Ok(());
			}
			"part" => {
				let name = a.first().cloned().unwrap_or_else(|| format!("{}_{}", prop.name, parts.len()));
				let mut part = self.new_part(&name);
				part.smooth = matches!(o.get("smooth").map(String::as_str), Some("1") | Some("true"));
				parts.push(part);
				return Ok(());
			}
			_ => {}
		}
		let marks: Vec<usize> = parts.iter().map(|p| p.faces.len()).collect();
		let place = if o.contains("on") { frame.mul(&self.on_matrix(stmt, env)?) } else { *frame };
		let copies = self.copies(stmt, env, false)?;
		let mut ground =
			if o.contains("drop") { Some(SurfaceIndex::new(parts.iter().flat_map(|p| p.faces.iter().map(|f| &f.points)), 0.5)) } else { None };
		let against = if o.contains("on") { self.against(stmt, env, frame)? } else { None };
		for (index, matrix) in copies.iter().enumerate() {
			let mut cenv = self.copy_env(stmt, env, index)?;
			let m = place.mul(matrix);
			let here = [m.0[0][3], m.0[1][3], m.0[2][3]];
			cenv.insert("here".into(), Value::Bounds(Rc::new(Bounds::new("here", Some(here), Some(here), vec![], true))));
			if let Some(when) = o.get("when") {
				if !evaluate_number(when, &cenv)?.truthy() {
					continue;
				}
			}
			let before: Vec<usize> = parts.iter().map(|p| p.faces.len()).collect();
			self.emit(stmt, &cenv, parts, &m, depth, prop, index)?;
			if let Some((point, normal)) = against {
				touch(&mut since_mut(parts, &before), point, normal);
			}
			if let Some(ground) = &mut ground {
				let sink = match o.get("sink") {
					Some(s) => evaluate(s, &cenv)?,
					None => 0.0,
				};
				drop_onto(&mut since_mut(parts, &before), ground, o.get("drop").map(String::as_str) == Some("lean"), sink);
			}
			if self.fallen.is_some() {
				self.rubble(stmt, &cenv, parts, &m)?;
			}
		}
		if !stmt.name.is_empty() {
			env.insert(stmt.name.clone(), Value::Bounds(Rc::new(bounds(&stmt.name, parts, &marks))));
		}
		Ok(())
	}

	/// A line's options, less any a def it uses takes as its own parameters.
	fn placing(&self, stmt: &Stmt) -> Ordered<String> {
		if stmt.op != "use" || stmt.args.is_empty() {
			return stmt.opts.clone();
		}
		match self.program.macro_(&stmt.args[0], &stmt.file) {
			Some(m) if PLACING.iter().any(|k| m.params.contains(k)) => {
				stmt.opts.iter().filter(|(k, _)| !(PLACING.contains(k) && m.params.contains(k))).map(|(k, v)| (k.to_string(), v.clone())).collect()
			}
			_ => stmt.opts.clone(),
		}
	}

	/// One copy of a line placed by m, then reshaped as its options say.
	#[allow(clippy::too_many_arguments)]
	fn emit(&mut self, stmt: &Rc<Stmt>, env: &Env, parts: &mut Vec<Part>, m: &M4, depth: usize, prop: &Ctx, index: usize) -> CResult<()> {
		let o = &stmt.opts;
		let before: Vec<usize> = parts.iter().map(|p| p.faces.len()).collect();
		self.one(stmt, env, parts, m, depth, prop, index)?;
		if o.contains("twist") || o.contains("bend") || o.contains("shrink") {
			let twist = evaluate(o.get("twist").map(String::as_str).unwrap_or("0"), env)?;
			let bend = match o.get("bend") {
				Some(b) if b.contains(',') => {
					let v = self.vec(b, env, 2)?;
					(v[0], v[1])
				}
				Some(b) => (evaluate(b, env)?, 0.0),
				None => (0.0, 0.0),
			};
			let shrink = evaluate(o.get("shrink").map(String::as_str).unwrap_or("1"), env)?;
			deform(&mut since_mut(parts, &before), twist, bend, shrink);
		}
		if let Some(w) = o.get("wobble") {
			let amount = self.vec(w, env, 3)?;
			let seed = env.get("__seed__").map(Value::text).ok_or("'__seed__'")?;
			wobble(&mut since_mut(parts, &before), [amount[0], amount[1], amount[2]], &seed);
		}
		if let Some(f) = o.get("fade") {
			let low = evaluate(f, env)?;
			fade(&mut since_mut(parts, &before), low);
		}
		Ok(())
	}

	/// on=desk or on=desk.front: (the named shape, the side).
	fn on_target(&self, stmt: &Stmt, env: &Env) -> CResult<(Rc<Bounds>, String)> {
		let on = stmt.opt("on").unwrap();
		let (name, side) = on.split_once('.').unwrap_or((on, ""));
		let Some(Value::Bounds(target)) = env.get(name) else {
			return Err(PartScriptError::new(format!("on={on}: no shape of that name above this line (name one: {name} = box ...)"), &stmt.file, stmt.line).into());
		};
		if !side.is_empty() && side_normal(side).is_none() {
			let names: Vec<&str> = SIDES.iter().map(|(s, _)| *s).collect();
			return Err(PartScriptError::new(format!("on={on}: a side is {} (on={name} is on its top)", names.join(", ")), &stmt.file, stmt.line).into());
		}
		Ok((target.clone(), if side.is_empty() { "top".into() } else { side.to_string() }))
	}

	fn on_matrix(&self, stmt: &Stmt, env: &Env) -> CResult<M4> {
		let (target, side) = self.on_target(stmt, env)?;
		Ok(M4::translation(target.point(&side, env_frame(env).as_deref())?))
	}

	/// on=NAME.SIDE (not the top): the side's plane in prop space, (a point on it, its outward normal).
	fn against(&self, stmt: &Stmt, env: &Env, frame: &M4) -> CResult<Option<(V3, V3)>> {
		let (target, side) = self.on_target(stmt, env)?;
		if side == "top" {
			return Ok(None);
		}
		let point = frame.point(target.point(&side, env_frame(env).as_deref())?);
		let normal = side_normal(&side).unwrap();
		let turned = normalized(frame.to_3x3().apply(normal));
		Ok(Some((point, turned)))
	}

	/// row x|y|z|-x... { lines }: every copy of every line inside set end to end along the axis.
	fn row(&mut self, stmt: &Rc<Stmt>, env: &Env, parts: &mut [Part], m: &M4, depth: usize, prop: &Ctx) -> CResult<()> {
		let (a, o) = (&stmt.args, &stmt.opts);
		let axis = a.first().cloned().unwrap_or_else(|| "x".into());
		let bare = axis.trim_start_matches(['+', '-']);
		if !matches!(bare, "x" | "y" | "z") {
			return Err(PartScriptError::new(
				format!("row {axis}: the axis it runs along, x, y or z (-x runs the other way); stack is row z"),
				&stmt.file,
				stmt.line,
			)
			.into());
		}
		let k = "xyz".find(axis.chars().last().unwrap()).unwrap();
		let sign = if axis.starts_with('-') { -1.0 } else { 1.0 };
		let mut local = match o.get("at") {
			Some(at) => M4::translation(self.vec3(at, env)?),
			None => M4::IDENTITY,
		};
		local = local.mul(&rot(match o.get("r") {
			Some(r) => self.vec3(r, env)?,
			None => [0.0; 3],
		})
		.to_4x4());
		let frame = m.mul(&local);
		let mut inner = env.clone();
		inner.insert("__frame__".into(), Value::Frame(Rc::new(frame)));
		type Item = (Vec<Face>, V3, V3, (usize, usize), (usize, usize));
		let mut items: Vec<Item> = Vec::new();
		for child in stmt.block.iter().flatten() {
			if child.op == "set" {
				for (key, value) in child.opts.iter() {
					let v = arg_value(value, &inner);
					inner.insert(key.to_string(), v);
				}
				continue;
			}
			if matches!(child.op.as_str(), "part" | "snap" | "size" | "card" | "join" | "link" | "chain") || BUILD_OPS.contains(&child.op.as_str()) || !child.name.is_empty() {
				let what = if child.name.is_empty() { format!("'{}'", child.op) } else { format!("{} = ...", child.name) };
				return Err(PartScriptError::new(format!("{what} can't go in a row or stack (name the row itself: books = row x ...)"), &child.file, child.line).into());
			}
			let place = if child.opts.contains("on") { self.on_matrix(child, &inner)? } else { M4::IDENTITY };
			let copies = self.copies(child, &inner, true)?;
			for (index, matrix) in copies.iter().enumerate() {
				let cenv = self.copy_env(child, &inner, index)?;
				if let Some(when) = child.opts.get("when") {
					if !evaluate_number(when, &cenv)?.truthy() {
						continue;
					}
				}
				let mut scratch = vec![self.new_part("row")];
				let (links, joints) = (self.links.len(), self.joints.len());
				self.emit(child, &cenv, &mut scratch, &place.mul(matrix), depth + 1, prop, index)?;
				let faces: Vec<Face> = scratch.into_iter().flat_map(|p| p.faces).collect();
				if !faces.is_empty() {
					let (lo, hi) = bounds_of(faces.iter());
					items.push((faces, lo, hi, (links, self.links.len()), (joints, self.joints.len())));
				}
			}
		}
		if items.is_empty() {
			return Ok(());
		}
		let sizes: Vec<f64> = items.iter().map(|(_, lo, hi, _, _)| hi[k] - lo[k]).collect();
		let mut gap = evaluate(o.get("gap").map(String::as_str).unwrap_or("0"), env)?;
		if let Some(over) = o.get("over") {
			if items.len() > 1 {
				gap = (evaluate(over, env)? - py::sum(sizes.iter().copied())) / (items.len() - 1) as f64;
			}
		}
		let total = py::sum(sizes.iter().copied()) + gap * (items.len() - 1) as f64;
		let pack = o.get("pack").cloned().unwrap_or_else(|| if k == 2 { "start".into() } else { "centre".into() });
		if !matches!(pack.as_str(), "start" | "centre" | "center" | "end") {
			return Err(PartScriptError::new(format!("pack={pack}: start, centre or end (where the row sits on its at=)"), &stmt.file, stmt.line).into());
		}
		let mut cursor = match pack.as_str() {
			"start" => 0.0,
			"end" => -total,
			_ => -total / 2.0,
		};
		let edge = |word: &str| -> Option<(usize, usize)> {
			Some(match word {
				"left" => (0, 0),
				"right" => (0, 1),
				"front" => (1, 0),
				"back" => (1, 1),
				"bottom" => (2, 0),
				"top" => (2, 1),
				_ => return None,
			})
		};
		let align: Vec<String> = o.get("align").map(|s| s.split(',').filter(|w| !w.is_empty()).map(str::to_string).collect()).unwrap_or_default();
		for word in &align {
			if edge(word).is_none() && word != "centre" && word != "center" {
				return Err(PartScriptError::new(
					format!("align={word}: left right front back bottom top or centre, joined by commas (align=back,bottom)"),
					&stmt.file,
					stmt.line,
				)
				.into());
			}
		}
		for ((faces, lo, hi, links, joints), size) in items.into_iter().zip(sizes) {
			let mut shift = [0.0; 3];
			shift[k] = if sign > 0.0 { cursor - lo[k] } else { -cursor - hi[k] };
			cursor += size + gap;
			for word in &align {
				let axes: Vec<usize> = match edge(word) {
					Some((j, _)) => vec![j],
					None => vec![0, 1, 2],
				};
				for j in axes {
					if j != k {
						shift[j] = match edge(word) {
							Some((_, which)) => -(if which == 0 { lo[j] } else { hi[j] }),
							None => -(lo[j] + hi[j]) / 2.0,
						};
					}
				}
			}
			let where_ = frame.mul(&M4::translation(shift));
			append(parts.last_mut().unwrap(), &faces, &where_, &empty_origin());
			let moved_links = moved(&self.links[links.0..links.1], &where_)?;
			self.links.splice(links.0..links.1, moved_links);
			let moved_joints = moved(&self.joints[joints.0..joints.1], &where_)?;
			self.joints.splice(joints.0..joints.1, moved_joints);
		}
		Ok(())
	}

	/// The variables one copy of a line sees: its random seed and, on a line that makes copies, i.
	fn copy_env(&self, stmt: &Stmt, env: &Env, index: usize) -> CResult<Env> {
		let mut out = env.clone();
		let seed = env.get("__seed__").map(Value::text).unwrap_or_default();
		out.insert("__seed__".into(), Value::str(&format!("{seed}|{}:{index}", stmt.seed())));
		let auto = env.get("__auto_i__").is_some_and(|v| v.as_number().is_some_and(|n| n != 0.0) || matches!(v, Value::Str(s) if !s.is_empty()));
		if (!stmt.mods.is_empty() || stmt.opts.contains("along")) && (!env.contains_key("i") || auto) {
			out.insert("i".into(), Value::Int(index as i64));
			out.insert("__auto_i__".into(), Value::Int(1));
		}
		if let Some(names) = stmt.opt("index") {
			let names: Vec<&str> = names.split(',').collect();
			let mut found = None;
			for m in &stmt.mods {
				if let Some(Mod::Repeat(c, _)) = parse_mod(m) {
					found = Some(counts(&c, env)?);
					break;
				}
			}
			let mut cs = found.unwrap_or_else(|| vec![index as i64 + 1]);
			while cs.len() < 3 {
				cs.push(1);
			}
			let i = index as i64;
			let values: Vec<i64> = if names.len() > 1 {
				if cs[0] == 0 || cs[0] * cs[1] == 0 {
					return Err("integer modulo by zero".into());
				}
				vec![i.rem_euclid(cs[0]), (i.div_euclid(cs[0])).rem_euclid(cs[1].max(1)), i.div_euclid(cs[0] * cs[1])]
			} else {
				vec![i]
			};
			for (name, value) in names.iter().zip(values) {
				out.insert(name.to_string(), Value::Int(value));
			}
		}
		Ok(out)
	}

	/// One copy of a shape, use or group line, placed by m.
	#[allow(clippy::too_many_arguments)]
	fn one(&mut self, stmt: &Rc<Stmt>, env: &Env, parts: &mut Vec<Part>, m: &M4, depth: usize, prop: &Ctx, index: usize) -> CResult<()> {
		let (op, a, o) = (stmt.op.as_str(), &stmt.args, &stmt.opts);
		match op {
			"at" => {
				let mut local = match a.first() {
					Some(at) => M4::translation(self.vec3(at, env)?),
					None => M4::IDENTITY,
				};
				local = local.mul(&rot(match o.get("r") {
					Some(r) => self.vec3(r, env)?,
					None => [0.0; 3],
				})
				.to_4x4());
				if let Some(s) = o.get("s") {
					let v = self.vec3(s, env)?;
					local = local.mul(&M4::diagonal([v[0], v[1], v[2], 1.0]));
				}
				let block = stmt.block.clone().unwrap_or_default();
				self.run(&block, env, parts, &m.mul(&local), depth + 1, prop)
			}
			"use" => self.use_(stmt, env, parts, m, depth, prop, index),
			"chain" => self.chain(stmt, env, parts, m),
			"row" => self.row(stmt, env, parts, m, depth, prop),
			_ => {
				let mut scratch = self.new_part("scratch");
				match self.shape(stmt, env, &mut scratch, index) {
					Ok(()) => {}
					Err(Fail::Skip) => return Ok(()),
					Err(e) => return Err(e),
				}
				let path = self.path.clone();
				append(parts.last_mut().unwrap(), &scratch.faces, m, &path);
				Ok(())
			}
		}
	}

	pub fn vec(&self, text: &str, env: &Env, n: usize) -> CResult<Vec<f64>> {
		if n == 3 && !text.contains(',') {
			if let Some(point) = anchor_point(text, env)? {
				return Ok(point.to_vec());
			}
		}
		let mut parts = split_top(text, ',');
		if parts.len() == 1 {
			parts = vec![parts[0].clone(); n];
		}
		if parts.len() != n {
			return Err(format!("{}: expected {n} numbers", py::repr_str(text)).into());
		}
		parts.iter().map(|p| if p.starts_with('~') { self.height(p, env) } else { evaluate(p, env).map_err(Fail::from) }).collect()
	}

	fn vec3(&self, text: &str, env: &Env) -> CResult<V3> {
		let v = self.vec(text, env, 3)?;
		Ok([v[0], v[1], v[2]])
	}

	fn vec2(&self, text: &str, env: &Env) -> CResult<[f64; 2]> {
		let v = self.vec(text, env, 2)?;
		Ok([v[0], v[1]])
	}

	/// on / on(z) / on(NAME) where nothing has a size to sit: the height itself.
	fn height(&self, text: &str, env: &Env) -> CResult<f64> {
		let rest = &text[1..];
		if let Some(Value::Bounds(b)) = env.get(rest) {
			return Ok(b.number("top", env_frame(env).as_deref())?);
		}
		if rest.is_empty() {
			Ok(0.0)
		} else {
			Ok(evaluate(rest, env)?)
		}
	}

	/// A position whose ~ components sit the shape on that axis (base + half extent).
	fn centre(&self, text: &str, env: &Env, half: V3) -> CResult<V3> {
		let out = self.centre_all(text, env, half)?;
		if out.len() < 3 {
			return Err(format!("not enough values to unpack (expected 3, got {})", out.len()).into());
		}
		// a fourth number and on are passed over, as the original passed them over
		Ok([out[0], out[1], out[2]])
	}

	/// Every number of a position (centre() takes the first three).
	fn centre_all(&self, text: &str, env: &Env, half: V3) -> CResult<Vec<f64>> {
		if !text.contains(',') {
			if let Some(point) = anchor_point(text, env)? {
				return Ok(point.to_vec());
			}
		}
		let mut parts = split_top(text, ',');
		if parts.len() == 1 {
			parts = vec![parts[0].clone(); 3];
		}
		let mut out = Vec::new();
		for (k, p) in parts.iter().enumerate() {
			if let Some(rest) = p.strip_prefix('~') {
				let low = match env.get(rest) {
					Some(Value::Bounds(b)) => b.number("top", env_frame(env).as_deref())?,
					_ if rest.is_empty() => 0.0,
					_ => evaluate(rest, env)?,
				};
				out.push(low + half.get(k).copied().ok_or("tuple index out of range")?);
			} else {
				out.push(evaluate(p, env)?);
			}
		}
		Ok(out)
	}

	fn arc(&self, text: &str, env: &Env) -> CResult<Vec<V3>> {
		let values: Vec<f64> = split_top(text, ',').iter().map(|v| evaluate(v, env)).collect::<Result<_, _>>()?;
		if values.len() < 5 {
			return Err(format!("not enough values to unpack (expected 5, got {})", values.len()).into());
		}
		let (cx, cz, radius, a0, a1) = (values[0], values[1], values[2], values[3], values[4]);
		let steps = if values.len() > 5 { py_int(values[5])? } else { 12 };
		zero_steps(steps)?;
		Ok((0..=steps)
			.map(|k| {
				let angle = (a0 + (a1 - a0) * k as f64 / steps as f64).to_radians();
				[cx + radius * angle.py_cos(), 0.0, cz + radius * angle.py_sin()]
			})
			.collect())
	}

	/// along="P P P" [every=D] [fit=1] [corners=1] [closed=1]: where copies go along a line of points.
	fn path_frames(&self, stmt: &Stmt, env: &Env) -> CResult<Vec<Frame>> {
		let o = &stmt.opts;
		let along = o.get("along").unwrap();
		let line = env.get(along.as_str()).map(Value::text).unwrap_or_else(|| along.clone());
		let mut points: Vec<V3> = Vec::new();
		for p in crate::lang::words(&line) {
			if split_top(p, ',').len() == 3 {
				points.push(self.vec3(p, env)?);
			} else {
				let v = self.vec2(p, env)?;
				points.push([v[0], v[1], 0.0]);
			}
		}
		let points = self.smooth(points, o, env)?;
		let flag = |key: &str| matches!(o.get(key).map(String::as_str), Some("1") | Some("true"));
		let every = match o.get("every") {
			Some(e) => evaluate(e, env)?,
			None => 0.0,
		};
		let points: Vec<Vec<f64>> = points.iter().map(|p| p.to_vec()).collect();
		Ok(path_frames(&points, every, flag("fit"), flag("corners"), flag("closed"), flag("joints"))?)
	}

	/// chain A B C*3 ...: pieces end to end, each one's start snap on the last one's end snap.
	fn chain(&mut self, stmt: &Rc<Stmt>, env: &Env, parts: &mut [Part], m: &M4) -> CResult<()> {
		let o = &stmt.opts;
		let mut local = match o.get("at") {
			Some(at) => M4::translation(self.vec3(at, env)?),
			None => M4::IDENTITY,
		};
		local = local.mul(&rot(match o.get("r") {
			Some(r) => self.vec3(r, env)?,
			None => [0.0; 3],
		})
		.to_4x4());
		let frame = m.mul(&local);
		let mut names = Vec::new();
		for token in &stmt.args {
			let (name, count) = token.split_once('*').unwrap_or((token, ""));
			let n = if count.is_empty() { 1 } else { py_int(evaluate(count, env)?)? };
			for _ in 0..n.max(0) {
				names.push(name.to_string());
			}
		}
		if names.is_empty() {
			return Err(PartScriptError::new("chain needs pieces: chain path_straight path_curve*2 ...", &stmt.file, stmt.line).into());
		}
		let mut current = Snap::new("start", [0.0; 3], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], "any");
		let rows = frame.0;
		for name in names {
			let piece = self.piece(&name, Some(stmt))?;
			let mut snaps: Ordered<Snap> = Ordered::new();
			if !piece.faces.is_empty() {
				let (lo, hi) = bounds_of(piece.faces.iter());
				for (k, s) in pb::bounds_snaps(lo, hi).iter() {
					snaps.insert(k, s.clone());
				}
			}
			for (k, s) in piece.snaps.iter() {
				snaps.insert(k, s.clone());
			}
			let start = snaps.get("start").or_else(|| snaps.get("front")).cloned().ok_or("'front'")?;
			let end = snaps.get("end").or_else(|| snaps.get("back")).cloned().ok_or("'back'")?;
			if !pb::kinds_fit(&start.kind, &current.kind) {
				return Err(PartScriptError::new(
					format!("chain: {name}'s start is kind {}, the piece before ends in kind {}", start.kind, current.kind),
					&stmt.file,
					stmt.line,
				)
				.into());
			}
			let placed = pb::snap_matrix(&start, &current, 0.0, (0.0, 0.0))?;
			let where_ = frame.mul(&M4(placed));
			let origin = origin_plus(&self.path, &stmt.file, stmt.line);
			append(parts.last_mut().unwrap(), &piece.faces, &where_, &origin);
			let moved_links = moved(&piece.links, &where_)?;
			self.links.extend(moved_links);
			self.joints.push(current.moved(&rows)?);
			current = end.moved(&placed)?;
		}
		self.joints.push(current.moved(&rows)?);
		Ok(())
	}

	// ------------------------------------------------------------ links and joins
	fn open_links(&mut self) -> (Vec<Snap>, Vec<(Rc<Stmt>, Env)>) {
		(std::mem::take(&mut self.links), std::mem::take(&mut self.join_rules))
	}

	/// Join what this build's join lines can, put the outer build's links back, and return the ends left
	/// open (in this build's space).
	fn close_links(&mut self, scope: (Vec<Snap>, Vec<(Rc<Stmt>, Env)>), parts: &mut Vec<Part>) -> CResult<Vec<Snap>> {
		let joined = self.join(parts);
		let left = std::mem::replace(&mut self.links, scope.0);
		self.join_rules = scope.1;
		joined?;
		Ok(left)
	}

	/// Each join rule bridges pairs of open ends of its kind that nearly meet: nearest pairs first, each end
	/// once, ends that already touch counted as joined.
	fn join(&mut self, parts: &mut Vec<Part>) -> CResult<()> {
		let rules = self.join_rules.clone();
		for (stmt, env) in rules {
			let kind = stmt.args.first().cloned().unwrap_or_default();
			let reach = evaluate(stmt.opt("reach").unwrap_or(".6"), &env)?;
			let ends: Vec<usize> = (0..self.links.len()).filter(|&k| self.links[k].kind == kind).collect();
			let mut done: HashSet<usize> = HashSet::new();
			let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
			for (n, &a) in ends.iter().enumerate() {
				for &b in &ends[n + 1..] {
					let (pa, pb_) = (self.links[a].pos, self.links[b].pos);
					let gap = py::dist(&pa, &pb_);
					if gap < 0.005 {
						done.insert(a);
						done.insert(b);
						continue;
					}
					let towards = sub(pb_, pa);
					let da = self.links[a].dir;
					let db = self.links[b].dir;
					let facing = py::sum((0..3).map(|i| da[i] * towards[i])) >= -0.1 * gap && py::sum((0..3).map(|i| -db[i] * towards[i])) >= -0.1 * gap;
					if gap <= reach && facing {
						pairs.push((gap, a, b));
					}
				}
			}
			pairs.sort_by(|x, y| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal));
			for (gap, a, b) in pairs {
				if done.contains(&a) || done.contains(&b) {
					continue;
				}
				done.insert(a);
				done.insert(b);
				let (la, lb) = (self.links[a].clone(), self.links[b].clone());
				self.bridge(&stmt, &env, parts, &la, &lb, gap)?;
				self.joints.push(la);
				self.joints.push(lb);
			}
			let kept: Vec<Snap> = self.links.iter().enumerate().filter(|(k, _)| !done.contains(k)).map(|(_, e)| e.clone()).collect();
			self.links = kept;
		}
		Ok(())
	}

	fn bridge(&mut self, stmt: &Rc<Stmt>, env: &Env, parts: &mut Vec<Part>, a: &Snap, b: &Snap, gap: f64) -> CResult<()> {
		let o = &stmt.opts;
		let origin = origin_plus(&self.path, &stmt.file, stmt.line);
		if let Some(with) = o.get("with") {
			let Some(m) = self.program.macro_(with, &stmt.file) else {
				return Err(PartScriptError::new(
					format!("join with={with}: no def of that name (it draws the bridge along +X, length metres long)"),
					&stmt.file,
					stmt.line,
				)
				.into());
			};
			let [dx, dy, dz] = sub(b.pos, a.pos);
			let (heading, pitch) = (dy.atan2(dx), dz.atan2(py::hypot2(dx, dy)));
			let frame = M4::translation(a.pos).mul(&M4::rotation_named(heading, 'Z')).mul(&M4::rotation_named(-pitch, 'Y'));
			let mut env = env.clone();
			for (k, v) in self.program.env_of(&m.file) {
				env.insert(k, v);
			}
			let mut inner = env.clone();
			for (k, v) in m.params.iter() {
				inner.insert(k.to_string(), arg_value(v, &env));
			}
			inner.insert("length".into(), Value::Num(gap));
			let saved = std::mem::replace(&mut self.path, origin);
			let run = self.run(&m.body, &inner, parts, &frame, 1, &Ctx::of_macro(&m.name, &m.file, m.line));
			self.path = saved;
			return run;
		}
		let radius = evaluate(o.get("radius").map(String::as_str).unwrap_or(".025"), env)?;
		let sides = py_int(evaluate(o.get("s").map(String::as_str).unwrap_or("6"), env)?)?;
		let bulge = match o.get("bulge") {
			Some(b) => evaluate(b, env)?,
			None => py::max2(0.06, py::min2(0.2, gap * 0.5)),
		};
		let material = self.material(stmt, o.get("mat").map(String::as_str).unwrap_or("steel_dark"), env, 0)?;
		let path = [a.pos, add(a.pos, scale(a.dir, bulge)), add(b.pos, scale(b.dir, bulge)), b.pos];
		let profile: Vec<[f64; 2]> = (0..sides)
			.map(|k| {
				let angle = std::f64::consts::TAU * k as f64 / sides as f64;
				[radius * angle.py_cos(), radius * angle.py_sin()]
			})
			.collect();
		let mut scratch = self.new_part("bridge");
		scratch.sweep(&profile, &smooth_points(&path, 5, false), &material, false, true, [0.0, 0.0, 1.0], 0.0, None);
		append(parts.last_mut().unwrap(), &scratch.faces, &M4::IDENTITY, &origin);
		Ok(())
	}

	/// count offsets scattered over an area: *N~R (a disc), *N~W,D (a rectangle), *N~W,D,GAP.
	fn scatter(&self, stmt: &Stmt, env: &Env, count: i64, spec: &str) -> CResult<Vec<V3>> {
		let values: Vec<f64> = split_top(spec, ',').iter().map(|v| evaluate(v, env)).collect::<Result<_, _>>()?;
		let width = values[0];
		let depth = values.get(1).copied().unwrap_or(0.0);
		let gap = values.get(2).copied().unwrap_or(0.0);
		let disc = depth <= 0.0;
		let seed = env.get("__seed__").map(Value::text).unwrap_or_default();
		let mut rng = PyRandom::from_str(&format!("{seed}|{}:scatter", stmt.seed()));
		let mut out: Vec<V3> = Vec::new();
		for _ in 0..count.max(0) {
			let mut point = [0.0; 3];
			for _attempt in 0..30 {
				point = if disc {
					let angle = rng.uniform(0.0, std::f64::consts::TAU);
					let radius = width * rng.random().sqrt();
					[radius * angle.py_cos(), radius * angle.py_sin(), 0.0]
				} else {
					[rng.uniform(-width / 2.0, width / 2.0), rng.uniform(-depth / 2.0, depth / 2.0), 0.0]
				};
				if out.iter().all(|q| py::hypot2(point[0] - q[0], point[1] - q[1]) >= gap) {
					break;
				}
			}
			out.push(point);
		}
		Ok(out)
	}

	/// scatter N on NAME [facing ...] [apart G]: N places over the faces of the shape named NAME.
	fn spread(&self, stmt: &Stmt, env: &Env, count: i64, spec: &str) -> CResult<Vec<M4>> {
		let (name, gap) = spec.split_once(',').unwrap_or((spec, ""));
		let target = match env.get(name) {
			Some(Value::Bounds(b)) if !b.faces.is_empty() => b.clone(),
			_ => {
				return Err(PartScriptError::new(format!("scatter on {name}: no shape of that name above this line (name one: {name} = box ...)"), &stmt.file, stmt.line)
					.into())
			}
		};
		let facing = stmt.opt("facing").unwrap_or("any");
		if !matches!(facing, "up" | "down" | "side" | "any") {
			return Err(PartScriptError::new(format!("facing={facing}: up, side, down or any (which faces of {name} it grows on)"), &stmt.file, stmt.line).into());
		}
		let seed = env.get("__seed__").map(Value::text).unwrap_or_default();
		let mut rng = PyRandom::from_str(&format!("{seed}|{}:on", stmt.seed()));
		let apart = if gap.is_empty() { 0.0 } else { evaluate(gap, env)? };
		let found = spread(target.faces.iter().map(|f| &f.points), count.max(0) as usize, &mut rng, facing, apart);
		let inverse = match env_frame(env) {
			Some(frame) => frame.inverted().ok_or("matrix has no inverse")?,
			None => M4::IDENTITY,
		};
		let mut out = Vec::new();
		for (point, normal) in found {
			let n = normalized(inverse.to_3x3().apply(normal));
			let axis = cross([0.0, 0.0, 1.0], n);
			let turn = if length(axis) > 1e-6 {
				M4::rotation_axis(py::max2(-1.0, py::min2(1.0, n[2])).acos(), normalized(axis))
			} else if n[2] > 0.0 {
				M4::IDENTITY
			} else {
				M4::rotation_named(std::f64::consts::PI, 'X')
			};
			out.push(M4::translation(inverse.point(point)).mul(&turn));
		}
		Ok(out)
	}

	/// The chunks a broken box lost, some of them (rubble=share) as rubble fallen round its foot.
	fn rubble(&mut self, stmt: &Stmt, env: &Env, parts: &mut [Part], m: &M4) -> CResult<()> {
		let (local, size, fallen) = self.fallen.take().unwrap();
		let share = evaluate(stmt.opt("rubble").unwrap_or("0"), env)?;
		if share <= 0.0 || fallen.is_empty() {
			return Ok(());
		}
		let m = m.mul(&local);
		let seed = env.get("__seed__").map(Value::text).unwrap_or_default();
		let mut rng = PyRandom::from_str(&format!("{seed}|rubble"));
		let take = (fallen.len()).min((py::round(fallen.len() as f64 * py::min2(share, 1.0)) as usize).max(1)).min(120);
		let pieces = rng.sample(&fallen, take);
		let core = stmt.opt("core").map(str::to_string).unwrap_or_else(|| stmt.args.get(2).cloned().unwrap_or_default());
		let material = match self.material(stmt, &core, env, 0) {
			Err(Fail::Skip) => return Ok(()),
			other => other?,
		};
		let mut ground = SurfaceIndex::new(parts.iter().flat_map(|p| p.faces.iter().map(|f| &f.points)), 0.5);
		for ([x, y, z], cell) in pieces {
			let side = *rng.choice(&[-1.0, 1.0]);
			let sx = x + rng.uniform(-0.3, 0.3) * size[2];
			let sy = side * (size[1] / 2.0 + rng.uniform(0.05, 0.3 + size[2] * 0.35));
			let spot = [sx, sy, -size[2] / 2.0 + 0.4];
			let mut lump = self.new_part("rubble");
			let turn = [rng.uniform(0.0, std::f64::consts::TAU), rng.uniform(0.0, std::f64::consts::TAU), rng.uniform(0.0, std::f64::consts::TAU)];
			lump.push_at(spot, turn);
			let dims = [cell[0] * rng.uniform(0.45, 0.8), cell[1] * rng.uniform(0.45, 0.8), cell[2] * rng.uniform(0.45, 0.8)];
			lump.simple_box([0.0; 3], dims, &material);
			lump.pop();
			let mut refs: Vec<&mut Face> = lump.faces.iter_mut().collect();
			wobble(&mut refs, [cell[0] * 0.12, cell[1] * 0.12, cell[2] * 0.12], &format!("{seed}|{},{},{}", py::repr(x), py::repr(y), py::repr(z)));
			let mut faces: Vec<Face> = lump
				.faces
				.iter()
				.map(|f| Face { points: f.points.iter().map(|q| m.point(*q)).collect(), origin: self.path.clone(), ..f.clone() })
				.collect();
			let mut refs: Vec<&mut Face> = faces.iter_mut().collect();
			drop_onto(&mut refs, &mut ground, true, 0.0);
			parts.last_mut().unwrap().faces.extend(faces);
		}
		Ok(())
	}

	/// Where each copy goes. still: repeat N without every= makes N copies in one place.
	fn copies(&self, stmt: &Stmt, env: &Env, still: bool) -> CResult<Vec<M4>> {
		let mut mats = vec![M4::IDENTITY];
		for token in &stmt.mods {
			match parse_mod(token) {
				Some(Mod::Repeat(count_text, step_text)) => {
					let cs = counts(&count_text, env)?;
					let offsets: Vec<V3> = if cs.len() == 1 {
						let step = match &step_text {
							Some(s) => self.vec3(s, env)?,
							None if still => [0.0; 3],
							None => [1.0, 0.0, 0.0],
						};
						let mut offsets: Vec<V3> = (0..cs[0]).map(|k| step.map(|s| s * k as f64)).collect();
						let hang = match stmt.opt("hang") {
							Some(h) => evaluate(h, env)?,
							None => 0.0,
						};
						if hang != 0.0 && cs[0] > 1 {
							let n = cs[0] as f64;
							offsets = offsets
								.iter()
								.enumerate()
								.map(|(k, [x, y, z])| [*x, *y, z - hang * (1.0 - (2.0 * k as f64 / (n - 1.0) - 1.0).powf(2.0))])
								.collect();
						}
						offsets
					} else {
						let step = match &step_text {
							Some(s) if split_top(s, ',').len() == 3 => self.vec3(s, env)?,
							Some(s) => {
								let v = self.vec2(s, env)?;
								[v[0], v[1], 0.0]
							}
							None if still => [0.0; 3],
							None => [1.0, 1.0, 1.0],
						};
						let mut cs = cs.clone();
						while cs.len() < 3 {
							cs.push(1);
						}
						let mut out = Vec::new();
						for k in 0..cs[2] {
							for j in 0..cs[1] {
								for i in 0..cs[0] {
									out.push([i as f64 * step[0], j as f64 * step[1], k as f64 * step[2]]);
								}
							}
						}
						out
					};
					mats = offsets.iter().flat_map(|off| mats.iter().map(move |m| M4::translation(*off).mul(m))).collect();
				}
				Some(Mod::Ring(count_text, deg_text)) => {
					let n = counts(&count_text, env)?[0];
					let deg = evaluate(&deg_text, env)?;
					mats = (0..n).flat_map(|k| mats.iter().map(move |m| M4::rotation_named((deg * k as f64).to_radians(), 'Z').mul(m))).collect();
				}
				Some(Mod::Scatter(count_text, spec)) => {
					let n = counts(&count_text, env)?[0];
					let offsets = self.scatter(stmt, env, n, &spec)?;
					mats = offsets.iter().flat_map(|off| mats.iter().map(move |m| M4::translation(*off).mul(m))).collect();
				}
				Some(Mod::On(count_text, spec)) => {
					let n = counts(&count_text, env)?[0];
					let spots = self.spread(stmt, env, n, &spec)?;
					mats = spots.iter().flat_map(|spot| mats.iter().map(move |m| spot.mul(m))).collect();
				}
				None => {
					if let Some(axis) = token.strip_prefix('m').and_then(|a| "xyz".find(a)) {
						let mut s = [1.0, 1.0, 1.0, 1.0];
						s[axis] = -1.0;
						let mirror = M4::diagonal(s);
						let extra: Vec<M4> = mats.iter().map(|m| mirror.mul(m)).collect();
						mats.extend(extra);
					}
				}
			}
		}
		if stmt.opts.contains("along") {
			let mut placed = Vec::new();
			for frame in self.path_frames(stmt, env)? {
				let mut mv = M4::translation(frame.pos).mul(&M4::rotation_named(frame.yaw.to_radians(), 'Z'));
				if frame.stretch != 1.0 {
					mv = mv.mul(&M4::diagonal([frame.stretch, 1.0, 1.0, 1.0]));
				}
				placed.extend(mats.iter().map(|m| mv.mul(m)));
			}
			mats = placed;
		}
		if let Some(jit) = stmt.opt("jit") {
			let values: Vec<f64> = split_top(jit, ',').iter().map(|v| evaluate(v, env)).collect::<Result<_, _>>()?;
			let (amount, spin) = (values[0], values.get(1).copied().unwrap_or(0.0));
			let seed = env.get("__seed__").map(Value::text).unwrap_or_default();
			let mut rng = PyRandom::from_str(&format!("{seed}{}", stmt.seed()));
			mats = mats
				.iter()
				.map(|m| {
					let t = M4::translation([rng.uniform(-amount, amount), rng.uniform(-amount, amount), 0.0]);
					let r = M4::rotation_named(rng.uniform(-spin, spin).to_radians(), 'Z');
					t.mul(m).mul(&r)
				})
				.collect();
		}
		Ok(mats)
	}

	fn shape(&mut self, stmt: &Stmt, env: &Env, p: &mut Part, copy: usize) -> CResult<()> {
		let (op, a, o) = (stmt.op.as_str(), &stmt.args, &stmt.opts);
		let rotation: V3 = match o.get("r") {
			Some(r) => self.vec3(r, env)?.map(f64::to_radians),
			None => [0.0; 3],
		};
		let mat = |token: &str| self.material(stmt, token, env, copy);
		let taper = if o.contains("taper") && matches!(op, "b" | "bb") { self.vec2(o.get("taper").unwrap(), env)? } else { [1.0, 1.0] };
		let lean = if o.contains("lean") && matches!(op, "b" | "bb") { self.vec2(o.get("lean").unwrap(), env)? } else { [0.0, 0.0] };
		let num = |text: &str| -> CResult<f64> { Ok(evaluate(text, env)?) };
		let flag = |key: &str| matches!(o.get(key).map(String::as_str), Some("1") | Some("true"));
		if matches!(op, "b" | "bb" | "c" | "cone") && o.contains("from") && o.contains("to") {
			return self.between(stmt, env, p, copy, rotation);
		}
		match op {
			"b" if o.contains("break") => {
				let size = self.vec3(arg(a, 1)?, env)?;
				let centre = self.centre(arg(a, 0)?, env, size.map(|v| v / 2.0))?;
				p.push_at(centre, rotation);
				let chunk = match o.get("chunk") {
					Some(c) => num(c)?,
					None => py::max2(py::max2(0.15, py::min2(py::min2(size[0], size[1]), size[2])), py::max2(py::max2(size[0], size[1]), size[2]) / 14.0),
				};
				let material = mat(arg(a, 2)?)?;
				let amount = num(o.get("break").unwrap())?;
				let seed = env.get("__seed__").map(Value::text).unwrap_or_default();
				let core = match o.get("core") {
					Some(c) => Some(mat(c)?),
					None => None,
				};
				let fallen = broken_box(p, size, &material, amount, chunk, &seed, core.as_deref())?;
				p.pop();
				let degrees = rotation.map(f64::to_degrees);
				self.fallen = Some((M4::translation(centre).mul(&rot(degrees).to_4x4()), size, fallen));
			}
			"b" => {
				let size = self.vec3(arg(a, 1)?, env)?;
				let skip: Vec<&str> = o.get("skip").map(|s| s.split(',').filter(|x| !x.is_empty()).collect()).unwrap_or_default();
				let centre = self.centre(arg(a, 0)?, env, size.map(|s| s / 2.0))?;
				let material = mat(arg(a, 2)?)?;
				let shade = match o.get("shade") {
					Some(s) => crate::expr::py_float(s).ok_or_else(|| format!("could not convert string to float: {}", py::repr_str(s)))?,
					None => 1.0,
				};
				p.box_(centre, size, &material, rotation, &skip, shade, taper, lean);
			}
			"bx" => {
				let (lo, hi) = (self.vec3(arg(a, 0)?, env)?, self.vec3(arg(a, 1)?, env)?);
				p.box_minmax(lo, hi, &mat(arg(a, 2)?)?);
			}
			"bb" => {
				let size = self.vec3(arg(a, 1)?, env)?;
				let centre = self.centre(arg(a, 0)?, env, size.map(|s| s / 2.0))?;
				let bevel = num(arg(a, 2)?)?;
				p.bevel_box(centre, size, bevel, &mat(arg(a, 3)?)?, rotation, taper, lean);
			}
			"ext" => {
				let mut outline = Vec::new();
				for point in a.iter().skip(3) {
					let values: Vec<f64> = point.split(':').map(|v| evaluate(v, env)).collect::<Result<_, _>>()?;
					if values.len() != 2 {
						return Err(format!("too many values to unpack (expected 2, got {})", values.len()).into());
					}
					outline.push([values[0], values[1]]);
				}
				let center = self.vec3(arg(a, 0)?, env)?;
				let width = num(arg(a, 1)?)?;
				let material = mat(arg(a, 2)?)?;
				let axis = o.get("ax").map(String::as_str).unwrap_or("x");
				let taper = match o.get("taper") {
					Some(t) => num(t)?,
					None => 1.0,
				};
				if axis.chars().count() != 1 {
					return Err("extrude axis: x, y or z".into());
				}
				p.extrude(center, &outline, width, &material, axis.chars().next().unwrap(), taper, rotation)?;
			}
			"c" | "cone" | "tube" => {
				let axis = o.get("ax").map(String::as_str).unwrap_or("z");
				let radius = num(arg(a, 1)?)?;
				let (inner, height, material) = if op == "tube" {
					(num(arg(a, 2)?)?, num(arg(a, 3)?)?, mat(arg(a, 4)?)?)
				} else {
					(0.0, num(arg(a, 2)?)?, mat(arg(a, 3)?)?)
				};
				let extent = match axis {
					"x" => [height / 2.0, radius, radius],
					"y" => [radius, height / 2.0, radius],
					"z" => [radius, radius, height / 2.0],
					other => return Err(format!("'{other}'").into()),
				};
				let centre = self.centre(arg(a, 0)?, env, extent)?;
				let axis_rot = match axis {
					"x" => [0.0, std::f64::consts::PI / 2.0, 0.0],
					"y" => [std::f64::consts::PI / 2.0, 0.0, 0.0],
					_ => [0.0; 3],
				};
				p.push_at(centre, rotation);
				p.push_at([0.0; 3], axis_rot);
				let default_sides = if op == "tube" { "12" } else if op == "c" { "10" } else { "8" };
				let sides_int = py_int(num(o.get("s").map(String::as_str).unwrap_or(default_sides))?)?;
				let sides = sides_int.max(0) as usize;
				if op == "tube" {
					let h = height / 2.0;
					zero_steps(sides_int)?;
					p.lathe(&[[inner, -h], [radius, -h], [radius, h], [inner, h], [inner, -h]], &material, sides, [0.0; 3], [0.0, std::f64::consts::TAU], false, [0.0; 3]);
				} else if op == "cone" {
					zero_steps(sides_int)?;
					p.cone([0.0; 3], radius, height, &material, sides, [0.0; 3]);
				} else {
					let arc = match o.get("arc") {
						Some(t) => self.vec2(t, env)?.map(f64::to_radians),
						None => [0.0, std::f64::consts::TAU],
					};
					let radius_top = match o.get("rt") {
						Some(t) => Some(num(t)?),
						None => None,
					};
					let cap = match o.get("capm") {
						Some(c) => Some(mat(c)?),
						None => None,
					};
					if (arc[1] - arc[0] - std::f64::consts::TAU).abs() < 1e-6 {
						zero_steps(sides_int)?;
					}
					p.cylinder([0.0; 3], radius, height, &material, sides, [0.0; 3], radius_top, o.get("caps").map(String::as_str) != Some("0"), cap.as_deref(), arc);
				}
				p.pop();
				p.pop();
			}
			"sph" => {
				let radius = num(arg(a, 1)?)?;
				let rings = py_int(num(o.get("rings").map(String::as_str).unwrap_or("6"))?)?;
				zero_steps(rings)?;
				let profile: Vec<[f64; 2]> = (0..=rings)
					.map(|k| {
						let angle = std::f64::consts::PI * k as f64 / rings as f64;
						[radius * angle.py_sin(), -radius * angle.py_cos()]
					})
					.collect();
				let material = mat(arg(a, 2)?)?;
				let sides = py_int(num(o.get("s").map(String::as_str).unwrap_or("10"))?)?;
				let center = self.centre(arg(a, 0)?, env, [radius; 3])?;
				zero_steps(sides)?;
				p.lathe(&profile, &material, sides.max(0) as usize, center, [0.0, std::f64::consts::TAU], false, rotation);
			}
			"wedge" => {
				let [w, d, h] = self.vec3(arg(a, 1)?, env)?;
				let all = self.centre_all(arg(a, 0)?, env, [w / 2.0, d / 2.0, h / 2.0])?;
				if all.len() != 3 {
					let kind = if all.len() < 3 { format!("not enough values to unpack (expected 3, got {})", all.len()) } else { "too many values to unpack (expected 3)".into() };
					return Err(kind.into());
				}
				let [cx, cy, cz] = [all[0], all[1], all[2]];
				let material = mat(arg(a, 2)?)?;
				p.push_at([cx, cy, cz], rotation);
				let (x, y, z) = (w / 2.0, d / 2.0, h / 2.0);
				p.plain(&[[-x, y, -z], [x, y, -z], [x, -y, -z], [-x, -y, -z]], &material);
				p.plain(&[[-x, y, -z], [-x, y, z], [x, y, z], [x, y, -z]], &material);
				p.plain(&[[-x, -y, -z], [x, -y, -z], [x, y, z], [-x, y, z]], &material);
				p.plain(&[[-x, -y, -z], [-x, y, z], [-x, y, -z]], &material);
				p.plain(&[[x, -y, -z], [x, y, -z], [x, y, z]], &material);
				p.pop();
			}
			"lathe" => {
				let mut profile = Vec::new();
				for point in a.iter().skip(2) {
					let values: Vec<f64> = point.split(':').map(|v| evaluate(v, env)).collect::<Result<_, _>>()?;
					if values.len() != 2 {
						return Err(format!("too many values to unpack (expected 2, got {})", values.len()).into());
					}
					profile.push([values[0], values[1]]);
				}
				let arc = match o.get("arc") {
					Some(t) => self.vec2(t, env)?.map(f64::to_radians),
					None => [0.0, std::f64::consts::TAU],
				};
				let material = mat(arg(a, 1)?)?;
				let sides = py_int(num(o.get("s").map(String::as_str).unwrap_or("16"))?)?;
				let center = self.vec3(arg(a, 0)?, env)?;
				zero_steps(sides)?;
				p.lathe(&profile, &material, sides.max(0) as usize, center, arc, flag("cap"), rotation);
			}
			"pipe" | "sweep" => {
				let (profile, from) = if op == "pipe" {
					let radius = num(arg(a, 1)?)?;
					let sides = py_int(num(o.get("s").map(String::as_str).unwrap_or("6"))?)?;
					let profile: Vec<[f64; 2]> = (0..sides)
						.map(|k| {
							let angle = std::f64::consts::TAU * k as f64 / sides as f64;
							[radius * angle.py_cos(), radius * angle.py_sin()]
						})
						.collect();
					(profile, 2)
				} else {
					let prof = o.get("prof").ok_or("'prof'")?;
					let mut profile = Vec::new();
					for point in prof.split(',') {
						let values: Vec<f64> = point.split(':').map(|v| evaluate(v, env)).collect::<Result<_, _>>()?;
						if values.len() != 2 {
							return Err(format!("not enough values to unpack (expected 2, got {})", values.len()).into());
						}
						profile.push([values[0], values[1]]);
					}
					(profile, 1)
				};
				let points = match o.get("arc") {
					Some(t) => self.arc(t, env)?,
					None => {
						let tokens = expand_points(&a[from.min(a.len())..], env);
						tokens.iter().map(|t| self.vec3(t, env)).collect::<CResult<Vec<_>>>()?
					}
				};
				let points = self.smooth(points, o, env)?;
				let up = if o.contains("arc") { [0.0, 1.0, 0.0] } else { [0.0, 0.0, 1.0] };
				let scales = match o.get("taper") {
					Some(t) => Some(taper_sizes(&points, num(t)?)),
					None => None,
				};
				let material = mat(arg(a, 0)?)?;
				if points.is_empty() {
					return Err("list index out of range".into());
				}
				let closed_profile = op == "pipe" || !flag("open");
				p.sweep(&profile, &points, &material, flag("closed"), closed_profile, up, 0.0, scales.as_deref());
			}
			"terrain" => self.terrain(stmt, env, p, copy)?,
			"archwall" => {
				let [cx, cy, cz] = self.vec3(arg(a, 0)?, env)?;
				let (width, height, thick) = (num(arg(a, 1)?)?, num(arg(a, 2)?)?, num(arg(a, 3)?)?);
				let material = mat(arg(a, 4)?)?;
				let span = match o.get("open") {
					Some(v) => num(v)?,
					None => width,
				};
				let spring = match o.get("spring") {
					Some(v) => num(v)?,
					None => height - span / 2.0,
				};
				let steps = py_int(num(o.get("s").map(String::as_str).unwrap_or("12"))?)?;
				zero_steps(steps)?;
				let (hw, ho) = (width / 2.0, span / 2.0);
				let (y0, y1) = (cy, cy + thick);
				let arc: Vec<(f64, f64)> = (0..=steps)
					.map(|k| {
						let angle = std::f64::consts::PI * k as f64 / steps as f64;
						(cx - ho * angle.py_cos(), cz + spring + ho * angle.py_sin())
					})
					.collect();
				let top = cz + height;
				for (y, flip) in [(y0, false), (y1, true)] {
					let mut quads: Vec<[(f64, f64); 4]> = Vec::new();
					if hw - ho > 0.001 {
						quads.push([(cx - hw, cz), (cx - ho, cz), (cx - ho, top), (cx - hw, top)]);
						quads.push([(cx + ho, cz), (cx + hw, cz), (cx + hw, top), (cx + ho, top)]);
					}
					for w in arc.windows(2) {
						let ((x0, z0), (x1, z1)) = (w[0], w[1]);
						quads.push([(x0, z0), (x1, z1), (x1, top), (x0, top)]);
					}
					for quad in quads {
						let mut points: Vec<V3> = quad.iter().map(|(x, z)| [*x, y, *z]).collect();
						if flip {
							points.reverse();
						}
						p.plain(&points, &material);
					}
				}
				for w in arc.windows(2) {
					let ((x0, z0), (x1, z1)) = (w[0], w[1]);
					p.plain(&[[x0, y0, z0], [x0, y1, z0], [x1, y1, z1], [x1, y0, z1]], &material);
				}
				for (x, sign) in [(cx - ho, 1), (cx + ho, -1)] {
					let mut jamb = vec![[x, y0, cz], [x, y1, cz], [x, y1, cz + spring], [x, y0, cz + spring]];
					if sign < 0 {
						jamb.reverse();
					}
					p.plain(&jamb, &material);
				}
				p.plain(&[[cx - hw, y0, top], [cx + hw, y0, top], [cx + hw, y1, top], [cx - hw, y1, top]], &material);
				for (x, sign) in [(cx - hw, -1), (cx + hw, 1)] {
					let mut end = vec![[x, y0, cz], [x, y0, top], [x, y1, top], [x, y1, cz]];
					if sign >= 0 {
						end.reverse();
					}
					p.plain(&end, &material);
				}
			}
			"vault" => {
				let [cx, cy, cz] = self.vec3(arg(a, 0)?, env)?;
				let (span, depth, rise) = (num(arg(a, 1)?)?, num(arg(a, 2)?)?, num(arg(a, 3)?)?);
				let material = mat(arg(a, 4)?)?;
				let steps = py_int(num(o.get("s").map(String::as_str).unwrap_or("12"))?)?;
				zero_steps(steps)?;
				let (half, y0, y1) = (span / 2.0, cy - depth / 2.0, cy + depth / 2.0);
				let ring: Vec<(f64, f64)> = (0..=steps)
					.map(|k| {
						let angle = std::f64::consts::PI * k as f64 / steps as f64;
						(cx - half * angle.py_cos(), cz + rise * angle.py_sin())
					})
					.collect();
				for w in ring.windows(2) {
					let ((x0, z0), (x1, z1)) = (w[0], w[1]);
					p.plain(&[[x0, y0, z0], [x0, y1, z0], [x1, y1, z1], [x1, y0, z1]], &material);
				}
			}
			"face" => {
				let points: Vec<V3> = a.iter().skip(1).map(|t| self.vec3(t, env)).collect::<CResult<_>>()?;
				let material = mat(arg(a, 0)?)?;
				p.plain(&points, &material);
				if flag("double") {
					if points.len() < 3 {
						return Err("list index out of range".into());
					}
					let n = cross(sub(points[1], points[0]), sub(points[2], points[0]));
					let n = if length(n) > 1e-9 { scale(normalized(n), 0.001) } else { [0.0; 3] };
					let back: Vec<V3> = points.iter().rev().map(|q| sub(*q, n)).collect();
					let material = mat(arg(a, 0)?)?;
					p.plain(&back, &material);
				}
			}
			"pan" => {
				let center = self.vec3(arg(a, 0)?, env)?;
				let (width, height) = (num(arg(a, 1)?)?, num(arg(a, 2)?)?);
				let material = mat(arg(a, 3)?)?;
				p.panel(center, width, height, &material, rotation, None, flag("double"));
			}
			"trim" => {
				let center = self.vec3(arg(a, 0)?, env)?;
				let (width, height) = (num(arg(a, 1)?)?, num(arg(a, 2)?)?);
				let material = match o.get("m") {
					Some(m) => Some(mat(m)?),
					None => None,
				};
				p.trim(center, width, height, arg(a, 3)?, &self.host.atlases(), rotation, material.as_deref(), false)?;
			}
			"sign" => {
				let spec = sign_spec(stmt, Some(env))?;
				let key = sign_key(&self.prefix, &spec);
				if !self.host.has_material(&key) {
					self.register_sign(&key, &spec);
				}
				let center = self.vec3(arg(a, 0)?, env)?;
				if let Some(wrap) = o.get("wrap") {
					let radius = num(wrap)?;
					let height = num(arg(a, 2)?)?;
					let sides = py_int(num(o.get("sides").map(String::as_str).unwrap_or("12"))?)?;
					p.wrapped_panel(center, radius, height, &key, sides.max(0) as usize, rotation)?;
				} else {
					let (width, height) = (num(arg(a, 1)?)?, num(arg(a, 2)?)?);
					p.panel(center, width, height, &key, rotation, None, flag("double"));
				}
			}
			"torus" => {
				let (radius, thick) = (num(arg(a, 1)?)?, num(arg(a, 2)?)?);
				let sides = py_int(num(o.get("s").map(String::as_str).unwrap_or("16"))?)?;
				let rings = py_int(num(o.get("rings").map(String::as_str).unwrap_or("6"))?)?;
				let profile: Vec<[f64; 2]> = (0..rings)
					.map(|k| {
						let angle = std::f64::consts::TAU * k as f64 / rings as f64;
						[thick * angle.py_cos(), thick * angle.py_sin()]
					})
					.collect();
				let ring: Vec<V3> = (0..sides)
					.map(|k| {
						let angle = std::f64::consts::TAU * k as f64 / sides as f64;
						[radius * angle.py_cos(), radius * angle.py_sin(), 0.0]
					})
					.collect();
				let axis = o.get("ax").map(String::as_str).unwrap_or("z");
				let axis_rot = match axis {
					"x" => [0.0, std::f64::consts::PI / 2.0, 0.0],
					"y" => [std::f64::consts::PI / 2.0, 0.0, 0.0],
					"z" => [0.0; 3],
					other => return Err(format!("'{other}'").into()),
				};
				let outer = radius + thick;
				let half = match axis {
					"z" => [outer, outer, thick],
					"x" => [thick, outer, outer],
					_ => [outer, thick, outer],
				};
				let centre = self.centre(arg(a, 0)?, env, half)?;
				let material = mat(arg(a, 3)?)?;
				p.push_at(centre, rotation);
				p.push_at([0.0; 3], axis_rot);
				p.sweep(&profile, &ring, &material, true, true, [0.0, 0.0, 1.0], 0.0, None);
				p.pop();
				p.pop();
			}
			"frame" => {
				let (width, height, depth) = (num(arg(a, 1)?)?, num(arg(a, 2)?)?, num(arg(a, 3)?)?);
				let material = mat(arg(a, 4)?)?;
				let [hw, hh] = match o.get("hole") {
					Some(h) => self.vec2(h, env)?,
					None => [width * 0.7, height * 0.7],
				};
				let [hx, hz] = match o.get("hole_at") {
					Some(h) => self.vec2(h, env)?,
					None => [0.0, 0.0],
				};
				let centre = self.centre(arg(a, 0)?, env, [width / 2.0, depth / 2.0, height / 2.0])?;
				p.push_at(centre, rotation);
				let (left, right, bottom, top) = (-width / 2.0, width / 2.0, -height / 2.0, height / 2.0);
				let (h0, h1, v0, v1) = (hx - hw / 2.0, hx + hw / 2.0, hz - hh / 2.0, hz + hh / 2.0);
				let bars: [(f64, f64, f64, f64, &[&str]); 4] =
					[(left, h0, bottom, top, &[]), (h1, right, bottom, top, &[]), (h0, h1, bottom, v0, &["-x", "+x"]), (h0, h1, v1, top, &["-x", "+x"])];
				for (x0, x1, z0, z1, skip) in bars {
					if x1 - x0 > 1e-6 && z1 - z0 > 1e-6 {
						let skip: &[&str] = if x0 > left + 1e-6 && x1 < right - 1e-6 { skip } else { &[] };
						p.box_([(x0 + x1) / 2.0, 0.0, (z0 + z1) / 2.0], [x1 - x0, depth, z1 - z0], &material, [0.0; 3], skip, 1.0, [1.0, 1.0], [0.0, 0.0]);
					}
				}
				p.pop();
			}
			other => return Err(PartScriptError::new(format!("unknown shape '{other}'"), &stmt.file, stmt.line).into()),
		}
		Ok(())
	}

	/// terrain at=C size=W,D mat=M height=EXPR [cells=N[,M]] [steep=M slope=35] [skirt=0]
	fn terrain(&self, stmt: &Stmt, env: &Env, p: &mut Part, copy: usize) -> CResult<()> {
		let (a, o) = (&stmt.args, &stmt.opts);
		let [cx, cy, cz] = self.vec3(arg(a, 0)?, env)?;
		let size = arg(a, 1)?;
		let [width, depth] = if size.contains(',') {
			self.vec2(size, env)?
		} else {
			let w = evaluate(size, env)?;
			[w, w]
		};
		let counts: Vec<i64> = split_top(o.get("cells").map(String::as_str).unwrap_or("16"), ',').iter().map(|v| -> CResult<i64> { py_int(evaluate(v, env)?) }).collect::<Result<_, _>>()?;
		let (nx, ny) = (counts[0], counts[counts.len() - 1]);
		if !((1..=96).contains(&nx) && (1..=96).contains(&ny)) {
			return Err(PartScriptError::new("terrain cells=N or N,M: 1-96 a side", &stmt.file, stmt.line).into());
		}
		let ground = self.material(stmt, arg(a, 2)?, env, copy)?;
		let steep = match o.get("steep") {
			Some(s) => Some(self.material(stmt, s, env, copy)?),
			None => None,
		};
		let limit = evaluate(o.get("slope").map(String::as_str).unwrap_or("35"), env)?.to_radians().py_cos();
		let height = o.get("height").map(String::as_str).unwrap_or("0");
		let mut corner_env = env.clone();
		let mut grid: Vec<Vec<V3>> = Vec::new();
		for j in 0..=ny {
			let mut row = Vec::new();
			for i in 0..=nx {
				let (x, y) = (-width / 2.0 + width * i as f64 / nx as f64, -depth / 2.0 + depth * j as f64 / ny as f64);
				corner_env.insert("x".into(), Value::Num(x));
				corner_env.insert("y".into(), Value::Num(y));
				row.push([cx + x, cy + y, cz + evaluate(height, &corner_env)?]);
			}
			grid.push(row);
		}
		let (nx, ny) = (nx as usize, ny as usize);
		for j in 0..ny {
			for i in 0..nx {
				let (a00, a10, a11, a01) = (grid[j][i], grid[j][i + 1], grid[j + 1][i + 1], grid[j + 1][i]);
				let pair = if (i + j) % 2 == 0 { [(a00, a10, a11), (a00, a11, a01)] } else { [(a00, a10, a01), (a10, a11, a01)] };
				for tri in pair {
					let u = sub(tri.1, tri.0);
					let v = sub(tri.2, tri.0);
					let nz = u[0] * v[1] - u[1] * v[0];
					let mut length = ((u[1] * v[2] - u[2] * v[1]).powf(2.0) + (u[2] * v[0] - u[0] * v[2]).powf(2.0) + nz.powf(2.0)).sqrt();
					if length == 0.0 {
						length = 1.0;
					}
					let material = match &steep {
						Some(s) if nz / length < limit => s,
						_ => &ground,
					};
					p.plain(&[tri.0, tri.1, tri.2], material);
				}
			}
		}
		if !matches!(o.get("skirt").map(String::as_str).unwrap_or("1"), "0" | "false") {
			let side = steep.as_ref().unwrap_or(&ground);
			let edges: [Vec<V3>; 4] = [
				grid[0].clone(),
				grid.iter().map(|row| row[row.len() - 1]).collect(),
				grid[grid.len() - 1].iter().rev().copied().collect(),
				grid.iter().map(|row| row[0]).rev().collect(),
			];
			for edge in edges {
				for w in edge.windows(2) {
					let (q0, q1) = (w[0], w[1]);
					p.plain(&[[q0[0], q0[1], cz], [q1[0], q1[1], cz], q1, q0], side);
				}
			}
			p.plain(
				&[
					[cx - width / 2.0, cy - depth / 2.0, cz],
					[cx - width / 2.0, cy + depth / 2.0, cz],
					[cx + width / 2.0, cy + depth / 2.0, cz],
					[cx + width / 2.0, cy - depth / 2.0, cz],
				],
				side,
			);
		}
		Ok(())
	}

	/// box/bevel_box/cylinder/cone from=A to=B: a beam or rod from one point to the other.
	fn between(&self, stmt: &Stmt, env: &Env, p: &mut Part, copy: usize, rotation: V3) -> CResult<()> {
		let (op, a, o) = (stmt.op.as_str(), &stmt.args, &stmt.opts);
		let start = self.vec3(o.get("from").unwrap(), env)?;
		let end = self.vec3(o.get("to").unwrap(), env)?;
		let length = py::dist(&start, &end);
		if length < 1e-6 {
			return Err(PartScriptError::new(format!("{op} from= and to= are the same point"), &stmt.file, stmt.line).into());
		}
		let mat = |token: &str| self.material(stmt, token, env, copy);
		if op == "b" || op == "bb" {
			let size = self.vec3(arg(a, 1)?, env)?;
			p.push_matrix(&aim(start, end, 'x'));
			if op == "b" {
				let taper = match o.get("taper") {
					Some(t) => self.vec2(t, env)?,
					None => [1.0, 1.0],
				};
				let material = mat(arg(a, 2)?)?;
				p.box_([0.0; 3], [length, size[1], size[2]], &material, rotation, &[], 1.0, taper, [0.0, 0.0]);
			} else {
				let bevel = evaluate(arg(a, 2)?, env)?;
				let material = mat(arg(a, 3)?)?;
				p.bevel_box([0.0; 3], [length, size[1], size[2]], bevel, &material, rotation, [1.0, 1.0], [0.0, 0.0]);
			}
			p.pop();
			return Ok(());
		}
		let radius = evaluate(arg(a, 1)?, env)?;
		p.push_matrix(&aim(start, end, 'z'));
		p.push_at([0.0; 3], rotation);
		let material = mat(arg(a, 3)?)?;
		if op == "cone" {
			let sides = py_int(evaluate(o.get("s").map(String::as_str).unwrap_or("8"), env)?)?;
			zero_steps(sides)?;
			p.cone([0.0; 3], radius, length, &material, sides.max(0) as usize, [0.0; 3]);
		} else {
			let sides = py_int(evaluate(o.get("s").map(String::as_str).unwrap_or("10"), env)?)?;
			let radius_top = match o.get("rt") {
				Some(t) => Some(evaluate(t, env)?),
				None => None,
			};
			let cap = match o.get("capm") {
				Some(c) => Some(mat(c)?),
				None => None,
			};
			zero_steps(sides)?;
			p.cylinder([0.0; 3], radius, length, &material, sides.max(0) as usize, [0.0; 3], radius_top, o.get("caps").map(String::as_str) != Some("0"), cap.as_deref(),
				[0.0, std::f64::consts::TAU]);
		}
		p.pop();
		p.pop();
		Ok(())
	}

	/// smooth=N: a curve through the points, N pieces between each pair.
	fn smooth(&self, points: Vec<V3>, o: &Ordered<String>, env: &Env) -> CResult<Vec<V3>> {
		let Some(s) = o.get("smooth") else { return Ok(points) };
		let steps = py_int(evaluate(s, env)?)?;
		Ok(smooth_points(&points, steps.max(0) as usize, matches!(o.get("closed").map(String::as_str), Some("1") | Some("true"))))
	}

	#[allow(clippy::too_many_arguments)]
	fn use_(&mut self, stmt: &Rc<Stmt>, env: &Env, parts: &mut Vec<Part>, matrix: &M4, depth: usize, prop: &Ctx, copy: usize) -> CResult<()> {
		let (a, o) = (&stmt.args, &stmt.opts);
		let name = arg(a, 0)?.to_string();
		let mut local = match a.get(1) {
			Some(at) => M4::translation(self.vec3(at, env)?),
			None => M4::IDENTITY,
		};
		let mut length_of = None;
		let placing = self.placing(stmt);
		if placing.contains("from") && placing.contains("to") {
			let start = self.vec3(o.get("from").unwrap(), env)?;
			let end = self.vec3(o.get("to").unwrap(), env)?;
			let length = py::dist(&start, &end);
			length_of = Some(length);
			let aim_m = if length > 1e-9 { aim(start, end, 'x') } else { M4::translation(start) };
			local = M4::translation(start).mul(&aim_m.to_3x3().to_4x4());
		}
		local = local.mul(&rot(match o.get("r") {
			Some(r) => self.vec3(r, env)?,
			None => [0.0; 3],
		})
		.to_4x4());
		if let Some(s) = o.get("s") {
			let v = self.vec3(s, env)?;
			local = local.mul(&M4::diagonal([v[0], v[1], v[2], 1.0]));
		}
		if let Some(m) = self.program.macro_(&name, &stmt.file) {
			let mut env = env.clone();
			for (k, v) in self.program.env_of(&m.file) {
				env.insert(k, v);
			}
			let mut inner = env.clone();
			for (k, v) in m.params.iter() {
				inner.insert(k.to_string(), arg_value(v, &env));
			}
			let offset = env.get("__copy__").and_then(Value::as_number).unwrap_or(0.0) as i64;
			inner.insert("__copy__".into(), Value::Int(copy as i64 + offset));
			if let Some(l) = length_of {
				inner.insert("length".into(), Value::Num(l));
			}
			for (key, value) in o.iter() {
				if !USE_OPTIONS.contains(&key) && (!PLACING.contains(&key) || m.params.contains(key)) {
					inner.insert(key.to_string(), arg_value(value, &env));
				}
			}
			let inner_path = origin_plus(&self.path, &stmt.file, stmt.line);
			let saved = std::mem::replace(&mut self.path, inner_path);
			let run = self.run(&m.body, &inner, parts, &matrix.mul(&local), depth + 1, prop);
			self.path = saved;
			return run;
		}
		let faces = self.foreign_faces(&name, stmt)?;
		let where_ = matrix.mul(&local);
		let origin = origin_plus(&self.path, &stmt.file, stmt.line);
		append(parts.last_mut().unwrap(), &faces, &where_, &origin);
		let key = self.key(&name, &stmt.file);
		let links = self.foreign_links.get(&key).cloned().unwrap_or_default();
		let moved = moved(&links, &where_)?;
		self.links.extend(moved);
		Ok(())
	}

	/// Faces of another prop: one of the program's (built whole), else the host's.
	fn foreign_faces(&mut self, used: &str, stmt: &Stmt) -> CResult<Rc<Vec<Face>>> {
		let name = self.key(used, &stmt.file);
		if let Some(f) = self.foreign.get(&name) {
			return Ok(f.clone());
		}
		let saved = self.snap_sink.take();
		let result: CResult<Option<Vec<Part>>> = (|| {
			let prop = self.program_prop(&name, "");
			match prop {
				Some(prop) if prop.kind == "prop" => {
					let asset = self.host.asset_id(&prop.name);
					let parts = self.build_used(&prop, &asset)?;
					self.foreign_links.insert(name.clone(), self.last_links.clone());
					Ok(Some(parts))
				}
				_ => Ok(self.host.foreign_parts(&name)),
			}
		})();
		self.snap_sink = saved;
		let Some(parts) = result? else {
			if stmt.called {
				return Err(PartScriptError::new(
					format!("unknown statement '{used}' (not a shape, a def, a std part or a prop; partscript ref lists the shapes)"),
					&stmt.file,
					stmt.line,
				)
				.into());
			}
			return Err(PartScriptError::new(format!("'use {used}': no def, std part or prop of that name"), &stmt.file, stmt.line).into());
		};
		let faces: Rc<Vec<Face>> = Rc::new(parts.into_iter().flat_map(|p| p.faces).collect());
		self.foreign.insert(name, faces.clone());
		Ok(faces)
	}
}

fn arg(a: &[String], i: usize) -> CResult<&str> {
	a.get(i).map(String::as_str).ok_or_else(|| Fail::Value("list index out of range".into()))
}

/// A whole position given by a named shape (desk, desk.top, desk.top_left), else None.
pub fn anchor_point(text: &str, env: &Env) -> Result<Option<V3>, String> {
	let text = crate::expr::py_strip(text);
	let (name, words) = text.split_once('.').unwrap_or((text, ""));
	if !is_identifier_lower(name) {
		return Ok(None);
	}
	if text.contains('.') && (words.is_empty() || !words.chars().all(|c| c.is_ascii_lowercase() || c == '_') || !(words.starts_with(|c: char| c.is_ascii_lowercase() || c == '_'))) {
		return Ok(None);
	}
	match env.get(name) {
		Some(Value::Bounds(b)) => b.point(words, env_frame(env).as_deref()).map(Some),
		_ => Ok(None),
	}
}

/// The sign spec (text, colours, texture shape) a sign or label line makes; with env, this copy's text.
pub fn sign_spec(stmt: &Stmt, env: Option<&Env>) -> Result<Json, String> {
	let rgb = |text: &str, default: [i64; 3]| -> [i64; 3] {
		let held = env.and_then(|e| e.get(text)).map(Value::text).unwrap_or_else(|| text.to_string());
		let t = held.trim_start_matches('#');
		if t.len() == 6 && t.chars().all(|c| c.is_ascii_hexdigit()) {
			[0, 1, 2].map(|i| i64::from_str_radix(&t[2 * i..2 * i + 2], 16).unwrap())
		} else {
			default
		}
	};
	let wrapped = stmt.opts.contains("wrap");
	let printed = wrapped || matches!(stmt.opt("printed"), Some("1") | Some("true"));
	let mut shape = "256x32";
	if wrapped {
		shape = "256x128";
	} else if printed {
		let aspect = match (stmt.args.get(1).and_then(|w| crate::expr::py_float(w)), stmt.args.get(2).and_then(|h| crate::expr::py_float(h))) {
			(Some(w), Some(h)) => w / py::max2(h, 1e-6),
			_ => 2.0,
		};
		shape = if aspect >= 6.0 {
			"256x32"
		} else if aspect >= 3.0 {
			"128x32"
		} else if aspect >= 1.5 {
			"128x64"
		} else {
			"64x64"
		};
	}
	let tex: Vec<String> = stmt.opt("tex").unwrap_or(shape).split('x').map(str::to_string).collect();
	let mut text = unquote(stmt.args.get(3).ok_or("list index out of range")?);
	let mut sub = stmt.opt("sub").unwrap_or("").to_string();
	if let Some(env) = env {
		text = interpolate(&text, env)?;
		sub = interpolate(&sub, env)?;
	}
	let bg = rgb(stmt.opt("bg").unwrap_or(""), [24, 40, 90]);
	let fg = rgb(stmt.opt("fg").unwrap_or(""), [236, 232, 220]);
	let lit_text = stmt.opt("lit").unwrap_or(if printed { "0" } else { "1.1" });
	let lit = crate::expr::py_float(lit_text).ok_or_else(|| format!("could not convert string to float: {}", py::repr_str(lit_text)))?;
	let int = |t: &str| t.parse::<i64>().map_err(|_| format!("invalid literal for int() with base 10: {}", py::repr_str(t)));
	if tex.len() < 2 {
		return Err("list index out of range".into());
	}
	let mut spec = Json::dict();
	spec.set("text", text).set("sub", sub).set("bg", bg.to_vec()).set("fg", fg.to_vec()).set("lit", lit).set("tex", vec![int(&tex[0])?, int(&tex[1])?]);
	if wrapped {
		spec.set("wrapped", true).set("mark", stmt.opt("mark").unwrap_or("bolt")).set("accent", rgb(stmt.opt("accent").unwrap_or(""), fg).to_vec());
	}
	Ok(spec)
}

/// Link ends carried into the space a matrix places them in.
pub fn moved(links: &[Snap], matrix: &M4) -> Result<Vec<Snap>, String> {
	links.iter().map(|end| end.moved(&matrix.0)).collect()
}

fn added(parts: &[Part], before: &[usize]) -> Vec<(usize, usize, usize)> {
	parts
		.iter()
		.enumerate()
		.filter_map(|(i, part)| {
			let first = before.get(i).copied().unwrap_or(0);
			(part.faces.len() > first).then_some((i, first, part.faces.len()))
		})
		.collect()
}

/// The faces added to parts since marks (each part's face count); parts new since then count whole.
fn since<'a>(parts: &'a [Part], marks: &[usize]) -> Vec<&'a Face> {
	parts.iter().enumerate().flat_map(|(i, p)| p.faces[marks.get(i).copied().unwrap_or(0).min(p.faces.len())..].iter()).collect()
}

fn since_mut<'a>(parts: &'a mut [Part], marks: &[usize]) -> Vec<&'a mut Face> {
	parts
		.iter_mut()
		.enumerate()
		.flat_map(|(i, p)| {
			let start = marks.get(i).copied().unwrap_or(0).min(p.faces.len());
			p.faces[start..].iter_mut()
		})
		.collect()
}

/// The box the faces added since marks fill, as the shape named name.
fn bounds(name: &str, parts: &[Part], marks: &[usize]) -> Bounds {
	let faces: Vec<Face> = since(parts, marks).into_iter().cloned().collect();
	if faces.iter().all(|f| f.points.is_empty()) {
		return Bounds::new(name, None, None, vec![], false);
	}
	let (lo, hi) = bounds_of(faces.iter());
	Bounds::new(name, Some(lo), Some(hi), faces, false)
}

/// Move what one copy made along normal until its nearest point lies on the plane (point, normal).
fn touch(faces: &mut [&mut Face], point: V3, normal: V3) {
	let mut nearest = f64::INFINITY;
	for f in faces.iter() {
		for q in &f.points {
			let d = dot(sub(*q, point), normal);
			if d < nearest {
				nearest = d;
			}
		}
	}
	if nearest == f64::INFINITY {
		return;
	}
	let mv = M4::translation(normal.map(|v| -nearest * v));
	for f in faces.iter_mut() {
		f.points = f.points.iter().map(|q| mv.point(*q)).collect();
	}
}

/// Let what one copy made fall (or rise) onto the surfaces under it, then sink that far; lean tilts it to
/// the ground there. It is then ground itself, for what falls after it.
fn drop_onto(faces: &mut [&mut Face], ground: &mut SurfaceIndex, lean: bool, sink: f64) {
	let points: Vec<V3> = faces.iter().flat_map(|f| f.points.iter().copied()).collect();
	if !points.is_empty() {
		let bottom = py::min_iter(points.iter().map(|q| q[2])).unwrap_or(f64::INFINITY);
		let top = py::max_iter(points.iter().map(|q| q[2])).unwrap_or(f64::NEG_INFINITY);
		let low: Vec<V3> = points.iter().copied().filter(|q| q[2] <= bottom + py::max2(0.02, (top - bottom) * 0.1)).collect();
		let cx = py::sum(low.iter().map(|q| q[0])) / low.len() as f64;
		let cy = py::sum(low.iter().map(|q| q[1])) / low.len() as f64;
		let mut samples: Vec<(f64, f64)> = Vec::new();
		for q in &low {
			let s = (py::round_to(q[0], 4), py::round_to(q[1], 4));
			if !samples.contains(&s) {
				samples.push(s);
			}
		}
		if !samples.contains(&(cx, cy)) {
			samples.push((cx, cy));
		}
		let mut hits: Vec<(f64, V3)> = samples.iter().filter_map(|(x, y)| ground.below(*x, *y, top)).collect();
		if hits.is_empty() {
			hits.push((0.0, [0.0, 0.0, 1.0]));
		}
		let rest = py::max_iter(hits.iter().map(|h| h.0)).unwrap_or(f64::NEG_INFINITY) - sink;
		let mut mv = M4::translation([0.0, 0.0, rest - bottom]);
		let centre = ground.below(cx, cy, top);
		if let (true, Some((_, n))) = (lean, centre) {
			let axis = cross([0.0, 0.0, 1.0], n);
			if length(axis) > 1e-6 {
				let pivot = [cx, cy, rest];
				let turn = M4::rotation_axis(py::max2(-1.0, py::min2(1.0, n[2])).acos(), normalized(axis));
				mv = M4::translation(pivot).mul(&turn).mul(&M4::translation(pivot.map(|v| -v))).mul(&mv);
			}
		}
		for f in faces.iter_mut() {
			f.points = f.points.iter().map(|q| mv.point(*q)).collect();
		}
	}
	for f in faces.iter() {
		ground.add(&f.points);
	}
}

/// Sizes along a line of points, 1 at the first and end at the last, by distance along it.
fn taper_sizes(points: &[V3], end: f64) -> Vec<f64> {
	let mut run = vec![0.0];
	for w in points.windows(2) {
		run.push(run[run.len() - 1] + py::dist(&w[0], &w[1]));
	}
	let total = if run[run.len() - 1] != 0.0 { run[run.len() - 1] } else { 1.0 };
	run.iter().map(|d| 1.0 + (end - 1.0) * d / total).collect()
}

/// A frame at the middle of start-end whose axis (x or z) runs from start to end, its other axes level.
fn aim(start: V3, end: V3, axis: char) -> M4 {
	let d = sub(end, start);
	let length = py::sum(d.map(|v| v * v)).sqrt();
	let d = d.map(|v| v / length);
	let up = if d[2].abs() > 0.999 { [0.0, 1.0, 0.0] } else { [0.0, 0.0, 1.0] };
	let y = [up[1] * d[2] - up[2] * d[1], up[2] * d[0] - up[0] * d[2], up[0] * d[1] - up[1] * d[0]];
	let n = py::sum(y.map(|v| v * v)).sqrt();
	let y = y.map(|v| v / n);
	let z = [d[1] * y[2] - d[2] * y[1], d[2] * y[0] - d[0] * y[2], d[0] * y[1] - d[1] * y[0]];
	let columns = if axis == 'x' { [d, y, z] } else { [y, z, d] };
	let mid = [(start[0] + end[0]) / 2.0, (start[1] + end[1]) / 2.0, (start[2] + end[2]) / 2.0];
	M4([
		[columns[0][0], columns[1][0], columns[2][0], mid[0]],
		[columns[0][1], columns[1][1], columns[2][1], mid[1]],
		[columns[0][2], columns[1][2], columns[2][2], mid[2]],
		[0.0, 0.0, 0.0, 1.0],
	])
}

/// Degrees about x, then y, then z, as a 3x3 rotation.
pub fn rot(degrees: V3) -> M3 {
	euler(degrees.map(f64::to_radians))
}

fn key6(p: V3) -> [u64; 3] {
	p.map(|v| {
		let r = py::round_to(v, 6);
		if r == 0.0 { 0 } else { r.to_bits() }
	})
}

/// Bend what a line made as a whole, by height from its lowest point to its highest, about its middle.
fn deform(faces: &mut [&mut Face], twist: f64, bend: (f64, f64), shrink: f64) {
	let points: Vec<V3> = faces.iter().flat_map(|f| f.points.iter().copied()).collect();
	if points.is_empty() {
		return;
	}
	let low = |k: usize| py::min_iter(points.iter().map(|p| p[k])).unwrap();
	let high = |k: usize| py::max_iter(points.iter().map(|p| p[k])).unwrap();
	let (cx, cy) = ((low(0) + high(0)) / 2.0, (low(1) + high(1)) / 2.0);
	let bottom = low(2);
	let height = py::max2(high(2) - bottom, 1e-6);
	let (angle, heading) = (bend.0.to_radians(), bend.1.to_radians());
	let radius = if angle.abs() > 1e-9 { Some(height / angle) } else { None };
	let mut moved: HashMap<[u64; 3], V3> = HashMap::new();
	for f in faces.iter_mut() {
		let mut out = Vec::with_capacity(f.points.len());
		for p in &f.points {
			let key = key6(*p);
			let value = *moved.entry(key).or_insert_with(|| {
				let t = (p[2] - bottom) / height;
				let (mut x, mut y, mut z) = (p[0] - cx, p[1] - cy, p[2]);
				if twist != 0.0 {
					let a = (twist * t).to_radians();
					(x, y) = (x * a.py_cos() - y * a.py_sin(), x * a.py_sin() + y * a.py_cos());
				}
				if shrink != 1.0 {
					let k = 1.0 + (shrink - 1.0) * t;
					(x, y) = (x * k, y * k);
				}
				if let Some(radius) = radius {
					let (u, mut v) = (x * (-heading).py_cos() - y * (-heading).py_sin(), x * (-heading).py_sin() + y * (-heading).py_cos());
					let theta = angle * t;
					(v, z) = (radius - (radius - v) * theta.py_cos(), bottom + (radius - v) * theta.py_sin());
					(x, y) = (u * heading.py_cos() - v * heading.py_sin(), u * heading.py_sin() + v * heading.py_cos());
				}
				[x + cx, y + cy, z]
			});
			out.push(value);
		}
		f.points = out;
	}
}

/// Move every corner by a random offset up to amount, drawn for this copy from where the corner is, so
/// corners that faces share move together and the surface stays closed.
fn wobble(faces: &mut [&mut Face], amount: V3, seed: &str) {
	let mut moved: HashMap<[u64; 3], V3> = HashMap::new();
	for f in faces.iter_mut() {
		let mut points = Vec::with_capacity(f.points.len());
		for p in &f.points {
			let key = p.map(|v| py::round_to(v, 4));
			let bits = key.map(|v| if v == 0.0 { 0 } else { v.to_bits() });
			let value = *moved.entry(bits).or_insert_with(|| {
				let text = format!("({}, {}, {})", py::repr(key[0]), py::repr(key[1]), py::repr(key[2]));
				let mut rng = PyRandom::from_str(&format!("{seed}~{text}"));
				[0, 1, 2].map(|k| p[k] + rng.uniform(-amount[k], amount[k]))
			});
			points.push(value);
		}
		f.points = points;
	}
}

/// Darken faces toward the base of what they make up: tone low at the lowest point, 1 at the highest.
fn fade(faces: &mut [&mut Face], low: f64) {
	let zs: Vec<f64> = faces.iter().flat_map(|f| f.points.iter().map(|p| p[2])).collect();
	if zs.is_empty() {
		return;
	}
	let bottom = py::min_iter(zs.iter().copied()).unwrap_or(f64::INFINITY);
	let span = py::max2(py::max_iter(zs.iter().copied()).unwrap_or(f64::NEG_INFINITY) - bottom, 1e-6);
	for f in faces.iter_mut() {
		let shades = match &f.corner_shade {
			Some(s) if s.len() == f.points.len() => s.clone(),
			_ => vec![1.0; f.points.len()],
		};
		f.corner_shade = Some(shades.iter().zip(&f.points).map(|(s, p)| s * (low + (1.0 - low) * (p[2] - bottom) / span)).collect());
	}
}

/// faces through matrix into part; origin (the use lines they came through) goes before their own.
pub fn append(part: &mut Part, faces: &[Face], matrix: &M4, origin: &Origin) {
	let flip = matrix.determinant() < 0.0;
	for f in faces {
		let mut points: Vec<V3> = f.points.iter().map(|p| matrix.point(*p)).collect();
		let (mut uv, mut shade) = (f.uv.clone(), f.corner_shade.clone());
		if flip {
			points.reverse();
			if let Some(u) = &mut uv {
				u.reverse();
			}
			if let Some(s) = &mut shade {
				s.reverse();
			}
		}
		let origin = if f.origin.is_empty() {
			origin.clone()
		} else if origin.is_empty() {
			f.origin.clone()
		} else {
			let mut joined = (**origin).clone();
			joined.extend(f.origin.iter().cloned());
			Rc::new(joined)
		};
		part.faces.push(Face { points, material: f.material.clone(), uv, shade: f.shade, corner_shade: shade, group: f.group, tiled: f.tiled, origin });
	}
}

/// A prop's px= and ao=auto, both sized from its bounds.
pub fn finish_parts(prop: &Prop, parts: &mut [Part]) {
	let px = prop.opt("px").unwrap_or("");
	let ao = prop.opt("ao").unwrap_or("");
	let live: Vec<usize> = (0..parts.len()).filter(|&i| !parts[i].faces.is_empty()).collect();
	if live.is_empty() || (px.is_empty() && ao != "auto") {
		return;
	}
	let mut size = [0.0; 3];
	for (k, s) in size.iter_mut().enumerate() {
		let hi = py::max_iter(live.iter().map(|&i| parts[i].bounds().1[k])).unwrap_or(f64::NEG_INFINITY);
		let lo = py::min_iter(live.iter().map(|&i| parts[i].bounds().0[k])).unwrap_or(f64::INFINITY);
		*s = hi - lo;
	}
	if !px.is_empty() {
		let density = if px == "auto" {
			py::max2(32.0, py::min2(256.0, 80.0 / py::max2(py::max2(py::max2(size[0], size[1]), size[2]), 0.001)))
		} else {
			crate::expr::py_float(px).unwrap_or(32.0)
		};
		for &i in &live {
			parts[i].uv_scale = density / 32.0;
		}
	}
	if ao == "auto" {
		for &i in &live {
			parts[i].ao_height = Some(py::max2(0.15, py::min2(1.4, size[2] * 1.2)));
		}
	}
}

#[allow(dead_code)]
fn _unused(_: &maths::M3) {}
