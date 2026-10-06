//! Geometry: the part (a mesh being modelled) and every shape PartScript draws.
//!
//! Axes follow Blender: X right, +Y back (north), Z up, metres; a prop's front faces -Y. The glTF
//! writer turns that into glTF's Y up / -Z forward. A Part collects faces tagged with a material key;
//! UVs default to a box projection in metres, and bake() later dedupes the faces and bakes a tone.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::maths::{self, add, cross, dot, euler, length, normalized, scale, sub, M4, V3};
use crate::py;

#[derive(Clone, Debug, PartialEq)]
pub struct Mat {
	pub texture: String,
	pub tile: f64,
	pub emission: String,
	pub emission_strength: f64,
	pub emission_color: [f64; 3],
	pub roughness: f64,
	pub metallic: f64,
	pub ao: bool,
	pub alpha_clip: bool,
	pub double_sided: bool,
	pub unlit_ao: f64,
	pub alpha: f64,
	pub tile_v: f64,
}

impl Mat {
	pub fn new(texture: &str, tile: f64) -> Mat {
		Mat {
			texture: texture.to_string(),
			tile,
			emission: String::new(),
			emission_strength: 0.0,
			emission_color: [0.0; 3],
			roughness: 1.0,
			metallic: 0.0,
			ao: true,
			alpha_clip: false,
			double_sided: false,
			unlit_ao: 1.0,
			alpha: 1.0,
			tile_v: 0.0,
		}
	}

	/// (u, v) metres per texture repeat.
	pub fn tiles(&self) -> (f64, f64) {
		(self.tile, if self.tile_v != 0.0 { self.tile_v } else { self.tile })
	}
}

/// A decal sheet: named cells on a square grid of one texture (the trim statement draws them).
#[derive(Clone, Debug)]
pub struct Atlas {
	pub material: String,
	pub cells: Vec<(String, [f64; 4])>,
	pub grid: f64,
	pub emissive: Vec<String>,
	pub emit_material: String,
}

pub type Materials = Rc<std::cell::RefCell<HashMap<String, Mat>>>;

/// The use lines a face came through, outermost first: ((file, line), ...).
pub type Origin = Rc<Vec<(Rc<str>, usize)>>;

#[derive(Clone, Debug)]
pub struct Face {
	pub points: Vec<V3>,
	pub material: Rc<str>,
	pub uv: Option<Vec<[f64; 2]>>,
	pub shade: f64,
	pub corner_shade: Option<Vec<f64>>,
	pub group: Option<u64>,
	pub tiled: bool,
	pub origin: Origin,
}

static GROUPS: AtomicU64 = AtomicU64::new(1);

/// A fresh group number (faces of one flat surface share one facet tone).
pub fn next_group() -> u64 {
	GROUPS.fetch_add(1, Ordering::Relaxed)
}

thread_local! {
	static EMPTY_ORIGIN: Origin = Rc::new(Vec::new());
}

pub fn empty_origin() -> Origin {
	EMPTY_ORIGIN.with(|o| o.clone())
}

/// Accumulates faces in a local frame; push/pop nest transforms.
pub struct Part {
	pub name: String,
	pub materials: Materials,
	pub faces: Vec<Face>,
	stack: Vec<M4>,
	pub smooth: bool,
	pub uv_scale: f64,
	pub ao_height: Option<f64>,
	pub lead_material: Option<String>,
}

pub struct FaceSpec {
	pub uv: Option<Vec<[f64; 2]>>,
	pub shade: f64,
	pub corner_shade: Option<Vec<f64>>,
	pub group: Option<u64>,
	pub tiled: bool,
}

impl Default for FaceSpec {
	fn default() -> Self {
		FaceSpec { uv: None, shade: 1.0, corner_shade: None, group: None, tiled: false }
	}
}

impl Part {
	pub fn new(name: &str, materials: Materials) -> Part {
		Part {
			name: name.to_string(),
			materials,
			faces: Vec::new(),
			stack: vec![M4::IDENTITY],
			smooth: false,
			uv_scale: 1.0,
			ao_height: None,
			lead_material: None,
		}
	}

