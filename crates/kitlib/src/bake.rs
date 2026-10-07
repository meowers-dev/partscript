//! Bake: a Part's faces into a mesh ready to write (deduped corners, UVs, baked vertex tone).
//!
//! The tone is the house shading: a grounded darkening that fades over ao_height metres, a face-facing
//! shade, a small seeded per-face jitter, and any per-corner tone the face carries. Triangulation and
//! normals follow Blender's, so a model looks the same imported there.

use std::collections::{HashMap, HashSet};

use crate::geom::{face_uvs, smooth, Face, Part};
use crate::hash::crc32;
use crate::maths::{cross, dot, length_squared, scale, sub, V3};
use crate::py::{self, PyRandom};

/// One kept face, baked.
#[derive(Clone, Debug)]
pub struct Polygon {
	pub face: Face,
	pub ids: Vec<usize>,
	pub step: i64,
	pub origin: usize,
	pub coords: Vec<V3>,
	pub normal: V3,
	pub uvs: Vec<[f64; 2]>,
	pub colours: Vec<f64>,
	pub triangles: Vec<[usize; 3]>,
	pub folded: bool,
}

/// A part ready to write.
#[derive(Clone, Debug)]
pub struct Baked {
	pub name: String,
	pub smooth: bool,
	pub materials: Vec<String>,
	pub polygons: Vec<Polygon>,
}

impl Baked {
	pub fn triangles(&self) -> usize {
		self.polygons.iter().map(|p| p.ids.len() - 2).sum()
	}
}

/// The face normal as Blender computes it: two edges' cross for a triangle, the diagonals' for a quad,
/// Newell's method beyond; +Z when the face has no area.
pub fn polygon_normal(points: &[V3]) -> V3 {
	let normal = if points.len() == 4 {
		cross(sub(points[0], points[2]), sub(points[1], points[3]))
	} else if points.len() == 3 {
		cross(sub(points[0], points[1]), sub(points[1], points[2]))
	} else {
		let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
		let mut prev = points[points.len() - 1];
		for &curr in points {
			x += (prev[1] - curr[1]) * (prev[2] + curr[2]);
			y += (prev[2] - curr[2]) * (prev[0] + curr[0]);
			z += (prev[0] - curr[0]) * (prev[1] + curr[1]);
			prev = curr;
		}
		[x, y, z]
	};
	let squared = length_squared(normal);
	if squared > 1.0e-35 {
		scale(normal, 1.0 / squared.sqrt())
	} else {
		[0.0, 0.0, 1.0]
	}
}

/// Corner indices of a polygon's triangles as Blender tessellates it: a fan from the first corner,
/// except a quad that would fold along 0-2 is split along 1-3.
pub fn triangles(points: &[V3]) -> Vec<[usize; 3]> {
	if points.len() == 4 {
		let (d12, d13, d14) = (sub(points[1], points[0]), sub(points[2], points[0]), sub(points[3], points[0]));
		if dot(cross(d12, d13), cross(d14, d13)) > 0.0 {
			return vec![[0, 1, 3], [1, 2, 3]];
		}
	}
	(1..points.len() - 1).map(|k| [0, k, k + 1]).collect()
}

