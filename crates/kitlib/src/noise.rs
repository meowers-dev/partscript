//! Smooth noise: Perlin gradient noise in 1-3D and a layered (fractal) version, seeded.
//!
//! noise() wanders smoothly between 0 and 1 (about .5 on average): hills, wear, where moss grows.
//! rough() adds finer layers of the same on top. The same seed and point always give the same value.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::py::PyRandom;

thread_local! {
	static TABLES: RefCell<HashMap<String, Rc<Vec<i64>>>> = RefCell::new(HashMap::new());
}

fn table(seed: &str) -> Rc<Vec<i64>> {
	TABLES.with(|tables| {
		if let Some(t) = tables.borrow().get(seed) {
			return t.clone();
		}
		let mut values: Vec<i64> = (0..256).collect();
		PyRandom::from_str(&format!("noise#{seed}")).shuffle(&mut values);
		let mut doubled = values.clone();
		doubled.extend(values);
		let t = Rc::new(doubled);
		tables.borrow_mut().insert(seed.to_string(), t.clone());
		t
	})
}

fn fade(t: f64) -> f64 {
	t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn grad(h: i64, x: f64, y: f64, z: f64) -> f64 {
	let h = h & 15;
	let u = if h < 8 { x } else { y };
	let v = if h < 4 { y } else if h == 12 || h == 14 { x } else { z };
	(if h & 1 == 0 { u } else { -u }) + (if h & 2 == 0 { v } else { -v })
}

fn lerp(t: f64, lo: f64, hi: f64) -> f64 {
	lo + t * (hi - lo)
}

/// Gradient noise about 0 (roughly -1..1), smooth, 1 unit across a bump.
pub fn perlin(x: f64, y: f64, z: f64, seed: &str) -> f64 {
	let p = table(seed);
	let (xi, yi, zi) = (x.floor(), y.floor(), z.floor());
	let (xf, yf, zf) = (x - xi, y - yi, z - zi);
	let (xi, yi, zi) = ((xi as i64) & 255, (yi as i64) & 255, (zi as i64) & 255);
	let (u, v, w) = (fade(xf), fade(yf), fade(zf));
	let (a, b) = (p[xi as usize] + yi, p[xi as usize + 1] + yi);
	let (aa, ab, ba, bb) = (p[a as usize] + zi, p[a as usize + 1] + zi, p[b as usize] + zi, p[b as usize + 1] + zi);
	let (aa, ab, ba, bb) = (aa as usize, ab as usize, ba as usize, bb as usize);
	lerp(
		w,
		lerp(v, lerp(u, grad(p[aa], xf, yf, zf), grad(p[ba], xf - 1.0, yf, zf)), lerp(u, grad(p[ab], xf, yf - 1.0, zf), grad(p[bb], xf - 1.0, yf - 1.0, zf))),
		lerp(
			v,
			lerp(u, grad(p[aa + 1], xf, yf, zf - 1.0), grad(p[ba + 1], xf - 1.0, yf, zf - 1.0)),
			lerp(u, grad(p[ab + 1], xf, yf - 1.0, zf - 1.0), grad(p[bb + 1], xf - 1.0, yf - 1.0, zf - 1.0)),
		),
	)
}

/// Smooth noise from 0 to 1.
pub fn noise(x: f64, y: f64, z: f64, seed: &str) -> f64 {
	(0.5 + 0.5 * perlin(x, y, z, seed) * 1.15).clamp(0.0, 1.0)
}

/// Layered noise from 0 to 1: each layer twice as fine and half as strong as the last.
pub fn rough(x: f64, y: f64, z: f64, seed: &str, octaves: usize) -> f64 {
	let (mut total, mut amplitude, mut frequency, mut norm) = (0.0, 1.0, 1.0, 0.0);
	for k in 0..octaves {
		let k = k as f64;
		total += amplitude * perlin(x * frequency + k * 17.3, y * frequency + k * 9.1, z * frequency + k * 5.7, seed);
		norm += amplitude;
		amplitude *= 0.5;
		frequency *= 2.0;
	}
	(0.5 + 0.5 * total / norm * 1.4).clamp(0.0, 1.0)
}