	pub fn mat(&self, key: &str) -> Mat {
		self.materials.borrow().get(key).cloned().unwrap_or_else(|| Mat::new("", 2.0))
	}

	fn tiles(&self, key: &str) -> (f64, f64) {
		self.materials.borrow().get(key).map(Mat::tiles).unwrap_or((2.0, 2.0))
	}

	pub fn matrix(&self) -> &M4 {
		self.stack.last().unwrap()
	}

	/// Moves and turns (radians, XYZ) and scales what comes next.
	pub fn push(&mut self, location: V3, rotation: V3, scale_by: V3) -> &mut Self {
		let m = M4::translation(location).mul(&euler(rotation).to_4x4()).mul(&M4::diagonal([scale_by[0], scale_by[1], scale_by[2], 1.0]));
		let top = self.matrix().mul(&m);
		self.stack.push(top);
		self
	}

	pub fn push_at(&mut self, location: V3, rotation: V3) -> &mut Self {
		self.push(location, rotation, [1.0; 3])
	}

	pub fn push_matrix(&mut self, m: &M4) -> &mut Self {
		let top = self.matrix().mul(m);
		self.stack.push(top);
		self
	}

	pub fn pop(&mut self) -> &mut Self {
		self.stack.pop();
		self
	}

	pub fn xf(&self, p: V3) -> V3 {
		self.matrix().point(p)
	}

	pub fn face(&mut self, points: &[V3], material: &str, spec: FaceSpec) {
		let points = points.iter().map(|p| self.xf(*p)).collect();
		self.faces.push(Face {
			points,
			material: Rc::from(material),
			uv: spec.uv,
			shade: spec.shade,
			corner_shade: spec.corner_shade,
			group: spec.group,
			tiled: spec.tiled,
			origin: empty_origin(),
		});
	}

	pub fn plain(&mut self, points: &[V3], material: &str) {
		self.face(points, material, FaceSpec::default());
	}

	/// Axis box; skip omits faces by name (+x -x +y -y +z -z); taper scales the top face in x and y and
	/// lean shifts it, for sloped and trapezoid bodies.
	#[allow(clippy::too_many_arguments)]
	pub fn box_(&mut self, center: V3, size: V3, material: &str, rotation: V3, skip: &[&str], shade: f64, taper: [f64; 2], lean: [f64; 2]) {
		self.push_at(center, rotation);
		let (hx, hy, hz) = (size[0] * 0.5, size[1] * 0.5, size[2] * 0.5);
		let c: Vec<V3> = [
			[-hx, -hy, -hz], [hx, -hy, -hz], [hx, hy, -hz], [-hx, hy, -hz],
			[-hx, -hy, hz], [hx, -hy, hz], [hx, hy, hz], [-hx, hy, hz],
		]
		.iter()
		.map(|p| taper_point(*p, hz, taper, lean))
		.collect();
		let quads: [(&str, [usize; 4]); 6] = [
			("-z", [0, 3, 2, 1]), ("+z", [4, 5, 6, 7]), ("-y", [0, 1, 5, 4]),
			("+y", [2, 3, 7, 6]), ("-x", [3, 0, 4, 7]), ("+x", [1, 2, 6, 5]),
		];
		for (key, idx) in quads {
			if !skip.contains(&key) {
				let points: Vec<V3> = idx.iter().map(|&i| c[i]).collect();
				self.face(&points, material, FaceSpec { shade, ..Default::default() });
			}
		}
		self.pop();
	}

	pub fn simple_box(&mut self, center: V3, size: V3, material: &str) {
		self.box_(center, size, material, [0.0; 3], &[], 1.0, [1.0, 1.0], [0.0, 0.0]);
	}