/// A Part ready to write: duplicate and degenerate faces dropped, materials in slot order, and per kept
/// face its corners with UVs and the baked tone. face_steps is each face's step (source statement).
pub fn bake(part: &Part, ao_height: f64, face_steps: Option<&[i64]>) -> Baked {
	let mut verts: Vec<V3> = Vec::new();
	let mut index: HashMap<[u64; 3], usize> = HashMap::new();
	let mut kept: Vec<(usize, Vec<usize>, i64)> = Vec::new();
	let mut seen: HashSet<Vec<usize>> = HashSet::new();
	for (n, face) in part.faces.iter().enumerate() {
		let step = face_steps.map(|s| s[n]).unwrap_or(0);
		let mut ids: Vec<usize> = Vec::with_capacity(face.points.len());
		for point in &face.points {
			let key = [py::round_to(point[0], 5), py::round_to(point[1], 5), py::round_to(point[2], 5)];
			let bits = [key[0].to_bits(), key[1].to_bits(), key[2].to_bits()];
			// -0.0 and 0.0 are one key, as Python's tuple equality has it
			let bits = bits.map(|b| if b == (-0.0f64).to_bits() { 0 } else { b });
			// a NaN is equal to nothing, itself included: such a corner is always a vertex of its own
			let id = if key.iter().any(|v| v.is_nan()) {
				verts.push(key);
				verts.len() - 1
			} else {
				*index.entry(bits).or_insert_with(|| {
					verts.push(key);
					verts.len() - 1
				})
			};
			if ids.last() != Some(&id) {
				ids.push(id);
			}
		}
		if ids.len() > 1 && ids[0] == ids[ids.len() - 1] {
			ids.pop();
		}
		let mut signature = ids.clone();
		signature.sort_unstable();
		let distinct = {
			let mut d = signature.clone();
			d.dedup();
			d.len()
		};
		if distinct < 3 || distinct != ids.len() || seen.contains(&signature) {
			continue;
		}
		seen.insert(signature);
		kept.push((n, ids, step));
	}
	let mut material_names: Vec<String> = kept.iter().map(|(n, _, _)| part.faces[*n].material.to_string()).collect();
	material_names.sort();
	material_names.dedup();
	if let Some(lead) = &part.lead_material {
		if let Some(pos) = material_names.iter().position(|m| m == lead) {
			let m = material_names.remove(pos);
			material_names.insert(0, m);
		}
	}
	let zmin = py::min_iter(verts.iter().map(|v| v[2])).unwrap_or(0.0);
	let ao_height = part.ao_height.unwrap_or(ao_height);
	let mut rng = PyRandom::from_int(crc32(part.name.as_bytes()) as i128);
	let mut tones: HashMap<u64, f64> = HashMap::new();
	let mut polygons = Vec::with_capacity(kept.len());
	let materials = part.materials.borrow();
	for (n, ids, step) in kept {
		let face = &part.faces[n];
		let mat = materials.get(&*face.material).cloned().unwrap_or_else(|| crate::geom::Mat::new("", 2.0));
		let coords: Vec<V3> = ids.iter().map(|&i| verts[i]).collect();
		let normal = polygon_normal(&coords);
		let uvs = face_uvs(face, &coords, normal, &mat, part.uv_scale);
		let tone = match face.group.and_then(|g| tones.get(&g).copied()) {
			Some(t) => t,
			None => {
				let t = face.shade * (1.0 + rng.uniform(-0.04, 0.04));
				if let Some(g) = face.group {
					tones.insert(g, t);
				}
				t
			}
		};
		let corners: Vec<f64> = match &face.corner_shade {
			Some(c) if c.len() == coords.len() => c.clone(),
			_ => vec![1.0; coords.len()],
		};
		let colours: Vec<f64> = coords
			.iter()
			.zip(&corners)
			.map(|(co, corner)| {
				if !mat.ao {
					return 1.0;
				}
				let ground = if ao_height > 0.0 { 0.62 + 0.38 * smooth(0.0, ao_height, co[2] - zmin) } else { 1.0 };
				let facing = if normal[2] > -0.5 { 0.86 + 0.14 * py::max2(0.0, normal[2]) } else { 0.74 };
				py::max2(0.0, py::min2(1.0, ground * facing * tone * corner))
			})
			.collect();
		let tris = triangles(&coords);
		let folded = coords.len() == 4 && tris[0] == [0, 1, 3];
		polygons.push(Polygon { face: face.clone(), ids, step, origin: 0, coords, normal, uvs, colours, triangles: tris, folded });
	}
	Baked { name: part.name.clone(), smooth: part.smooth, materials: material_names, polygons }
}

/// Vertex normals for a smooth part: face normals weighted by the corner angle, as Blender does.
pub fn smooth_normals(polygons: &[Polygon]) -> HashMap<usize, V3> {
	let mut totals: HashMap<usize, V3> = HashMap::new();
	for polygon in polygons {
		let (coords, ids) = (&polygon.coords, &polygon.ids);
		let n = coords.len();
		for (k, &vertex) in ids.iter().enumerate() {
			let a = crate::maths::normalized(sub(coords[(k + n - 1) % n], coords[k]));
			let b = crate::maths::normalized(sub(coords[(k + 1) % n], coords[k]));
			let angle = py::max2(-1.0, py::min2(1.0, dot(a, b))).acos();
			let entry = totals.entry(vertex).or_insert([0.0; 3]);
			*entry = crate::maths::add(*entry, scale(polygon.normal, angle));
		}
	}
	totals.into_iter().map(|(k, v)| (k, crate::maths::normalized(v))).collect()
}
