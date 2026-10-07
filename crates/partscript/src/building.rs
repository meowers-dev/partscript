//! Kits, snap points and buildings.
//!
//! A kit is a set of pieces by role (wall kinds, floor, roof, parapet, partition, corner, stair) on a grid
//! of cells and storeys. A building lays rooms out on that grid; openings swap wall kinds, and attach snaps
//! any other piece onto a placed one by named snap points. Coordinates: X right, Y back, Z up, metres; a
//! building's origin is the front-left corner of cell 0,0, its front (side s) faces -Y.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use kitlib::py::PyMath;
use kitlib::json::Json;
use kitlib::py;

use crate::lang::{is_snake_letter, tokenize, unquote, KitSnap, KitSpec, PartScriptError, Program, Prop, Stmt};
use crate::ordered::Ordered;

pub const ROOM_OPTS: [&str; 8] = ["storeys", "storey", "floor", "roof", "walls", "theme", "as", "fill"];
pub const FILL_VARIABLES: [&str; 8] = ["w", "d", "h", "level", "door_s", "door_n", "door_e", "door_w"];
pub const WALL_KINDS: [&str; 7] = ["solid", "window", "window_broken", "door", "wide_door", "damaged", "upper"];
pub const ROLES: [&str; 6] = ["floor", "roof", "parapet", "partition", "corner", "stair"];
const SIDES: [(&str, (i64, i64)); 4] = [("s", (0, -1)), ("n", (0, 1)), ("w", (-1, 0)), ("e", (1, 0))];
pub const BUILDING_BUDGET: usize = 200000;
pub const SNAP_USAGE: &str = "snap NAME C DIR [up=DIR] [kind=WORD]   DIR: +x -x +y -y +z -z (or front back left right up down)";
pub const ATTACH_USAGE: &str = "attach PIECE to=TARGET [at=SNAP] [via=SNAP] [spin=DEG] [slide=A,B] [as=NAME]";

fn side_dir(side: &str) -> Option<(i64, i64)> {
	SIDES.iter().find(|(s, _)| *s == side).map(|(_, d)| *d)
}

fn wall_turn(inward: (i64, i64)) -> f64 {
	match inward {
		(0, 1) => 180.0,
		(0, -1) => 0.0,
		(1, 0) => 90.0,
		_ => -90.0,
	}
}

pub fn dirs(word: &str) -> Option<[f64; 3]> {
	Some(match word {
		"+x" | "right" => [1.0, 0.0, 0.0],
		"-x" | "left" => [-1.0, 0.0, 0.0],
		"+y" | "back" => [0.0, 1.0, 0.0],
		"-y" | "front" => [0.0, -1.0, 0.0],
		"+z" | "up" => [0.0, 0.0, 1.0],
		"-z" | "down" => [0.0, 0.0, -1.0],
		_ => return None,
	})
}

// ------------------------------------------------------------------ small vector maths
type V = [f64; 3];
pub type M = [[f64; 4]; 4];