	pub fn box_minmax(&mut self, lo: V3, hi: V3, material: &str) {
		let center = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5];
		let size = [(hi[0] - lo[0]).abs(), (hi[1] - lo[1]).abs(), (hi[2] - lo[2]).abs()];
		self.box_(center, size, material, [0.0; 3], &[], 1.0, [1.0, 1.0], [0.0, 0.0]);
	}

	/// Closed chamfered box: six panels, twelve edge strips, eight corner triangles (44 triangles).
	#[allow(clippy::too_many_arguments)]
	pub fn bevel_box(&mut self, center: V3, size: V3, bevel: f64, material: &str, rotation: V3, taper: [f64; 2], lean: [f64; 2]) {
		let half = [size[0] * 0.5, size[1] * 0.5, size[2] * 0.5];
		let mut b = bevel;
		for h in half {
			b = b.min(h * 0.9);
		}
		let b = if b > 0.0 { b } else { 0.0f64.max(b) };
		if b == 0.0 {
			self.box_(center, size, material, rotation, &[], 1.0, taper, lean);
			return;
		}
		let inner = [half[0] - b, half[1] - b, half[2] - b];
		self.push_at(center, rotation);
		let emit = |part: &mut Part, mut points: Vec<V3>| {
			let normal = cross(sub(points[1], points[0]), sub(points[2], points[0]));
			let mut total = [0.0; 3];
			for p in &points {
				total = add(total, *p);
			}
			let midpoint = maths::div(total, points.len() as f64);
			if dot(normal, midpoint) < 0.0 {
				points.reverse();
			}
			let shaped: Vec<V3> = points.iter().map(|p| taper_point(*p, half[2], taper, lean)).collect();
			if shaped.len() == 4 && taper[0] != taper[1] {
				let (a, b, c, d) = (shaped[0], shaped[1], shaped[2], shaped[3]);
				if dot(cross(sub(b, a), sub(c, a)), sub(d, a)).abs() > 1e-9 {
					let group = next_group();
					part.face(&shaped[..3], material, FaceSpec { group: Some(group), ..Default::default() });
					part.face(&[shaped[0], shaped[2], shaped[3]], material, FaceSpec { group: Some(group), ..Default::default() });
					return;
				}
			}
			part.plain(&shaped, material);
		};
		for axis in 0..3 {
			let other: Vec<usize> = (0..3).filter(|&i| i != axis).collect();
			for sign in [-1.0, 1.0] {
				let mut points = Vec::new();
				for (u, v) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
					let mut point = [0.0; 3];
					point[axis] = sign * half[axis];
					point[other[0]] = u * inner[other[0]];
					point[other[1]] = v * inner[other[1]];
					points.push(point);
				}
				emit(self, points);
			}
		}
		for (a, c) in [(0, 1), (0, 2), (1, 2)] {
			let free = 3 - a - c;
			for sa in [-1.0, 1.0] {
				for sc in [-1.0, 1.0] {
					let mut points = Vec::new();
					for (outer_a, sf) in [(true, -1.0), (false, -1.0), (false, 1.0), (true, 1.0)] {
						let mut point = [0.0; 3];
						point[a] = sa * if outer_a { half[a] } else { inner[a] };
						point[c] = sc * if outer_a { inner[c] } else { half[c] };
						point[free] = sf * inner[free];
						points.push(point);
					}
					emit(self, points);
				}
			}
		}
		for sx in [-1.0, 1.0] {
			for sy in [-1.0, 1.0] {
				for sz in [-1.0, 1.0] {
					let signs = [sx, sy, sz];
					let points: Vec<V3> = (0..3)
						.map(|axis| {
							let mut p = [0.0; 3];
							for i in 0..3 {
								p[i] = signs[i] * if i == axis { half[i] } else { inner[i] };
							}
							p
						})
						.collect();
					emit(self, points);
				}
			}
		}
		self.pop();
	}

	#[allow(clippy::too_many_arguments)]
	pub fn cylinder(&mut self, center: V3, radius: f64, height: f64, material: &str, sides: usize, rotation: V3, radius_top: Option<f64>,
		caps: bool, cap_material: Option<&str>, arc: [f64; 2]) {
		self.push_at(center, rotation);
		let rt = radius_top.unwrap_or(radius);
		let h = height * 0.5;
		let full = (arc[1] - arc[0] - std::f64::consts::TAU).abs() < 1e-6;
		let steps = if full { sides } else { sides.max(1) };
		let angles: Vec<f64> = (0..=steps).map(|i| arc[0] + (arc[1] - arc[0]) * i as f64 / steps as f64).collect();
		let tiles = self.tiles(material);
		for i in 0..steps {
			let (a0, a1) = (angles[i], angles[i + 1]);
			let p = [
				[radius * a0.cos(), radius * a0.sin(), -h], [radius * a1.cos(), radius * a1.sin(), -h],
				[rt * a1.cos(), rt * a1.sin(), h], [rt * a0.cos(), rt * a0.sin(), h],
			];
			let (u0, u1) = (i as f64 / steps as f64, (i + 1) as f64 / steps as f64);
			let circumference = radius.max(rt) * (arc[1] - arc[0]);
			let uv = cyl_uv(u0, u1, circumference, height, tiles);
			self.face(&p, material, FaceSpec { uv: Some(uv), tiled: true, ..Default::default() });
		}
		if caps && full {
			let top: Vec<V3> = angles[..angles.len() - 1].iter().map(|a| [rt * a.cos(), rt * a.sin(), h]).collect();
			let bottom: Vec<V3> = angles[..angles.len() - 1].iter().rev().map(|a| [radius * a.cos(), radius * a.sin(), -h]).collect();
			let cap = cap_material.unwrap_or(material);
			if rt > 0.0005 {
				self.plain(&top, cap);
			}
			if radius > 0.0005 {
				self.plain(&bottom, cap);
			}
		}
		self.pop();
	}

	/// A printed sleeve on a cylindrical shell; full normalised UVs, front at u = .5. Open ends.
	pub fn wrapped_panel(&mut self, center: V3, radius: f64, height: f64, material: &str, sides: usize, rotation: V3) -> Result<(), String> {
		if radius <= 0.0 || height <= 0.0 || !(3..=48).contains(&sides) {
			return Err("Wrapped panel needs positive radius/height and 3-48 sides".into());
		}
		self.push_at(center, rotation);
		let h = height * 0.5;
		let u_offset = -0.25 + rotation[2] / 360.0;
		for index in 0..sides {
			let a0 = std::f64::consts::TAU * index as f64 / sides as f64;
			let a1 = std::f64::consts::TAU * (index + 1) as f64 / sides as f64;
			let points = [
				[radius * a0.cos(), radius * a0.sin(), -h], [radius * a1.cos(), radius * a1.sin(), -h],
				[radius * a1.cos(), radius * a1.sin(), h], [radius * a0.cos(), radius * a0.sin(), h],
			];
			let (u0, u1) = (index as f64 / sides as f64 + u_offset, (index + 1) as f64 / sides as f64 + u_offset);
			self.face(&points, material, FaceSpec { uv: Some(vec![[u0, 0.0], [u1, 0.0], [u1, 1.0], [u0, 1.0]]), ..Default::default() });
		}
		self.pop();
		Ok(())
	}

	pub fn cone(&mut self, center: V3, radius: f64, height: f64, material: &str, sides: usize, rotation: V3) {
		self.cylinder(center, radius, height, material, sides, rotation, Some(0.0), true, None, [0.0, std::f64::consts::TAU]);
	}

	/// A closed outline [(u, v)] pushed width through, centred on center along axis (x: u = y, v = z;
	/// y: u = x, v = z; z: u = x, v = y). Concave outlines and either winding; taper narrows the width
	/// toward the highest v.
	pub fn extrude(&mut self, center: V3, outline: &[[f64; 2]], width: f64, material: &str, axis: char, taper: f64, rotation: V3)
		-> Result<(), String> {
		if !matches!(axis, 'x' | 'y' | 'z') {
			return Err("extrude axis: x, y or z".into());
		}
		let mut points: Vec<[f64; 2]> = outline.to_vec();
		if points.len() > 1 && points[0] == points[points.len() - 1] {
			points.pop();
		}
		if points.len() < 3 {
			return Err("an outline needs 3 or more points".into());
		}
		let n = points.len();
		let area = py::sum((0..n).map(|i| {
			let (a, b) = (points[i], points[(i + 1) % n]);
			a[0] * b[1] - b[0] * a[1]
		}));
		if area.abs() < 1e-10 {
			return Err("the outline encloses no area".into());
		}
		if area < 0.0 {
			points.reverse();
		}
		if self_crossing(&points) {
			return Err("the outline crosses itself".into());
		}
		let triangles = ear_clip(&points)?;
		let low = points.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min);
		let high = points.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max);
		let place = |point: [f64; 2], side: f64| -> V3 {
			let [u, v] = point;
			let t = if high > low { (v - low) / (high - low) } else { 0.0 };
			let w = side * width * 0.5 * (1.0 + (taper - 1.0) * t);
			match axis {
				'x' => [w, u, v],
				'y' => [u, w, v],
				_ => [u, v, w],
			}
		};
		let mirrored = axis == 'y';
		self.push_at(center, rotation);
		for i in 0..points.len() {
			let (a, b) = (points[i], points[(i + 1) % points.len()]);
			let mut quad = vec![place(a, -1.0), place(b, -1.0), place(b, 1.0), place(a, 1.0)];
			if mirrored {
				quad.reverse();
			}
			self.plain(&quad, material);
		}
		for side in [1.0, -1.0] {
			let group = next_group();
			for tri in &triangles {
				let mut corners: Vec<V3> = tri.iter().map(|&i| place(points[i], side)).collect();
				if (side < 0.0) != mirrored {
					corners.reverse();
				}
				self.face(&corners, material, FaceSpec { group: Some(group), ..Default::default() });
			}
		}
		self.pop();
		Ok(())
	}

	/// Revolve [(radius, z)] round local Z.
	#[allow(clippy::too_many_arguments)]
	pub fn lathe(&mut self, profile: &[[f64; 2]], material: &str, sides: usize, center: V3, arc: [f64; 2], cap: bool, rotation: V3) {
		self.push_at(center, rotation);
		let full = (arc[1] - arc[0] - std::f64::consts::TAU).abs() < 1e-6;
		let angles: Vec<f64> = (0..=sides).map(|i| arc[0] + (arc[1] - arc[0]) * i as f64 / sides as f64).collect();
		let mut total = 0.0;
		let mut lengths = vec![0.0];
		for pair in profile.windows(2) {
			total += py::hypot2(pair[1][0] - pair[0][0], pair[1][1] - pair[0][1]);
			lengths.push(total);
		}
		let (tile, tile_v) = self.tiles(material);
		for j in 0..profile.len().saturating_sub(1) {
			let ([r0, z0], [r1, z1]) = (profile[j], profile[j + 1]);
			for i in 0..sides {
				let (a0, a1) = (angles[i], angles[i + 1]);
				let p = [
					[r0 * a0.cos(), r0 * a0.sin(), z0], [r0 * a1.cos(), r0 * a1.sin(), z0],
					[r1 * a1.cos(), r1 * a1.sin(), z1], [r1 * a0.cos(), r1 * a0.sin(), z1],
				];
				let circ = r0.max(r1).max(0.01) * (arc[1] - arc[0]);
				let u0 = i as f64 / sides as f64 * circ / tile;
				let u1 = (i + 1) as f64 / sides as f64 * circ / tile;
				let v0 = lengths[j] / tile_v;
				let v1 = lengths[j + 1] / tile_v;
				self.face(&p, material, FaceSpec { uv: Some(vec![[u0, v0], [u1, v0], [u1, v1], [u0, v1]]), tiled: true, ..Default::default() });
			}
		}
		if cap && full {
			let [r, z] = profile[profile.len() - 1];
			if r > 0.001 {
				let points: Vec<V3> = angles[..angles.len() - 1].iter().map(|a| [r * a.cos(), r * a.sin(), z]).collect();
				self.plain(&points, material);
			}
		}
		self.pop();
	}

	/// A 2D profile [(x, y)] along a 3D path (profile x = side, y = up).
	#[allow(clippy::too_many_arguments)]
	pub fn sweep(&mut self, profile: &[[f64; 2]], path: &[V3], material: &str, closed_path: bool, closed_profile: bool, up: V3, twist: f64,
		scales: Option<&[f64]>) {
		let count = path.len();
		let mut frames = Vec::with_capacity(count);
		for (i, &p) in path.iter().enumerate() {
			let (nxt, prv) = if closed_path {
				(path[(i + 1) % count], path[(i + count - 1) % count])
			} else {
				(path[(i + 1).min(count - 1)], path[i.saturating_sub(1)])
			};
			let tangent = normalized(sub(nxt, prv));
			let mut side = cross(tangent, up);
			if length(side) < 1e-5 {
				side = cross(tangent, [1.0, 0.0, 0.0]);
			}
			side = normalized(side);
			let mut normal = normalized(cross(side, tangent));
			let angle = twist * i as f64 / (count.saturating_sub(1)).max(1) as f64;
			if angle != 0.0 {
				let rot = maths::M3::rotation_axis(angle, tangent);
				side = rot.apply(side);
				normal = rot.apply(normal);
			}
			frames.push((p, side, normal));
		}
		let rings: Vec<Vec<V3>> = frames
			.iter()
			.enumerate()
			.map(|(i, (p, side, normal))| {
				let s = scales.map(|v| v[i]).unwrap_or(1.0);
				profile.iter().map(|[x, y]| add(add(*p, scale(*side, x * s)), scale(*normal, y * s))).collect()
			})
			.collect();
		let segments = if closed_path { count } else { count - 1 };
		let plen = profile.len();
		let edges = if closed_profile { plen } else { plen - 1 };
		let tile = self.tiles(material).0;
		let mut dist = vec![0.0];
		let upto = count + usize::from(closed_path);
		for i in 1..upto {
			let d = length(sub(path[i % count], path[i - 1]));
			dist.push(dist[dist.len() - 1] + d);
		}
		let mut prof_len = vec![0.0];
		for k in 1..=plen {
			let a = profile[k % plen];
			let b = profile[k - 1];
			let d = py::sum([(a[0] - b[0]) * (a[0] - b[0]), (a[1] - b[1]) * (a[1] - b[1])]).sqrt();
			prof_len.push(prof_len[prof_len.len() - 1] + d);
		}
		for i in 0..segments {
			let ra = &rings[i];
			let rb = &rings[(i + 1) % count];
			for k in 0..edges {
				let k1 = (k + 1) % plen;
				let pts = [ra[k], rb[k], rb[k1], ra[k1]];
				let uv = vec![
					[dist[i] / tile, prof_len[k] / tile], [dist[i + 1] / tile, prof_len[k] / tile],
					[dist[i + 1] / tile, prof_len[k + 1] / tile], [dist[i] / tile, prof_len[k + 1] / tile],
				];
				self.face(&pts, material, FaceSpec { uv: Some(uv), tiled: true, ..Default::default() });
			}
		}
		if !closed_path && closed_profile {
			let first: Vec<V3> = rings[0].iter().rev().copied().collect();
			self.plain(&first, material);
			let last = rings[rings.len() - 1].clone();
			self.plain(&last, material);
		}
	}

	/// Vertical quad facing local -Y with 0..1 UVs unless given.
	pub fn panel(&mut self, center: V3, width: f64, height: f64, material: &str, rotation: V3, uv: Option<Vec<[f64; 2]>>, double: bool) {
		self.push_at(center, rotation);
		let (w, h) = (width * 0.5, height * 0.5);
		let pts = [[-w, 0.0, -h], [w, 0.0, -h], [w, 0.0, h], [-w, 0.0, h]];
		let uv = uv.unwrap_or_else(|| vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
		self.face(&pts, material, FaceSpec { uv: Some(uv.clone()), ..Default::default() });
		if double {
			let back: Vec<V3> = pts.iter().rev().map(|p| [p[0], 0.001, p[2]]).collect();
			let back_uv: Vec<[f64; 2]> = uv.iter().rev().copied().collect();
			self.face(&back, material, FaceSpec { uv: Some(back_uv), ..Default::default() });
		}
		self.pop();
	}

	/// Decal: a panel with UVs from the first atlas that names the cell.
	#[allow(clippy::too_many_arguments)]
	pub fn trim(&mut self, center: V3, width: f64, height: f64, cell: &str, atlases: &[Atlas], rotation: V3, material: Option<&str>, mirror: bool)
		-> Result<(), String> {
		let Some(atlas) = atlases.iter().find(|a| a.cells.iter().any(|(n, _)| n == cell)) else {
			return Err(format!("no decal sheet has a cell {}", py::repr_str(cell)));
		};
		let [col, row, w, h] = atlas.cells.iter().find(|(n, _)| n == cell).unwrap().1;
		let (mut u0, mut u1) = (col / atlas.grid, (col + w) / atlas.grid);
		let (v1, v0) = (1.0 - row / atlas.grid, 1.0 - (row + h) / atlas.grid);
		if mirror {
			std::mem::swap(&mut u0, &mut u1);
		}
		let mat = match material {
			Some(m) => m.to_string(),
			None if atlas.emissive.iter().any(|c| c == cell) && !atlas.emit_material.is_empty() => atlas.emit_material.clone(),
			None => atlas.material.clone(),
		};
		self.panel(center, width, height, &mat, rotation, Some(vec![[u0, v0], [u1, v0], [u1, v1], [u0, v1]]), false);
		Ok(())
	}

	pub fn triangle_count(&self) -> usize {
		self.faces.iter().map(|f| f.points.len() - 2).sum()
	}

	/// ((min x, y, z), (max x, y, z)) of every corner.
	pub fn bounds(&self) -> (V3, V3) {
		bounds_of(self.faces.iter())
	}
}

