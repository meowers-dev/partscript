//! Surfaces: what is under a point (for things that settle onto the ground) and points spread over
//! faces (for things that grow on them: moss on tops, ivy up walls, rivets on a hull).

use std::collections::HashMap;

use crate::maths::V3;
use crate::py::{self, PyRandom};

fn fan(points: &[V3]) -> impl Iterator<Item = (V3, V3, V3)> + '_ {
	(1..points.len().saturating_sub(1)).map(move |k| (points[0], points[k], points[k + 1]))
}

/// (unit normal, area) of a triangle; zero for a degenerate one.
fn normal(a: V3, b: V3, c: V3) -> (V3, f64) {
	let (ux, uy, uz) = (b[0] - a[0], b[1] - a[1], b[2] - a[2]);
	let (vx, vy, vz) = (c[0] - a[0], c[1] - a[1], c[2] - a[2]);
	let n = [uy * vz - uz * vy, uz * vx - ux * vz, ux * vy - uy * vx];
	let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
	if length > 1e-12 {
		([n[0] / length, n[1] / length, n[2] / length], length / 2.0)
	} else {
		([0.0; 3], 0.0)
	}
}

type Tri = (V3, V3, V3, V3);

/// The up-facing triangles of some faces, found by where they lie in plan.
pub struct SurfaceIndex {
	cell: f64,
	buckets: HashMap<(i64, i64), Vec<Tri>>,
}

impl SurfaceIndex {
	pub fn new<'a>(polygons: impl IntoIterator<Item = &'a Vec<V3>>, cell: f64) -> SurfaceIndex {
		let mut index = SurfaceIndex { cell, buckets: HashMap::new() };
		for points in polygons {
			index.add(points);
		}
		index
	}

	pub fn add(&mut self, points: &[V3]) {
		for (a, b, c) in fan(points) {
			let (n, area) = normal(a, b, c);
			if n[2] < 0.15 || area <= 0.0 {
				continue;
			}
			let (x0, x1) = (py::min2(py::min2(a[0], b[0]), c[0]), py::max2(py::max2(a[0], b[0]), c[0]));
			let (y0, y1) = (py::min2(py::min2(a[1], b[1]), c[1]), py::max2(py::max2(a[1], b[1]), c[1]));
			let tri = (a, b, c, n);
			for i in (x0 / self.cell).floor() as i64..=(x1 / self.cell).floor() as i64 {
				for j in (y0 / self.cell).floor() as i64..=(y1 / self.cell).floor() as i64 {
					self.buckets.entry((i, j)).or_default().push(tri);
				}
			}
		}
	}

	/// Every up-facing surface over (x, y): [(z, normal)], highest first.
	pub fn heights(&self, x: f64, y: f64) -> Vec<(f64, V3)> {
		let mut out = Vec::new();
		if let Some(tris) = self.buckets.get(&((x / self.cell).floor() as i64, (y / self.cell).floor() as i64)) {
			for (a, b, c, n) in tris {
				let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
				if d.abs() < 1e-12 {
					continue;
				}
				let l1 = ((b[1] - c[1]) * (x - c[0]) + (c[0] - b[0]) * (y - c[1])) / d;
				let l2 = ((c[1] - a[1]) * (x - c[0]) + (a[0] - c[0]) * (y - c[1])) / d;
				let l3 = 1.0 - l1 - l2;
				if py::min2(py::min2(l1, l2), l3) < -1e-6 {
					continue;
				}
				out.push((l1 * a[2] + l2 * b[2] + l3 * c[2], *n));
			}
		}
		out.sort_by(|p, q| q.0.partial_cmp(&p.0).unwrap_or(std::cmp::Ordering::Equal));
		out
	}

	/// The first surface at or below z over (x, y), else the lowest one above it; None when there is none.
	pub fn below(&self, x: f64, y: f64, z: f64) -> Option<(f64, V3)> {
		let hits = self.heights(x, y);
		hits.iter().find(|hit| hit.0 <= z + 1e-6).copied().or_else(|| hits.last().copied())
	}
}

fn keep(facing: &str, n: V3) -> bool {
	match facing {
		"up" => n[2] > 0.7,
		"down" => n[2] < -0.7,
		"side" => n[2].abs() < 0.35,
		_ => true,
	}
}

/// count points over the polygons' triangles, by area, on those facing the given way: [(point, normal)].
/// Points keep apart where they can.
pub fn spread<'a>(polygons: impl IntoIterator<Item = &'a Vec<V3>>, count: usize, rng: &mut PyRandom, facing: &str, apart: f64) -> Vec<(V3, V3)> {
	let mut triangles: Vec<(f64, V3, V3, V3, V3)> = Vec::new();
	let mut total = 0.0;
	for points in polygons {
		for (a, b, c) in fan(points) {
			let (n, area) = normal(a, b, c);
			if area > 0.0 && keep(facing, n) {
				total += area;
				triangles.push((total, a, b, c, n));
			}
		}
	}
	if triangles.is_empty() {
		return Vec::new();
	}
	let mut out: Vec<(V3, V3)> = Vec::with_capacity(count);
	for _ in 0..count {
		let mut chosen = ([0.0; 3], [0.0; 3]);
		for _attempt in 0..30 {
			let pick = rng.random() * total;
			let (mut lo, mut hi) = (0usize, triangles.len() - 1);
			while lo < hi {
				let mid = (lo + hi) / 2;
				if triangles[mid].0 < pick {
					lo = mid + 1;
				} else {
					hi = mid;
				}
			}
			let (_, a, b, c, n) = triangles[lo];
			let (mut r1, mut r2) = (rng.random(), rng.random());
			if r1 + r2 > 1.0 {
				r1 = 1.0 - r1;
				r2 = 1.0 - r2;
			}
			let point = [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * r1 + (c[k] - a[k]) * r2);
			chosen = (point, n);
			if out.iter().all(|(q, _)| py::dist(&point, q) >= apart) {
				break;
			}
		}
		out.push(chosen);
	}
	out
}