fn vsub(a: V, b: V) -> V {
	[a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn vadd(a: V, b: V) -> V {
	[a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn vscale(a: V, k: f64) -> V {
	[a[0] * k, a[1] * k, a[2] * k]
}
fn vdot(a: V, b: V) -> f64 {
	py::sum([a[0] * b[0], a[1] * b[1], a[2] * b[2]])
}
fn vcross(a: V, b: V) -> V {
	[a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unit(a: V) -> Result<V, String> {
	let length = vdot(a, a).sqrt();
	if length < 1e-9 {
		return Err("a snap direction has no length".into());
	}
	Ok(vscale(a, 1.0 / length))
}

/// Orthonormal columns (direction, up, direction x up); up is made perpendicular to direction.
fn frame(direction: V, up: V) -> Result<(V, V, V), String> {
	let d = unit(direction)?;
	let mut u = vsub(up, vscale(d, vdot(up, d)));
	if vdot(u, u) < 1e-9 {
		let (base, along) = if d[2].abs() < 0.9 { ([0.0, 0.0, 1.0], d[2]) } else { ([0.0, 1.0, 0.0], d[1]) };
		u = vsub(base, vscale(d, along));
	}
	let u = unit(u)?;
	Ok((d, u, vcross(d, u)))
}

pub fn mat_identity() -> M {
	[[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}

pub fn mat_mul(a: &M, b: &M) -> M {
	let mut out = [[0.0; 4]; 4];
	for i in 0..4 {
		for j in 0..4 {
			out[i][j] = py::sum((0..4).map(|k| a[i][k] * b[k][j]));
		}
	}
	out
}

pub fn mat_point(m: &M, p: V) -> V {
	[0, 1, 2].map(|i| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3])
}

fn mat_vector(m: &M, v: V) -> V {
	[0, 1, 2].map(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

/// Translation then a turn of degrees about +Z.
pub fn mat_place(position: V, degrees: f64) -> M {
	let (c, s) = (degrees.to_radians().py_cos(), degrees.to_radians().py_sin());
	let (c, s) = (py::round_to(c, 12), py::round_to(s, 12));
	[[c, -s, 0.0, position[0]], [s, c, 0.0, position[1]], [0.0, 0.0, 1.0, position[2]], [0.0, 0.0, 0.0, 1.0]]
}

fn mat_rotation(columns: (V, V, V)) -> M {
	let (a, b, c) = columns;
	[[a[0], b[0], c[0], 0.0], [a[1], b[1], c[1], 0.0], [a[2], b[2], c[2], 0.0], [0.0, 0.0, 0.0, 1.0]]
}

/// Rotation by degrees about a unit axis through the origin (Rodrigues).
fn mat_axis_turn(axis: V, degrees: f64) -> Result<M, String> {
	let [x, y, z] = unit(axis)?;
	let (c, s) = (degrees.to_radians().py_cos(), degrees.to_radians().py_sin());
	let t = 1.0 - c;
	Ok([
		[t * x * x + c, t * x * y - s * z, t * x * z + s * y, 0.0],
		[t * x * y + s * z, t * y * y + c, t * y * z - s * x, 0.0],
		[t * x * z - s * y, t * y * z + s * x, t * z * z + c, 0.0],
		[0.0, 0.0, 0.0, 1.0],
	])
}

fn mat_translate(v: V) -> M {
	let mut m = mat_identity();
	m[0][3] = v[0];
	m[1][3] = v[1];
	m[2][3] = v[2];
	m
}

// ------------------------------------------------------------------ snaps
#[derive(Clone, Debug, PartialEq)]
pub struct Snap {
	pub name: String,
	pub pos: V,
	pub dir: V,
	pub up: V,
	pub kind: String,
}

impl Snap {
	pub fn new(name: &str, pos: V, dir: V, up: V, kind: &str) -> Snap {
		Snap { name: name.to_string(), pos, dir, up, kind: kind.to_string() }
	}

	pub fn plain(name: &str, pos: V, dir: V, kind: &str) -> Snap {
		Snap::new(name, pos, dir, [0.0, 0.0, 1.0], kind)
	}

	pub fn moved(&self, m: &M) -> Result<Snap, String> {
		Ok(Snap::new(&self.name, mat_point(m, self.pos), unit(mat_vector(m, self.dir))?, unit(mat_vector(m, self.up))?, &self.kind))
	}

	pub fn as_json(&self) -> Json {
		let r = |v: V| Json::List(v.iter().map(|x| Json::Float(py::round_to(*x, 4))).collect());
		let mut out = Json::dict();
		out.set("name", self.name.as_str()).set("pos", r(self.pos)).set("dir", r(self.dir)).set("up", r(self.up)).set("kind", self.kind.as_str());
		out
	}
}

/// A direction word (+x, back, up...) or an x,y,z vector evaluated by number().
pub fn parse_dir(text: &str, number: &mut dyn FnMut(&str) -> Result<f64, String>) -> Result<V, String> {
	if let Some(d) = dirs(text) {
		return Ok(d);
	}
	let parts: Vec<&str> = text.split(',').collect();
	if parts.len() != 3 {
		return Err(format!(
			"direction {}: one of +x -x +y -y +z -z (or front back left right up down) or x,y,z",
			py::repr_str(text)
		));
	}
	unit([number(parts[0])?, number(parts[1])?, number(parts[2])?])
}

pub fn default_up(direction: V) -> V {
	if direction[2].abs() > 0.9 {
		[0.0, 1.0, 0.0]
	} else {
		[0.0, 0.0, 1.0]
	}
}

/// Snaps every piece has from its bounds: base, top, front, back, left, right (kind any).
pub fn bounds_snaps(low: V, high: V) -> Ordered<Snap> {
	let (cx, cy, cz) = ((low[0] + high[0]) / 2.0, (low[1] + high[1]) / 2.0, (low[2] + high[2]) / 2.0);
	let mut out = Ordered::new();
	for (name, pos, d) in [
		("base", [cx, cy, low[2]], [0.0, 0.0, -1.0]),
		("top", [cx, cy, high[2]], [0.0, 0.0, 1.0]),
		("front", [cx, low[1], cz], [0.0, -1.0, 0.0]),
		("back", [cx, high[1], cz], [0.0, 1.0, 0.0]),
		("left", [low[0], cy, cz], [-1.0, 0.0, 0.0]),
		("right", [high[0], cy, cz], [1.0, 0.0, 0.0]),
	] {
		out.insert(name, Snap::new(name, pos, d, default_up(d), "any"));
	}
	out
}

/// Grid snaps a kit's pieces get from their role: wall ends and faces, floor and roof edges.
pub fn role_snaps(role: &str, kit: &Kit) -> Vec<Snap> {
	let (g, h, t) = (kit.grid, kit.storey, kit.wall);
	if role.starts_with("wall") || role == "partition" {
		return vec![
			Snap::plain("left", [-g / 2.0, 0.0, 0.0], [-1.0, 0.0, 0.0], "wall_end"),
			Snap::plain("right", [g / 2.0, 0.0, 0.0], [1.0, 0.0, 0.0], "wall_end"),
			Snap::new("top", [0.0, 0.0, h], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0], "wall_top"),
			Snap::plain("outside", [0.0, t / 2.0, h / 2.0], [0.0, 1.0, 0.0], "wall_face"),
			Snap::plain("inside", [0.0, -t / 2.0, h / 2.0], [0.0, -1.0, 0.0], "wall_face"),
		];
	}
	if role == "floor" || role == "roof" {
		let kind = format!("{role}_edge");
		return vec![
			Snap::new("top", [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], &format!("{role}_top")),
			Snap::plain("n", [0.0, g / 2.0, 0.0], [0.0, 1.0, 0.0], &kind),
			Snap::plain("s", [0.0, -g / 2.0, 0.0], [0.0, -1.0, 0.0], &kind),
			Snap::plain("e", [g / 2.0, 0.0, 0.0], [1.0, 0.0, 0.0], &kind),
			Snap::plain("w", [-g / 2.0, 0.0, 0.0], [-1.0, 0.0, 0.0], &kind),
		];
	}
	Vec::new()
}

/// Placement that puts source (piece-local) on target (building space): same point, facing each other,
/// ups together; then spin about the target's direction and slide along its face.
pub fn snap_matrix(source: &Snap, target: &Snap, spin: f64, slide: (f64, f64)) -> Result<M, String> {
	let (sd, su, sx) = frame(source.dir, source.up)?;
	let (td, tu, _) = frame(target.dir, target.up)?;
	let nd = vscale(td, -1.0);
	let (nd, nu, nx) = frame(nd, tu)?;
	let mut rotation = mat_mul(&mat_rotation((nd, nu, nx)), &[
		[sd[0], sd[1], sd[2], 0.0],
		[su[0], su[1], su[2], 0.0],
		[sx[0], sx[1], sx[2], 0.0],
		[0.0, 0.0, 0.0, 1.0],
	]);
	if spin != 0.0 {
		rotation = mat_mul(&mat_axis_turn(td, spin)?, &rotation);
	}
	let right = vcross(vscale(td, -1.0), tu);
	let point = vadd(target.pos, vadd(vscale(right, slide.0), vscale(tu, slide.1)));
	Ok(mat_mul(&mat_translate(point), &mat_mul(&rotation, &mat_translate(vscale(source.pos, -1.0)))))
}

pub fn kinds_fit(a: &str, b: &str) -> bool {
	a == "any" || b == "any" || a == b
}

// ------------------------------------------------------------------ kits
#[derive(Clone, Debug)]
pub struct Kit {
	pub name: String,
	pub grid: f64,
	pub storey: f64,
	pub wall: f64,
	pub parapet_lift: f64,
	pub stair_cells: i64,
	pub stair_exit: String,
	pub walls: Ordered<String>,
	pub pieces: Ordered<String>,
	pub snaps: Ordered<Vec<KitSnap>>,
	pub file: String,
	pub line: usize,
}

impl Kit {
	pub fn roles_of(&self, name: &str) -> Vec<String> {
		let mut out: Vec<String> = self.walls.iter().filter(|(_, v)| *v == name).map(|(k, _)| format!("wall:{k}")).collect();
		out.extend(self.pieces.iter().filter(|(_, v)| *v == name).map(|(k, _)| k.to_string()));
		out
	}
}

const KIT_OPTS: [(&str, f64); 5] = [("grid", 4.0), ("storey", 4.0), ("wall", 0.3), ("parapet_lift", 0.3), ("stair_cells", 2.0)];

/// kit NAME [walls=SET] [grid=4] ..., then wall KIND=PIECE, piece ROLE=PIECE, snap PIECE NAME C DIR lines.
pub fn parse_kit(raw: &[(usize, String)], file: &str, program: &mut Program) -> Result<(), PartScriptError> {
	let (number, header) = &raw[0];
	let tokens = tokenize(header).map_err(|e| PartScriptError::new(e, file, *number))?;
	if tokens.len() < 2 || !(tokens[1].len() >= 2 && tokens[1].len() <= 41 && is_snake_letter(&tokens[1])) {
		return Err(PartScriptError::new("kit needs a lower_snake_case name: kit NAME [walls=SET] [grid=4] [storey=4]", file, *number));
	}
	let name = program.qualified(&tokens[1], file);
	if let Some(before) = program.kits.get(&name) {
		return Err(PartScriptError::new(format!("kit {name} defined twice (also {}:{})", before.file, before.line), file, *number));
	}
	let mut opts = Ordered::new();
	for token in &tokens[2..] {
		let (key, value) = match token.split_once('=') {
			Some(kv) => kv,
			None => (token.as_str(), ""),
		};
		if !token.contains('=') || !(KIT_OPTS.iter().any(|(k, _)| *k == key) || key == "walls" || key == "stair_exit") {
			let listed: Vec<String> = KIT_OPTS.iter().map(|(k, v)| format!("{k}={}", if *k == "stair_cells" { "2".into() } else { py::repr(*v) })).collect();
			return Err(PartScriptError::new(
				format!("kit option {}: walls=SET (start from a base wall set), {}", py::repr_str(token), listed.join(", ")),
				file,
				*number,
			));
		}
		opts.insert(key, unquote(value));
	}
	let mut spec = KitSpec { name: name.clone(), opts, walls: Ordered::new(), pieces: Ordered::new(), snaps: Ordered::new(), file: file.into(), line: *number };
	for (number, statement) in &raw[1..] {
		if statement == "end" {
			continue;
		}
		let tokens = tokenize(statement).map_err(|e| PartScriptError::new(e, file, *number))?;
		match tokens[0].as_str() {
			"wall" => {
				for token in &tokens[1..] {
					let (kind, piece) = token.split_once('=').unwrap_or((token, ""));
					let kind_ok = !kind.is_empty() && kind.len() <= 31 && is_snake_letter(kind);
					if !token.contains('=') || !kind_ok || piece.is_empty() {
						return Err(PartScriptError::new(
							format!(
								"kit wall {}: KIND=PIECE; the usual kinds are {} (any word works: shop, sash...)",
								py::repr_str(token),
								WALL_KINDS.join(", ")
							),
							file,
							*number,
						));
					}
					spec.walls.insert(kind, piece.to_string());
				}
			}
			"piece" => {
				for token in &tokens[1..] {
					let (role, piece) = token.split_once('=').unwrap_or((token, ""));
					if !token.contains('=') || !ROLES.contains(&role) || piece.is_empty() {
						return Err(PartScriptError::new(
							format!("kit piece {}: ROLE=PIECE with ROLE one of {}", py::repr_str(token), ROLES.join(", ")),
							file,
							*number,
						));
					}
					spec.pieces.insert(role, piece.to_string());
				}
			}
			"snap" => {
				if tokens.len() < 5 {
					return Err(PartScriptError::new(
						"kit snap: snap PIECE NAME C DIR [up=DIR] [kind=WORD] (snap points for a piece that declares none)",
						file,
						*number,
					));
				}
				spec.snaps.entry_or(&tokens[1], Vec::new()).push(KitSnap { tokens: tokens[2..].to_vec(), file: file.into(), line: *number });
			}
			head => {
				return Err(PartScriptError::new(format!("'{head}' in kit {name}: wall KIND=PIECE, piece ROLE=PIECE or snap PIECE NAME C DIR"), file, *number));
			}
		}
	}
	program.kits.insert(&name, spec);
	Ok(())
}

/// The base kit a host offers (kit NAME walls=SET starts from one of its wall sets).
#[derive(Clone, Debug, Default)]
pub struct BaseKit {
	pub walls: Ordered<Ordered<String>>,
	pub pieces: Ordered<String>,
	pub grid: Option<f64>,
	pub storey: Option<f64>,
	pub wall_thickness: Option<f64>,
	pub parapet_lift: Option<f64>,
}

/// The kit a building names: its own lines over the host's wall set it starts from.
pub fn resolve_kit(program: &Program, name: &str, base: &BaseKit, file: &str) -> Result<Kit, String> {
	let name = program.kit_name(name, file);
	let Some(spec) = program.kits.get(&name) else {
		let mut known: Vec<&str> = program.kits.keys().collect();
		known.sort();
		let known = if known.is_empty() { "none yet".to_string() } else { known.join(", ") };
		return Err(format!(
			"no kit {} (kits: {known}); define one: kit NAME walls=SET, then wall KIND=PIECE and piece ROLE=PIECE lines",
			py::repr_str(&name)
		));
	};
	let opts = &spec.opts;
	let mut kit = Kit {
		name: name.clone(),
		grid: 4.0,
		storey: 4.0,
		wall: 0.3,
		parapet_lift: 0.3,
		stair_cells: 2,
		stair_exit: "ahead".into(),
		walls: Ordered::new(),
		pieces: Ordered::new(),
		snaps: Ordered::new(),
		file: spec.file.clone(),
		line: spec.line,
	};
	for (key, default) in KIT_OPTS {
		let value = match opts.get(key) {
			Some(v) => crate::expr::py_float(v).ok_or_else(|| format!("could not convert string to float: {}", py::repr_str(v)))?,
			None => default,
		};
		match key {
			"grid" => kit.grid = value,
			"storey" => kit.storey = value,
			"wall" => kit.wall = value,
			"parapet_lift" => kit.parapet_lift = value,
			_ => kit.stair_cells = value as i64,
		}
	}
	kit.stair_exit = opts.get("stair_exit").cloned().unwrap_or_else(|| "ahead".into());
	if kit.stair_exit != "ahead" && kit.stair_exit != "back" {
		return Err(format!("kit {name}: stair_exit={}: ahead (a straight flight) or back (a dog-leg that turns back on itself)", kit.stair_exit));
	}
	if let Some(set) = opts.get("walls").filter(|v| !v.is_empty()) {
		let Some(walls) = base.walls.get(set) else {
			let mut sets: Vec<&str> = base.walls.keys().collect();
			sets.sort();
			return Err(format!("kit {name}: walls={set}: the base kit has wall sets {}", if sets.is_empty() { "none".into() } else { sets.join(", ") }));
		};
		for (k, v) in walls.iter() {
			kit.walls.insert(k, v.clone());
		}
		for (role, key) in [("floor", "floor"), ("roof", "roof"), ("parapet", "parapet"), ("partition", "partition"), ("corner", "pillar"), ("stair", "stair_4m")] {
			if let Some(piece) = base.pieces.get(key) {
				kit.pieces.insert(role, piece.clone());
			}
		}
		kit.grid = base.grid.unwrap_or(kit.grid);
		kit.storey = base.storey.unwrap_or(kit.storey);
		kit.wall = base.wall_thickness.unwrap_or(kit.wall);
		kit.parapet_lift = base.parapet_lift.unwrap_or(kit.parapet_lift);
		if kit.pieces.contains("corner") && !spec.pieces.contains("corner") {
			kit.pieces.remove("corner");
		}
	}
	let namespace = program.namespaces.get(&spec.file).cloned();
	let own: HashSet<&str> = program.props.iter().map(|p| p.name.as_str()).collect();
	let piece = |value: &str| -> String {
		if let Some(ns) = &namespace {
			let qualified = format!("{ns}.{value}");
			if own.contains(qualified.as_str()) {
				return qualified;
			}
		}
		value.to_string()
	};
	for (k, v) in spec.walls.iter() {
		kit.walls.insert(k, piece(v));
	}
	for (k, v) in spec.pieces.iter() {
		kit.pieces.insert(k, piece(v));
	}
	if !kit.walls.contains("solid") {
		return Err(format!("kit {name}: no solid wall (wall solid=PIECE, or walls=SET to start from a base wall set)"));
	}
	for (k, v) in spec.snaps.iter() {
		kit.snaps.insert(&piece(k), v.clone());
	}
	Ok(kit)
}

// ------------------------------------------------------------------ building plans
#[derive(Clone, Debug)]
pub struct Attach {
	pub to: String,
	pub at: String,
	pub via: String,
	pub spin: f64,
	pub slide: (f64, f64),
}

/// A room's furnishing: the variables its def gets, where its doors are, the room's name.
#[derive(Clone, Debug)]
pub struct Fill {
	pub env: Vec<(String, FillValue)>,
	pub doors: Vec<(String, f64)>,
	pub room: String,
}

#[derive(Clone, Debug)]
pub enum FillValue {
	Num(f64),
	Int(i64),
	Text(String),
}

#[derive(Clone, Debug)]
pub struct Placement {
	pub piece: String,
	pub role: String,
	pub matrix: Option<M>,
	pub stmt: Rc<Stmt>,
	pub names: Vec<String>,
	pub nav: String,
	pub attach: Option<Attach>,
	pub fill: Option<Fill>,
	/// the order it was added in (names keep the first placement added under them)
	pub seq: usize,
}

#[derive(Clone, Debug)]
pub struct RoomOut {
	pub name: String,
	pub cells: [i64; 4],
	pub storey: i64,
	pub storeys: i64,
	pub theme: String,
}

#[derive(Clone, Debug)]
pub struct Opening {
	pub kind: String,
	pub cell: [i64; 2],
	pub side: String,
	pub storey: i64,
	pub pos: V,
	pub turn: f64,
}

#[derive(Clone, Debug)]
pub struct Plan {
	pub kit: Kit,
	pub placements: Vec<Placement>,
	pub rooms: Vec<RoomOut>,
	pub openings: Vec<Opening>,
	pub errors: Vec<String>,
	pub warnings: Vec<String>,
	pub by_name: HashMap<String, usize>,
}

impl Plan {
	pub fn for_stmt(&self, stmt: &Stmt) -> Vec<usize> {
		(0..self.placements.len()).filter(|&i| self.placements[i].stmt.uid == stmt.uid).collect()
	}

	fn add(&mut self, mut placement: Placement) {
		placement.seq = self.placements.len();
		for name in &placement.names {
			self.by_name.entry(name.clone()).or_insert(self.placements.len());
		}
		self.placements.push(placement);
	}

	fn reindex(&mut self) {
		self.by_name.clear();
		let mut order: Vec<usize> = (0..self.placements.len()).collect();
		order.sort_by_key(|&i| self.placements[i].seq);
		for i in order {
			for name in &self.placements[i].names {
				self.by_name.entry(name.clone()).or_insert(i);
			}
		}
	}
}

/// X,Y with either part a number or an inclusive a..b range.
fn cells(text: &str, number: &mut dyn FnMut(&str) -> Result<f64, String>, what: &str) -> Result<Vec<(i64, i64)>, String> {
	let parts: Vec<&str> = text.split(',').collect();
	if parts.len() != 2 {
		return Err(format!("{what} {}: X,Y in cells (0,0 is the front-left cell; a..b gives a range: 0..2,0)", py::repr_str(text)));
	}
	let mut ranges: Vec<Vec<i64>> = Vec::new();
	for part in parts {
		if let Some((a, b)) = part.split_once("..") {
			let (a, b) = (py::round(number(a)?) as i64, py::round(number(b)?) as i64);
			ranges.push((a.min(b)..=a.max(b)).collect());
		} else {
			ranges.push(vec![py::round(number(part)?) as i64]);
		}
	}
	let mut out = Vec::new();
	for &y in &ranges[1] {
		for &x in &ranges[0] {
			out.push((x, y));
		}
	}
	Ok(out)
}

type Edge = (char, i64, i64);

fn edge(cell: (i64, i64), side: &str) -> Edge {
	let (x, y) = cell;
	match side {
		"s" => ('x', x, y),
		"n" => ('x', x, y + 1),
		"w" => ('y', x, y),
		_ => ('y', x + 1, y),
	}
}

fn edge_names(e: Edge, storey: i64) -> Vec<String> {
	let (axis, x, y) = e;
	let cells: [((i64, i64), &str); 2] = if axis == 'x' { [((x, y - 1), "n"), ((x, y), "s")] } else { [((x - 1, y), "e"), ((x, y), "w")] };
	cells.iter().map(|(c, side)| format!("{},{},{side},{storey}", c.0, c.1)).collect()
}

struct Room {
	stmt: Rc<Stmt>,
	x: i64,
	y: i64,
	w: i64,
	d: i64,
	storeys: i64,
	base: i64,
	index: usize,
}

struct EdgeData {
	room: usize,
	inward: (i64, i64),
	shared: bool,
	side: String,
	cell: (i64, i64),
}

struct Stair {
	stmt: Rc<Stmt>,
	x: i64,
	y: i64,
	dir: String,
	storey: i64,
}

/// Grid placements of a building, statement by statement; attach requests stay unresolved.
pub fn plan(prop: &Prop, kit: Kit, number: &mut dyn FnMut(&str) -> Result<f64, String>) -> Plan {
	let mut result = Plan { kit: kit.clone(), placements: Vec::new(), rooms: Vec::new(), openings: Vec::new(), errors: Vec::new(), warnings: Vec::new(),
		by_name: HashMap::new() };
	let (g, h) = (kit.grid, kit.storey);
	let body: Vec<Rc<Stmt>> = prop.body.iter().filter(|s| crate::lang::BUILD_OPS.contains(&s.op.as_str())).cloned().collect();
	let mut rooms: Vec<Room> = Vec::new();
	let mut defaults: Vec<(String, Option<HashSet<i64>>, Option<HashSet<String>>)> = Vec::new();
	let mut opens: HashMap<(Edge, i64), (String, Rc<Stmt>)> = HashMap::new();
	let mut stairs: Vec<Stair> = Vec::new();
	let (mut roof_roof, mut roof_parapet): (Option<String>, Option<String>) = (None, None);
	let kind_ok = |kind: &str| kind == "none" || kind == "open" || kit.walls.contains(kind);
	let walls_list = || kit.walls.keys().collect::<Vec<_>>().join(", ");
	for stmt in &body {
		let (a, o) = (&stmt.args, &stmt.opts);
		let outcome: Result<(), String> = (|| {
			match stmt.op.as_str() {
				"room" => {
					if a.len() < 2 {
						return Err("room X,Y W,D [storeys=1] [storey=0] [floor=none] [roof=none] [walls=KIND] [theme=STYLE] [as=NAME]".into());
					}
					let corner = cells(&a[0], number, "room")?;
					if corner.len() != 1 {
						return Err(format!("room {}: X,Y is the room's front-left cell (no ranges); W,D its size in cells", a[0]));
					}
					let (x, y) = corner[0];
					let size: Vec<&str> = a[1].split(',').collect();
					if size.len() != 2 {
						return Err(format!("room size {}: W,D in cells (3,2 is 12 x 8 m on a 4 m grid)", py::repr_str(&a[1])));
					}
					let (w, d) = (py::round(number(size[0])?) as i64, py::round(number(size[1])?) as i64);
					let storeys = py::round(number(o.get("storeys").map(String::as_str).unwrap_or("1"))?) as i64;
					let base = py::round(number(o.get("storey").map(String::as_str).unwrap_or("0"))?) as i64;
					if w < 1 || d < 1 || !(1..=12).contains(&storeys) || base < 0 {
						return Err(format!("room {} {}: width, depth and storeys from 1 (storeys up to 12), storey from 0", a[0], a[1]));
					}
					if let Some(walls) = o.get("walls").filter(|v| !v.is_empty()) {
						if !kind_ok(walls) {
							return Err(format!("walls={walls}: kit {} has {} (or none)", kit.name, walls_list()));
						}
					}
					rooms.push(Room { stmt: stmt.clone(), x, y, w, d, storeys, base, index: rooms.len() });
				}
				"walls" => {
					if a.is_empty() || !kind_ok(&a[0]) {
						return Err(format!("walls KIND [storey=N] [side=s,n,e,w]: KIND one of {}, none", walls_list()));
					}
					let storeys = match o.get("storey").map(String::as_str).unwrap_or("all") {
						"all" => None,
						text => Some(text.split(',').map(|v| number(v).map(|n| py::round(n) as i64)).collect::<Result<HashSet<_>, _>>()?),
					};
					let sides: Option<HashSet<String>> = o.get("side").map(|s| s.split(',').map(str::to_string).collect());
					if let Some(sides) = &sides {
						if !sides.is_empty() && !sides.iter().all(|s| side_dir(s).is_some()) {
							return Err(format!("side={}: s (front), n (back), w, e", o.get("side").unwrap()));
						}
					}
					defaults.push((a[0].clone(), storeys, sides));
				}
				"open" => {
					if a.len() < 3 || side_dir(&a[1]).is_none() || !kind_ok(&a[2]) {
						return Err(format!("open X,Y SIDE KIND [storey=0]: SIDE s n w e, KIND one of {}, none", walls_list()));
					}
					let storey = py::round(number(o.get("storey").map(String::as_str).unwrap_or("0"))?) as i64;
					for cell in cells(&a[0], number, "open")? {
						opens.insert((edge(cell, &a[1]), storey), (a[2].clone(), stmt.clone()));
					}
				}
				"stair" => {
					if a.is_empty() {
						return Err("stair X,Y [DIR] [storey=0]: rises toward DIR (n back, s front, e, w)".into());
					}
					let direction = a.get(1).cloned().unwrap_or_else(|| "n".into());
					if side_dir(&direction).is_none() {
						return Err(format!("stair direction {}: n (back), s (front), e or w", py::repr_str(&direction)));
					}
					if kit.pieces.get("stair").is_none_or(|s| s.is_empty()) {
						return Err(format!("kit {} has no stair piece (piece stair=PIECE)", kit.name));
					}
					let cell = cells(&a[0], number, "stair")?;
					if cell.len() != 1 {
						return Err(format!("stair {}: one cell X,Y (its foot)", a[0]));
					}
					let storey = py::round(number(o.get("storey").map(String::as_str).unwrap_or("0"))?) as i64;
					stairs.push(Stair { stmt: stmt.clone(), x: cell[0].0, y: cell[0].1, dir: direction, storey });
				}
				"roof" => {
					if let Some(v) = o.get("roof").cloned().or_else(|| a.first().cloned()) {
						roof_roof = Some(v);
					}
					if let Some(v) = o.get("parapet").cloned() {
						roof_parapet = Some(v);
					}
				}
				"attach" | "place" => {
					if a.is_empty() {
						return Err(if stmt.op == "attach" { ATTACH_USAGE.into() } else { "place PIECE C [r=rx,ry,rz] [as=NAME]".into() });
					}
					if stmt.op == "attach" && !o.contains("to") {
						return Err(format!(
							"attach needs to=TARGET (a name given with as=, or X,Y,SIDE[,STOREY] for a wall, X,Y,floor[,STOREY], X,Y,roof): {ATTACH_USAGE}"
						));
					}
				}
				_ => {}
			}
			Ok(())
		})();
		if let Err(message) = outcome {
			result.errors.push(format!("{}: {message}", stmt.where_()));
		}
	}
	if rooms.is_empty() && !body.iter().any(|s| s.op == "place" || s.op == "attach") {
		result.errors.push(format!("{}:{}: a building needs at least one room X,Y W,D (or place/attach lines)", prop.file, prop.line));
	}
	if !result.errors.is_empty() {
		return result;
	}

	// Which storeys each cell has a floor on, and the stairwells that open the floor above.
	let mut occupied: HashMap<(i64, i64, i64), usize> = HashMap::new();
	for room in &rooms {
		for s in room.base..room.base + room.storeys {
			for cy in room.y..room.y + room.d {
				for cx in room.x..room.x + room.w {
					occupied.entry((cx, cy, s)).or_insert(room.index);
				}
			}
		}
	}
	let mut wells: HashSet<(i64, i64, i64)> = HashSet::new();
	for stair in &stairs {
		let (dx, dy) = side_dir(&stair.dir).unwrap();
		for k in 0..kit.stair_cells {
			wells.insert((stair.x + dx * k, stair.y + dy * k, stair.storey + 1));
		}
	}

	// Edges: the first room to claim one owns it; the other side of a shared edge makes it a partition.
	let mut edges: Vec<((Edge, i64), EdgeData)> = Vec::new();
	let mut edge_index: HashMap<(Edge, i64), usize> = HashMap::new();
	for room in &rooms {
		let inside: HashSet<(i64, i64)> = (room.y..room.y + room.d).flat_map(|cy| (room.x..room.x + room.w).map(move |cx| (cx, cy))).collect();
		let mut ordered: Vec<(i64, i64)> = inside.iter().copied().collect();
		ordered.sort_by_key(|c| (c.1, c.0));
		for s in room.base..room.base + room.storeys {
			for &(cx, cy) in &ordered {
				for (side, (dx, dy)) in SIDES {
					if inside.contains(&(cx + dx, cy + dy)) {
						continue;
					}
					let key = (edge((cx, cy), side), s);
					let inward = (-dx, -dy);
					if let Some(&i) = edge_index.get(&key) {
						if edges[i].1.inward != inward {
							edges[i].1.shared = true;
						}
						continue;
					}
					edge_index.insert(key, edges.len());
					edges.push((key, EdgeData { room: room.index, inward, shared: false, side: side.into(), cell: (cx, cy) }));
				}
			}
		}
	}

	let wall_kind = |key: &(Edge, i64), data: &EdgeData| -> String {
		let s = key.1;
		let mut kind = if data.shared { "partition".to_string() } else { "solid".to_string() };
		if !data.shared {
			if let Some(w) = rooms[data.room].stmt.opts.get("walls") {
				kind = w.clone();
			}
			for (value, storeys, sides) in &defaults {
				if storeys.as_ref().is_none_or(|set| set.contains(&s)) && sides.as_ref().is_none_or(|set| set.contains(&data.side)) {
					kind = value.clone();
				}
			}
		}
		if let Some((open_kind, _)) = opens.get(key) {
			kind = open_kind.clone();
		}
		kind
	};
	let cutaway = matches!(prop.opt("cutaway").unwrap_or(""), "open" | "1" | "true");

	let doors_of = |room: &Room, s: i64| -> (Vec<(String, f64)>, Vec<(String, f64)>) {
		let mut first: Vec<(String, f64)> = SIDES.iter().map(|(side, _)| (format!("door_{side}"), -1.0)).collect();
		let mut every = Vec::new();
		for cy in room.y..room.y + room.d {
			for cx in room.x..room.x + room.w {
				for (side, _) in SIDES {
					let key = (edge((cx, cy), side), s);
					if let Some(&i) = edge_index.get(&key) {
						if wall_kind(&key, &edges[i].1).contains("door") {
							let raw = if side == "s" || side == "n" { (cx - room.x) as f64 + 0.5 } else { (cy - room.y) as f64 + 0.5 } * g;
							let along = py::round_to(raw, 4) - kit.wall / 2.0;
							every.push((side.to_string(), py::round_to(along, 4)));
							let slot = first.iter_mut().find(|(k, _)| *k == format!("door_{side}")).unwrap();
							if slot.1 < 0.0 {
								slot.1 = py::round_to(along, 4);
							}
						}
					}
				}
			}
		}
		(first, every)
	};

	// Every cell that is the top of some room: a roof there joins the roofs around it.
	let mut roofed: HashSet<(i64, i64, i64)> = HashSet::new();
	for r in &rooms {
		for cy in r.y..r.y + r.d {
			for cx in r.x..r.x + r.w {
				let key = (cx, cy, r.base + r.storeys);
				if !occupied.contains_key(&key) {
					roofed.insert(key);
				}
			}
		}
	}
	let (mut placed_floor, mut placed_roof, mut parapets, mut corners) =
		(HashSet::new(), HashSet::new(), HashSet::<(Edge, i64)>::new(), HashSet::<(i64, i64, i64)>::new());
	let role_piece = |value: &str| -> String { if ROLES.contains(&value) { kit.pieces.get(value).cloned().unwrap_or_else(|| value.to_string()) } else { value.to_string() } };
	for room in &rooms {
		let (stmt, o) = (&room.stmt, &room.stmt.opts);
		let floor_piece = o.get("floor").cloned().unwrap_or_else(|| kit.pieces.get("floor").cloned().unwrap_or_default());
		for s in room.base..room.base + room.storeys {
			if !floor_piece.is_empty() && floor_piece != "none" {
				for cy in room.y..room.y + room.d {
					for cx in room.x..room.x + room.w {
						if placed_floor.contains(&(cx, cy, s)) || wells.contains(&(cx, cy, s)) {
							continue;
						}
						placed_floor.insert((cx, cy, s));
						result.add(Placement {
							piece: role_piece(&floor_piece),
							role: "floor".into(),
							matrix: Some(mat_place([(cx as f64 + 0.5) * g, (cy as f64 + 0.5) * g, s as f64 * h], 0.0)),
							stmt: stmt.clone(),
							names: vec![format!("{cx},{cy},floor,{s}")],
							nav: "walkable".into(),
							attach: None,
							fill: None,
							seq: 0,
						});
					}
				}
			}
			for (key, data) in &edges {
				if data.room != room.index || key.1 != s {
					continue;
				}
				let mut kind = wall_kind(key, data);
				if kind == "none" || kind == "open" || (cutaway && data.side == "s" && !data.shared) {
					continue;
				}
				let (piece, role);
				if kind == "partition" {
					piece = kit.pieces.get("partition").filter(|p| !p.is_empty()).cloned().unwrap_or_else(|| kit.walls.get("solid").unwrap().clone());
					role = "partition".to_string();
				} else {
					if kind == "solid" && s > 0 && kit.walls.contains("upper") {
						kind = "upper".into();
					}
					piece = kit.walls.get(&kind).cloned().unwrap_or_else(|| kit.walls.get("solid").unwrap().clone());
					role = format!("wall:{kind}");
				}
				let ((axis, ex, ey), _) = *key;
				let mid = if axis == 'x' { [(ex as f64 + 0.5) * g, ey as f64 * g, s as f64 * h] } else { [ex as f64 * g, (ey as f64 + 0.5) * g, s as f64 * h] };
				let turn = wall_turn(data.inward);
				result.add(Placement {
					piece,
					role,
					matrix: Some(mat_place(mid, turn)),
					stmt: stmt.clone(),
					names: edge_names(key.0, s),
					nav: "walkable".into(),
					attach: None,
					fill: None,
					seq: 0,
				});
				if !matches!(kind.as_str(), "solid" | "upper" | "blank") {
					result.openings.push(Opening {
						kind: kind.clone(),
						cell: [data.cell.0, data.cell.1],
						side: data.side.clone(),
						storey: s,
						pos: mid.map(|v| py::round_to(v, 4)),
						turn,
					});
				}
			}
			if kit.pieces.get("corner").is_some_and(|c| !c.is_empty()) {
				for (key, data) in &edges {
					let (e, es) = *key;
					if data.room != room.index || es != s || data.shared || matches!(wall_kind(key, data).as_str(), "none" | "open") {
						continue;
					}
					let (axis, ex, ey) = e;
					let vertices = if axis == 'x' { [(ex, ey), (ex + 1, ey)] } else { [(ex, ey), (ex, ey + 1)] };
					for (vx, vy) in vertices {
						let perpendicular: [Edge; 2] = if axis == 'x' { [('y', vx, vy - 1), ('y', vx, vy)] } else { [('x', vx - 1, vy), ('x', vx, vy)] };
						let touching = perpendicular.iter().any(|p| edge_index.get(&(*p, s)).is_some_and(|&i| !edges[i].1.shared));
						if touching && !corners.contains(&(vx, vy, s)) {
							corners.insert((vx, vy, s));
							result.add(Placement {
								piece: kit.pieces.get("corner").unwrap().clone(),
								role: "corner".into(),
								matrix: Some(mat_place([vx as f64 * g, vy as f64 * g, s as f64 * h], 0.0)),
								stmt: stmt.clone(),
								names: vec![format!("corner:{vx},{vy},{s}")],
								nav: "walkable".into(),
								attach: None,
								fill: None,
								seq: 0,
							});
						}
					}
				}
			}
		}
		let top = room.base + room.storeys;
		let roof_piece = o.get("roof").cloned().unwrap_or_else(|| roof_roof.clone().filter(|r| !r.is_empty()).unwrap_or_else(|| kit.pieces.get("roof").cloned().unwrap_or_default()));
		if !roof_piece.is_empty() && roof_piece != "none" && !cutaway {
			for cy in room.y..room.y + room.d {
				for cx in room.x..room.x + room.w {
					if occupied.contains_key(&(cx, cy, top)) || placed_roof.contains(&(cx, cy, top)) {
						continue;
					}
					placed_roof.insert((cx, cy, top));
					result.add(Placement {
						piece: role_piece(&roof_piece),
						role: "roof".into(),
						matrix: Some(mat_place([(cx as f64 + 0.5) * g, (cy as f64 + 0.5) * g, top as f64 * h], 0.0)),
						stmt: stmt.clone(),
						names: vec![format!("{cx},{cy},roof"), format!("{cx},{cy},roof,{top}")],
						nav: "ignore".into(),
						attach: None,
						fill: None,
					seq: 0,
					});
				}
			}
			let parapet = match &roof_parapet {
				Some(p) => p.clone(),
				None => kit.pieces.get("parapet").cloned().unwrap_or_default(),
			};
			if !parapet.is_empty() && parapet != "none" {
				for cy in room.y..room.y + room.d {
					for cx in room.x..room.x + room.w {
						if !placed_roof.contains(&(cx, cy, top)) {
							continue;
						}
						for (side, (dx, dy)) in SIDES {
							let (nx, ny) = (cx + dx, cy + dy);
							if roofed.contains(&(nx, ny, top)) || occupied.contains_key(&(nx, ny, top)) {
								continue;
							}
							let e = edge((cx, cy), side);
							if parapets.contains(&(e, top)) {
								continue;
							}
							parapets.insert((e, top));
							let (axis, ex, ey) = e;
							let mid = if axis == 'x' {
								[(ex as f64 + 0.5) * g, ey as f64 * g, top as f64 * h + kit.parapet_lift]
							} else {
								[ex as f64 * g, (ey as f64 + 0.5) * g, top as f64 * h + kit.parapet_lift]
							};
							result.add(Placement {
								piece: parapet.clone(),
								role: "parapet".into(),
								matrix: Some(mat_place(mid, if axis == 'x' { 0.0 } else { 90.0 })),
								stmt: stmt.clone(),
								names: vec![],
								nav: "ignore".into(),
								attach: None,
								fill: None,
								seq: 0,
							});
						}
					}
				}
			}
		}
		let name = o.get("as").cloned().unwrap_or_else(|| format!("room{}", result.rooms.len() + 1));
		result.rooms.push(RoomOut { name: name.clone(), cells: [room.x, room.y, room.w, room.d], storey: room.base, storeys: room.storeys,
			theme: o.get("theme").cloned().unwrap_or_default() });
		if let Some(fill) = o.get("fill").filter(|f| !f.is_empty()) {
			let params: Vec<(String, FillValue)> = o.iter().filter(|(k, _)| !ROOM_OPTS.contains(k)).map(|(k, v)| (k.to_string(), FillValue::Text(v.clone()))).collect();
			for s in room.base..room.base + room.storeys {
				let (doors, every) = doors_of(room, s);
				let mut variables = params.clone();
				variables.push(("w".into(), FillValue::Num(room.w as f64 * g - kit.wall)));
				variables.push(("d".into(), FillValue::Num(room.d as f64 * g - kit.wall)));
				variables.push(("h".into(), FillValue::Num(h)));
				variables.push(("level".into(), FillValue::Int(s)));
				for (k, v) in doors {
					variables.push((k, FillValue::Num(v)));
				}
				variables.push(("__seed__".into(), FillValue::Text(format!("{}|{name}|{s}", prop.name))));
				// later keys win, as in a dict literal
				let mut merged: Vec<(String, FillValue)> = Vec::new();
				for (k, v) in variables {
					if let Some(slot) = merged.iter_mut().find(|(key, _)| *key == k) {
						slot.1 = v;
					} else {
						merged.push((k, v));
					}
				}
				result.add(Placement {
					piece: fill.clone(),
					role: "fill".into(),
					matrix: Some(mat_place([room.x as f64 * g + kit.wall / 2.0, room.y as f64 * g + kit.wall / 2.0, s as f64 * h], 0.0)),
					stmt: stmt.clone(),
					names: vec![format!("{name}:{s}")],
					nav: String::new(),
					attach: None,
					fill: Some(Fill { env: merged, doors: every, room: name.clone() }),
					seq: 0,
				});
			}
		}
	}
	for stair in &stairs {
		let (x, y, s) = (stair.x as f64, stair.y as f64, stair.storey);
		let foot = match stair.dir.as_str() {
			"n" => ((x + 0.5) * g, y * g),
			"s" => ((x + 0.5) * g, (y + 1.0) * g),
			"e" => (x * g, (y + 0.5) * g),
			_ => ((x + 1.0) * g, (y + 0.5) * g),
		};
		let turn = match stair.dir.as_str() {
			"n" => 0.0,
			"s" => 180.0,
			"e" => -90.0,
			_ => 90.0,
		};
		result.add(Placement {
			piece: kit.pieces.get("stair").unwrap().clone(),
			role: "stair".into(),
			matrix: Some(mat_place([foot.0, foot.1, s as f64 * h], turn)),
			stmt: stair.stmt.clone(),
			names: stair.stmt.opts.get("as").map(|a| vec![a.clone()]).unwrap_or_default(),
			nav: "walkable".into(),
			attach: None,
			fill: None,
			seq: 0,
		});
	}
	for stmt in &body {
		if stmt.op == "place" {
			let at = stmt.args.get(1).cloned().unwrap_or_else(|| "0,0,0".into());
			let parsed: Result<(Vec<f64>, Vec<f64>), String> = (|| {
				let position = at.split(',').map(|v| number(v)).collect::<Result<Vec<_>, _>>()?;
				let rot = stmt.opts.get("r").map(String::as_str).unwrap_or("0,0,0").split(',').map(|v| number(v)).collect::<Result<Vec<_>, _>>()?;
				if position.len() != 3 || rot.len() != 3 {
					return Err(String::new());
				}
				Ok((position, rot))
			})();
			let Ok((position, rot)) = parsed else {
				result.errors.push(format!("{}: place {} {at}: C is x,y,z in metres; r=rx,ry,rz in degrees", stmt.where_(), stmt.args[0]));
				continue;
			};
			let mut matrix = mat_translate([position[0], position[1], position[2]]);
			for (axis, angle) in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].into_iter().zip(rot) {
				if angle != 0.0 {
					matrix = mat_mul(&matrix, &mat_axis_turn(axis, angle).unwrap());
				}
			}
			result.add(Placement {
				piece: stmt.args[0].clone(),
				role: "place".into(),
				matrix: Some(matrix),
				stmt: stmt.clone(),
				names: stmt.opts.get("as").map(|a| vec![a.clone()]).unwrap_or_default(),
				nav: String::new(),
				attach: None,
				fill: None,
				seq: 0,
			});
		} else if stmt.op == "attach" {
			let o = &stmt.opts;
			let parsed: Result<((f64, f64), f64), String> = (|| {
				let slide = o.get("slide").map(String::as_str).unwrap_or("0,0").split(',').map(|v| number(v)).collect::<Result<Vec<_>, _>>()?;
				if slide.len() != 2 {
					return Err(String::new());
				}
				let spin = number(o.get("spin").map(String::as_str).unwrap_or("0"))?;
				Ok(((slide[0], slide[1]), spin))
			})();
			let Ok((slide, spin)) = parsed else {
				result.errors.push(format!("{}: slide=A,B: metres right and up along the target face; spin=DEG", stmt.where_()));
				continue;
			};
			let mut target = o.get("to").unwrap().clone();
			let parts: Vec<&str> = target.split(',').collect();
			if parts.len() == 3 && (side_dir(parts[2]).is_some() || parts[2] == "floor") {
				target.push_str(",0");
			}
			result.add(Placement {
				piece: stmt.args[0].clone(),
				role: "attach".into(),
				matrix: None,
				stmt: stmt.clone(),
				names: o.get("as").map(|a| vec![a.clone()]).unwrap_or_default(),
				nav: String::new(),
				attach: Some(Attach { to: target, at: o.get("at").cloned().unwrap_or_default(), via: o.get("via").cloned().unwrap_or_default(), spin, slide }),
				fill: None,
				seq: 0,
			});
		}
	}
	let warnings = reachability(&rooms, &edges, &wall_kind, &stairs, &occupied, &wells, &kit);
	result.warnings.extend(warnings);
	// Order the plan as the statements were written (a room's pieces, then its stairs, attachments...).
	let order: HashMap<usize, usize> = prop.body.iter().enumerate().map(|(i, s)| (s.uid, i)).collect();
	result.placements.sort_by_key(|p| order.get(&p.stmt.uid).copied().unwrap_or(0));
	result.reindex();
	result
}

fn passable(kind: &str) -> bool {
	kind.contains("door") || kind == "none" || kind == "open"
}

#[allow(clippy::too_many_arguments)]
fn reachability(rooms: &[Room], edges: &[((Edge, i64), EdgeData)], wall_kind: &dyn Fn(&(Edge, i64), &EdgeData) -> String, stairs: &[Stair],
	occupied: &HashMap<(i64, i64, i64), usize>, wells: &HashSet<(i64, i64, i64)>, kit: &Kit) -> Vec<String> {
	let room_at = |cell: (i64, i64), s: i64| occupied.get(&(cell.0, cell.1, s)).copied();
	let label = |room: &Room, s: i64| {
		let name = room.stmt.opts.get("as").cloned().unwrap_or_else(|| format!("room {},{}", room.x, room.y));
		format!("{name} (storey {s})")
	};
	let mut links: HashMap<(usize, i64), HashSet<(usize, i64)>> = HashMap::new();
	let mut starts: HashSet<(usize, i64)> = HashSet::new();
	let link = |a: (usize, i64), b: (usize, i64), links: &mut HashMap<(usize, i64), HashSet<(usize, i64)>>| {
		links.entry(a).or_default().insert(b);
		links.entry(b).or_default().insert(a);
	};
	for (key, data) in edges {
		let kind = wall_kind(key, data);
		if !passable(&kind) {
			continue;
		}
		let ((axis, x, y), s) = *key;
		let cells = if axis == 'x' { [(x, y - 1), (x, y)] } else { [(x - 1, y), (x, y)] };
		let inside = [room_at(cells[0], s), room_at(cells[1], s)];
		if let (Some(a), Some(b)) = (inside[0], inside[1]) {
			link((a, s), (b, s), &mut links);
		} else if inside.iter().any(Option::is_some) && kind.contains("door") {
			starts.insert((inside.iter().flatten().next().copied().unwrap(), s));
		}
	}
	let mut out = Vec::new();
	for stair in stairs {
		let (dx, dy) = side_dir(&stair.dir).unwrap();
		let (x, y, s) = (stair.x, stair.y, stair.storey);
		let foot = (x - dx, y - dy);
		let top = if kit.stair_exit == "ahead" { (x + dx * kit.stair_cells, y + dy * kit.stair_cells) } else { (x - dx, y - dy) };
		let where_ = format!("{}: stair {x},{y} {} storey={s}", stair.stmt.where_(), stair.dir);
		let (low, high) = (room_at(foot, s), room_at(top, s + 1));
		if low.is_none() || wells.contains(&(foot.0, foot.1, s)) {
			out.push(format!("{where_}: no floor at its foot (cell {},{} on storey {s}) to step on from", foot.0, foot.1));
		}
		if high.is_none() || wells.contains(&(top.0, top.1, s + 1)) {
			out.push(format!("{where_}: no floor where it arrives (cell {},{} on storey {}) to step off onto", top.0, top.1, s + 1));
		}
		if let (Some(l), Some(hh)) = (low, high) {
			link((l, s), (hh, s + 1), &mut links);
		}
	}
	let mut seen: HashSet<(usize, i64)> = starts.clone();
	let mut todo: Vec<(usize, i64)> = starts.into_iter().collect();
	while let Some(node) = todo.pop() {
		if let Some(others) = links.get(&node) {
			for other in others {
				if seen.insert(*other) {
					todo.push(*other);
				}
			}
		}
	}
	for room in rooms {
		for s in room.base..room.base + room.storeys {
			if !seen.contains(&(room.index, s)) {
				out.push(format!("{}: {} cannot be reached from outside (no door or stair leads to it)", room.stmt.where_(), label(room, s)));
			}
		}
	}
	out
}

/// Why an attachment could not be placed: the plan's own fault, or the piece's.
pub enum ResolveError {
	Build(String),
	Piece(crate::compiler::Fail),
}

/// Places attachments in order. snaps_of(piece, role) gives its snaps in its own space.
pub fn resolve(result: &mut Plan, snaps_of: &mut dyn FnMut(&str, &str) -> Result<Ordered<Snap>, crate::compiler::Fail>) -> Result<(), ResolveError> {
	for index in 0..result.placements.len() {
		let Some(request) = result.placements[index].attach.clone() else { continue };
		let stmt = result.placements[index].stmt.clone();
		let fail = ResolveError::Build;
		let target_index = result.by_name.get(&request.to).copied().filter(|&t| result.placements[t].matrix.is_some());
		let Some(target_index) = target_index else {
			let mut names: Vec<&String> = result.by_name.keys().filter(|n| !n.contains(',')).collect();
			names.sort();
			let named = if names.is_empty() { String::new() } else { format!(" (named pieces: {})", names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")) };
			return Err(fail(format!(
				"{}: attach to={}: nothing placed there yet{named}; walls are X,Y,SIDE,STOREY, floors X,Y,floor,STOREY, roofs X,Y,roof",
				stmt.where_(),
				request.to
			)));
		};
		let target = result.placements[target_index].clone();
		let mut target_snaps: Ordered<Snap> = Ordered::new();
		for (name, snap) in snaps_of(&target.piece, &target.role).map_err(ResolveError::Piece)?.iter() {
			target_snaps.insert(name, snap.moved(target.matrix.as_ref().unwrap()).map_err(fail)?);
		}
		let at = if !request.at.is_empty() { request.at.clone() } else if target_snaps.contains("outside") { "outside".into() } else { "top".into() };
		let Some(goal) = target_snaps.get(&at).cloned() else {
			let mut names: Vec<&str> = target_snaps.keys().collect();
			names.sort();
			return Err(fail(format!("{}: {} has no snap {} (it has {})", stmt.where_(), target.piece, py::repr_str(&at), names.join(", "))));
		};
		let piece = result.placements[index].piece.clone();
		let own = snaps_of(&piece, "attach").map_err(ResolveError::Piece)?;
		let mut via = request.via.clone();
		if via.is_empty() {
			let matching: Vec<&Snap> = own.values().filter(|s| s.kind == goal.kind && goal.kind != "any").collect();
			via = match matching.first() {
				Some(s) => s.name.clone(),
				None if goal.dir[2] > 0.7 => "base".into(),
				None if goal.dir[2] < -0.7 => "top".into(),
				None => "back".into(),
			};
		}
		let Some(source) = own.get(&via).cloned() else {
			let mut names: Vec<&str> = own.keys().collect();
			names.sort();
			return Err(fail(format!("{}: {piece} has no snap {} (it has {})", stmt.where_(), py::repr_str(&via), names.join(", "))));
		};
		if !kinds_fit(&source.kind, &goal.kind) {
			return Err(fail(format!(
				"{}: {piece}.{via} is kind {}, {}.{at} is kind {}: they do not fit",
				stmt.where_(),
				source.kind,
				target.piece,
				goal.kind
			)));
		}
		result.placements[index].matrix = Some(snap_matrix(&source, &goal, request.spin, request.slide).map_err(fail)?);
	}
	Ok(())
}

// ------------------------------------------------------------------ export for game engines (Y up)
/// An authoring-space point (Z up, +Y back) in Y-up space: (x, z, -y).
pub fn yup_position(p: V) -> Json {
	Json::List(vec![
		Json::Float(py::round_to(p[0], 4) + 0.0),
		Json::Float(py::round_to(p[2], 4) + 0.0),
		Json::Float(py::round_to(-p[1], 4) + 0.0),
	])
}

/// ("yaw", degrees) for a turn about the vertical, else ("rotation", [x, y, z]) radians, Y-up, YXZ order.
pub fn yup_rotation(m: &M) -> (String, Json) {
	if (m[2][2] - 1.0).abs() < 1e-6 {
		return ("yaw".into(), Json::Float(py::round_to(m[1][0].atan2(m[0][0]).to_degrees(), 4)));
	}
	let c = [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]];
	let r: [[f64; 3]; 3] = [0, 1, 2].map(|i| [0, 1, 2].map(|j| m[i][j]));
	let cr: [[f64; 3]; 3] = [0, 1, 2].map(|i| [0, 1, 2].map(|j| py::sum((0..3).map(|k| c[i][k] * r[k][j]))));
	let gm: [[f64; 3]; 3] = [0, 1, 2].map(|i| [0, 1, 2].map(|j| py::sum((0..3).map(|k| cr[i][k] * c[j][k]))));
	let x = py::max2(-1.0, py::min2(1.0, -gm[1][2])).asin();
	let (y, z) = if x.py_cos().abs() > 1e-6 { (gm[0][2].atan2(gm[2][2]), gm[1][0].atan2(gm[1][1])) } else { ((-gm[2][0]).atan2(gm[0][0]), 0.0) };
	("rotation".into(), Json::List(vec![Json::Float(py::round_to(x, 6)), Json::Float(py::round_to(y, 6)), Json::Float(py::round_to(z, 6))]))
}

/// A building as data for a game engine: its pieces in Y-up space round the front-left corner, its rooms
/// and its openings. asset_of(piece) names the asset a piece places.
pub fn export_building(result: &Plan, title: &str, asset_of: &dyn Fn(&str) -> String) -> Json {
	let g = result.kit.grid;
	let mut placements = Vec::new();
	for p in &result.placements {
		let Some(m) = &p.matrix else { continue };
		let mut row = Json::dict();
		row.set("asset", asset_of(&p.piece)).set("role", p.role.as_str()).set("position", yup_position(mat_point(m, [0.0; 3])));
		let (key, value) = yup_rotation(m);
		row.set(&key, value);
		if !p.nav.is_empty() {
			row.set("nav", p.nav.as_str());
		}
		placements.push(row);
	}
	let rooms: Vec<Json> = result
		.rooms
		.iter()
		.map(|room| {
			let [x, y, w, d] = room.cells.map(|v| v as f64);
			let mut out = Json::dict();
			out.set("name", room.name.as_str())
				.set("rect", vec![x * g, -(y + d) * g, (x + w) * g, -y * g])
				.set("storey", room.storey)
				.set("storeys", room.storeys)
				.set("theme", room.theme.as_str());
			out
		})
		.collect();
	let openings: Vec<Json> = result
		.openings
		.iter()
		.map(|o| {
			let mut out = Json::dict();
			out.set("kind", o.kind.as_str())
				.set("cell", vec![o.cell[0], o.cell[1]])
				.set("side", o.side.as_str())
				.set("storey", o.storey)
				.set("position", yup_position(o.pos))
				.set("yaw", o.turn);
			out
		})
		.collect();
	let mut out = Json::dict();
	out.set("title", title)
		.set("set", result.kit.name.as_str())
		.set("grid", g)
		.set("storey", result.kit.storey)
		.set("placements", Json::List(placements))
		.set("rooms", Json::List(rooms))
		.set("openings", Json::List(openings));
	out
}

pub fn snaps_vec(snaps: &[Snap]) -> Ordered<Snap> {
	let mut out = Ordered::new();
	for s in snaps {
		out.insert(&s.name, s.clone());
	}
	out
}