pub fn bounds_of<'a>(faces: impl Iterator<Item = &'a Face>) -> (V3, V3) {
	let mut lo = [f64::INFINITY; 3];
	let mut hi = [f64::NEG_INFINITY; 3];
	for f in faces {
		for p in &f.points {
			for k in 0..3 {
				if p[k] < lo[k] {
					lo[k] = p[k];
				}
				if p[k] > hi[k] {
					hi[k] = p[k];
				}
			}
		}
	}
	(lo, hi)
}

/// A local point scaled and shifted by its height: untouched at the bottom, by taper and lean at the top.
pub fn taper_point(point: V3, half_height: f64, taper: [f64; 2], lean: [f64; 2]) -> V3 {
	if half_height <= 0.0 || (taper == [1.0, 1.0] && lean == [0.0, 0.0]) {
		return point;
	}
	let t = (point[2] + half_height) / (2.0 * half_height);
	[point[0] * (1.0 + (taper[0] - 1.0) * t) + lean[0] * t, point[1] * (1.0 + (taper[1] - 1.0) * t) + lean[1] * t, point[2]]
}

fn self_crossing(points: &[[f64; 2]]) -> bool {
	let side = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
	let count = points.len();
	for i in 0..count {
		for j in (i + 2)..count {
			if i == 0 && j == count - 1 {
				continue;
			}
			let (a, b) = (points[i], points[(i + 1) % count]);
			let (c, d) = (points[j], points[(j + 1) % count]);
			if side(a, b, c) * side(a, b, d) < -1e-12 && side(c, d, a) * side(c, d, b) < -1e-12 {
				return true;
			}
		}
	}
	false
}

