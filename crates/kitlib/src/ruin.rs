//! Broken boxes: a wall or a block with chunks knocked out of it, crumbling edges and the rubble that fell.
//!
//! broken_box() cuts a box into a lattice of chunks, knocks out a share of them (most from the top and the
//! ends, in noisy bites), lets go of any that no longer reach the ground through the rest, and draws what
//! is left: the box's own faces where they survive (merged into big quads where nothing nearby broke) and
//! rough broken faces, in a core material, where chunks came away.
//!
//! The cells kept are walked in the order Python's set of them iterates in (py::PySet), which is the
//! order the faces were always drawn in.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::geom::Part;
use crate::maths::V3;
use crate::noise::rough;
use crate::py::{self, PyRandom, PySet};

type Cell = (i64, i64, i64);

const SIDES: [([i64; 3], [[i64; 3]; 4]); 6] = [
	([1, 0, 0], [[1, 0, 0], [1, 1, 0], [1, 1, 1], [1, 0, 1]]),
	([-1, 0, 0], [[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]]),
	([0, 1, 0], [[0, 1, 0], [0, 1, 1], [1, 1, 1], [1, 1, 0]]),
	([0, -1, 0], [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]]),
	([0, 0, 1], [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]]),
	([0, 0, -1], [[0, 0, 0], [0, 1, 0], [1, 1, 0], [1, 0, 0]]),
];

fn get(c: Cell, a: usize) -> i64 {
	[c.0, c.1, c.2][a]
}

/// A box of size centred on the part's origin with amount (0-1) of it knocked out in chunks about chunk
/// across. Draws into part; returns the fallen chunks as [(centre, size)] in the same space.
#[allow(clippy::too_many_arguments)]
pub fn broken_box(part: &mut Part, size: V3, material: &str, amount: f64, chunk: f64, seed: &str, core: Option<&str>) -> Vec<(V3, V3)> {
	let core = core.unwrap_or(material);
	let n: [i64; 3] = size.map(|s| (py::round(s / chunk) as i64).max(1));
	let cell: V3 = [size[0] / n[0] as f64, size[1] / n[1] as f64, size[2] / n[2] as f64];
	let half: V3 = size.map(|s| s / 2.0);
	let mut rng = PyRandom::from_str(&format!("ruin#{seed}"));
	let mut cells: Vec<Cell> = Vec::new();
	for k in 0..n[2] {
		for j in 0..n[1] {
			for i in 0..n[0] {
				cells.push((i, j, k));
			}
		}
	}
	let centre = |c: Cell| -> V3 {
		[-half[0] + (c.0 as f64 + 0.5) * cell[0], -half[1] + (c.1 as f64 + 0.5) * cell[1], -half[2] + (c.2 as f64 + 0.5) * cell[2]]
	};
	let scale = 1.0 / (chunk * 4.5).max(1e-6);
	let mut scores: HashMap<Cell, f64> = HashMap::new();
	for &c in &cells {
		let [x, y, z] = centre(c);
		let top = (c.2 as f64 + 0.5) / n[2] as f64;
		let end = ((c.0 as f64 + 0.5) / n[0] as f64 * 2.0 - 1.0).abs();
		let score = rough(x * scale, y * scale * 0.5, z * scale * 0.8, seed, 4) * 0.8 + top * 0.28 + end * 0.14 + rng.random() * 0.1;
		scores.insert(c, score);
	}
	let mut order = cells.clone();
	order.sort_by(|a, b| scores[b].partial_cmp(&scores[a]).unwrap_or(std::cmp::Ordering::Equal));
	let take = py::round(amount.clamp(0.0, 1.0) * cells.len() as f64) as usize;
	let gone_first: PySet<Cell> = order[..take.min(order.len())].iter().copied().collect();
	// Anything no longer joined to the bottom through the rest falls too.
	let kept_all = cells.iter().copied().collect::<PySet<Cell>>().difference(&gone_first);
	let mut reached: PySet<Cell> = PySet::default();
	let mut todo: Vec<Cell> = kept_all.iter().copied().filter(|c| c.2 == 0).collect();
	while let Some(c) = todo.pop() {
		if reached.contains(&c) {
			continue;
		}
		reached.add(c);
		for (d, _) in SIDES {
			let nb = (c.0 + d[0], c.1 + d[1], c.2 + d[2]);
			if kept_all.contains(&nb) && !reached.contains(&nb) {
				todo.push(nb);
			}
		}
	}
	let mut gone: BTreeSet<Cell> = gone_first.iter().copied().collect();
	gone.extend(kept_all.iter().copied().filter(|c| !reached.contains(c)));
	let kept = reached;
	let inside = |c: Cell| (0..n[0]).contains(&c.0) && (0..n[1]).contains(&c.1) && (0..n[2]).contains(&c.2);
	let mut damaged: HashSet<Cell> = HashSet::new();
	for &(i, j, k) in &gone {
		for di in 0..2 {
			for dj in 0..2 {
				for dk in 0..2 {
					damaged.insert((i + di, j + dj, k + dk));
				}
			}
		}
	}
	let mut moved: HashMap<Cell, V3> = HashMap::new();
	let mut point = |v: Cell| -> V3 {
		if let Some(p) = moved.get(&v) {
			return *p;
		}
		let mut base = [0, 1, 2].map(|a| -half[a] + get(v, a) as f64 * cell[a]);
		if damaged.contains(&v) {
			let mut jitter = PyRandom::from_str(&format!("ruin#{seed}#({}, {}, {})", v.0, v.1, v.2));
			for a in 0..3 {
				if 0 < get(v, a) && get(v, a) < n[a] {
					base[a] += jitter.uniform(-0.44, 0.44) * cell[a];
				}
			}
		}
		moved.insert(v, base);
		base
	};
	for (direction, offsets) in SIDES {
		let axis = (0..3).find(|&a| direction[a] != 0).unwrap();
		let other: Vec<usize> = (0..3).filter(|&a| a != axis).collect();
		let layer = if direction[axis] > 0 { n[axis] - 1 } else { 0 };
		let mut clean: Vec<((i64, i64), Cell)> = Vec::new();
		let lattice = |c: Cell, o: [i64; 3]| (c.0 + o[0], c.1 + o[1], c.2 + o[2]);
		for &c in kept.iter() {
			if get(c, axis) != layer {
				continue;
			}
			if offsets.iter().all(|o| !damaged.contains(&lattice(c, *o))) {
				let key = (get(c, other[0]), get(c, other[1]));
				if let Some(slot) = clean.iter_mut().find(|(k, _)| *k == key) {
					slot.1 = c;
				} else {
					clean.push((key, c));
				}
			} else {
				let corners: Vec<V3> = offsets.iter().map(|o| point(lattice(c, *o))).collect();
				quad(part, &corners, material);
			}
		}
		let cells_clean: HashSet<(i64, i64)> = clean.iter().map(|(k, _)| *k).collect();
		let lookup = |key: (i64, i64)| clean.iter().find(|(k, _)| *k == key).unwrap().1;
		for (u0, v0, u1, v1) in merge(&cells_clean, n[other[0]], n[other[1]]) {
			let (first, last) = (lookup((u0, v0)), lookup((u1, v1)));
			let box_lo = [0, 1, 2].map(|a| get(first, a).min(get(last, a)));
			let box_hi = [0, 1, 2].map(|a| get(first, a).max(get(last, a)) + 1);
			let points: Vec<V3> = offsets
				.iter()
				.map(|o| {
					let mut v = [0, 1, 2].map(|a| if o[a] == 0 { box_lo[a] } else { box_hi[a] });
					v[axis] = layer + i64::from(direction[axis] > 0);
					[0, 1, 2].map(|a| -half[a] + v[a] as f64 * cell[a])
				})
				.collect();
			part.plain(&points, material);
		}
		// Broken faces: a kept chunk beside a missing one.
		for &c in kept.iter() {
			let nb = (c.0 + direction[0], c.1 + direction[1], c.2 + direction[2]);
			if inside(nb) && gone.contains(&nb) {
				let corners: Vec<V3> = offsets.iter().map(|o| point(lattice(c, *o))).collect();
				quad(part, &corners, core);
			}
		}
	}
	gone.iter().map(|&c| (centre(c), cell)).collect()
}

/// A quad, as two triangles when nudged corners have bent it out of flat.
fn quad(part: &mut Part, points: &[V3], material: &str) {
	if flat(points) {
		part.plain(points, material);
	} else {
		part.plain(&[points[0], points[1], points[2]], material);
		part.plain(&[points[0], points[2], points[3]], material);
	}
}

fn flat(points: &[V3]) -> bool {
	let (a, b, c, d) = (points[0], points[1], points[2], points[3]);
	let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
	let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
	let w = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
	let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
	(n[0] * w[0] + n[1] * w[1] + n[2] * w[2]).abs() < 1e-9
}

/// Greedy rectangles covering cells (u, v): [(u0, v0, u1, v1)] inclusive.
fn merge(cells: &HashSet<(i64, i64)>, nu: i64, nv: i64) -> Vec<(i64, i64, i64, i64)> {
	let mut left = cells.clone();
	let mut out = Vec::new();
	for v in 0..nv {
		for u in 0..nu {
			if !left.contains(&(u, v)) {
				continue;
			}
			let mut u1 = u;
			while left.contains(&(u1 + 1, v)) {
				u1 += 1;
			}
			let mut v1 = v;
			while (u..=u1).all(|x| left.contains(&(x, v1 + 1))) {
				v1 += 1;
			}
			for x in u..=u1 {
				for y in v..=v1 {
					left.remove(&(x, y));
				}
			}
			out.push((u, v, u1, v1));
		}
	}
	out
}