/// Index triples covering a simple counter-clockwise polygon, concave or not.
fn ear_clip(points: &[[f64; 2]]) -> Result<Vec<[usize; 3]>, String> {
	let cross2 = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
	let inside = |q: [f64; 2], a: [f64; 2], b: [f64; 2], c: [f64; 2]| cross2(a, b, q) >= -1e-12 && cross2(b, c, q) >= -1e-12 && cross2(c, a, q) >= -1e-12;
	let mut left: Vec<usize> = (0..points.len()).collect();
	let mut out = Vec::new();
	while left.len() > 3 {
		let count = left.len();
		let mut found = false;
		for k in 0..count {
			let (i0, i1, i2) = (left[(k + count - 1) % count], left[k], left[(k + 1) % count]);
			let (a, b, c) = (points[i0], points[i1], points[i2]);
			if cross2(a, b, c) <= 1e-12 {
				continue;
			}
			if left.iter().any(|&j| j != i0 && j != i1 && j != i2 && points[j] != a && points[j] != b && points[j] != c && inside(points[j], a, b, c)) {
				continue;
			}
			out.push([i0, i1, i2]);
			left.remove(k);
			found = true;
			break;
		}
		if !found {
			let flat = (0..count).find(|&k| cross2(points[left[(k + count - 1) % count]], points[left[k]], points[left[(k + 1) % count]]).abs() <= 1e-12);
			match flat {
				Some(k) => {
					left.remove(k);
				}
				None => return Err("the outline crosses itself".into()),
			}
		}
	}
	if left.len() == 3 && cross2(points[left[0]], points[left[1]], points[left[2]]).abs() > 1e-12 {
		out.push([left[0], left[1], left[2]]);
	}
	Ok(out)
}

/// smoothstep from a to b.
pub fn smooth(a: f64, b: f64, x: f64) -> f64 {
	let t = ((x - a) / (b - a).max(1e-6)).clamp(0.0, 1.0);
	t * t * (3.0 - 2.0 * t)
}

/// A face's UVs: its own (scaled when they are metric), else the box projection at the part's density.
pub fn face_uvs(face: &Face, coords: &[V3], normal: V3, mat: &Mat, uv_scale: f64) -> Vec<[f64; 2]> {
	if let Some(uv) = &face.uv {
		if uv.len() == coords.len() {
			return if face.tiled && uv_scale != 1.0 { uv.iter().map(|[u, v]| [u * uv_scale, v * uv_scale]).collect() } else { uv.clone() };
		}
	}
	let (tile, tile_v) = mat.tiles();
	box_uv(coords, normal, tile / uv_scale, tile_v / uv_scale)
}

fn box_uv(coords: &[V3], normal: V3, tile: f64, tile_v: f64) -> Vec<[f64; 2]> {
	let tv = if tile_v != 0.0 { tile_v } else { tile };
	let mut axis = 0;
	for i in 1..3 {
		if normal[i].abs() > normal[axis].abs() {
			axis = i;
		}
	}
	match axis {
		0 => coords.iter().map(|c| [c[1] / tile * if normal[0] > 0.0 { 1.0 } else { -1.0 }, c[2] / tv]).collect(),
		1 => coords.iter().map(|c| [c[0] / tile * if normal[1] > 0.0 { -1.0 } else { 1.0 }, c[2] / tv]).collect(),
		_ => coords.iter().map(|c| [c[0] / tile, c[1] / tv]).collect(),
	}
}

fn cyl_uv(u0: f64, u1: f64, circumference: f64, height: f64, tiles: (f64, f64)) -> Vec<[f64; 2]> {
	let (tile, tile_v) = tiles;
	let uu0 = u0 * circumference / tile;
	let uu1 = u1 * circumference / tile;
	let vv = height / tile_v;
	vec![[uu0, 0.0], [uu1, 0.0], [uu1, vv], [uu0, vv]]
}
