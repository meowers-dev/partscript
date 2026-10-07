//! Animation as the .glb writer takes it: clips of sampled tracks on nodes, and events.
//!
//! Values are in the authoring space (Z up, +Y back, metres; rotations as quaternions x, y, z, w); the
//! writer turns them into glTF's axes. A clip holds only sampled keys: whatever made them (keyframes,
//! generators, solvers) has been baked down before it gets here, so a game needs no runtime library.

use crate::json::Json;
use crate::maths::{M3, M4, V3};

/// A rotation: x, y, z, w.
pub type Quat = [f64; 4];

pub const QUAT_IDENTITY: Quat = [0.0, 0.0, 0.0, 1.0];

/// How a sampler fills between keys (no CUBICSPLINE: the keys are already baked from the curves).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interp {
	Linear,
	Step,
}

impl Interp {
	pub fn gltf(self) -> &'static str {
		match self {
			Interp::Linear => "LINEAR",
			Interp::Step => "STEP",
		}
	}
}

/// What a track moves on its node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
	Translation,
	Rotation,
	Scale,
}

impl Channel {
	pub fn gltf(self) -> &'static str {
		match self {
			Channel::Translation => "translation",
			Channel::Rotation => "rotation",
			Channel::Scale => "scale",
		}
	}

	/// Numbers per key: 3, or 4 for a rotation.
	pub fn width(self) -> usize {
		if self == Channel::Rotation {
			4
		} else {
			3
		}
	}
}

/// One key: a time in seconds and a value (translation and scale use the first three numbers).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
	pub t: f64,
	pub v: [f64; 4],
}

impl Key {
	pub fn vec3(t: f64, v: V3) -> Key {
		Key { t, v: [v[0], v[1], v[2], 0.0] }
	}

	pub fn quat(t: f64, q: Quat) -> Key {
		Key { t, v: q }
	}
}

/// Keys for one channel of one node (an index into the nodes the writer is given).
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
	pub node: usize,
	pub channel: Channel,
	pub interp: Interp,
	pub keys: Vec<Key>,
}

/// A moment in a clip a game turns into a call: eject a casing at a mark, play a sound, a footstep.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
	pub t: f64,
	pub name: String,
	/// anything else the event carries (mark, throw, spin, sound...), written beside t and name
	pub data: Vec<(String, Json)>,
}

impl Event {
	pub fn json(&self) -> Json {
		let mut out = Json::dict();
		out.set("t", self.t).set("name", self.name.as_str());
		for (k, v) in &self.data {
			out.set(k, v.clone());
		}
		out
	}
}

/// A named clip. Its events and any other extras go in the animation's glTF extras.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
	pub name: String,
	pub length: f64,
	pub tracks: Vec<Track>,
	pub events: Vec<Event>,
	/// more extras (loop, speed, fps...): written after the events
	pub extras: Vec<(String, Json)>,
}

impl Clip {
	pub fn new(name: &str, length: f64) -> Clip {
		Clip { name: name.to_string(), length, tracks: Vec::new(), events: Vec::new(), extras: Vec::new() }
	}

	/// The animation's extras: {"events": [...], ...}, or None when there is nothing to say.
	pub fn extras_json(&self) -> Option<Json> {
		if self.events.is_empty() && self.extras.is_empty() {
			return None;
		}
		let mut out = Json::dict();
		if !self.events.is_empty() {
			out.set("events", Json::List(self.events.iter().map(Event::json).collect()));
		}
		for (k, v) in &self.extras {
			out.set(k, v.clone());
		}
		Some(out)
	}
}

// ------------------------------------------------------------------ quaternions
pub fn quat_normalized(q: Quat) -> Quat {
	let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
	if n > 1e-12 {
		[q[0] / n, q[1] / n, q[2] / n, q[3] / n]
	} else {
		QUAT_IDENTITY
	}
}

/// a then b applied to a vector is quat_mul(b, a); quat_mul(a, b) is a's frame turned by b.
pub fn quat_mul(a: Quat, b: Quat) -> Quat {
	let [ax, ay, az, aw] = a;
	let [bx, by, bz, bw] = b;
	[
		aw * bx + ax * bw + ay * bz - az * by,
		aw * by - ax * bz + ay * bw + az * bx,
		aw * bz + ax * by - ay * bx + az * bw,
		aw * bw - ax * bx - ay * by - az * bz,
	]
}

pub fn quat_conj(q: Quat) -> Quat {
	[-q[0], -q[1], -q[2], q[3]]
}

/// A turn of angle radians about an axis.
pub fn quat_axis_angle(axis: V3, angle: f64) -> Quat {
	let n = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
	if n < 1e-12 {
		return QUAT_IDENTITY;
	}
	let (s, c) = (angle * 0.5).sin_cos();
	[axis[0] / n * s, axis[1] / n * s, axis[2] / n * s, c]
}

/// Euler XYZ in radians, as PartScript's turn= (Blender's "XYZ": X first, then Y, then Z, about fixed axes).
pub fn quat_euler(angles: V3) -> Quat {
	let qx = quat_axis_angle([1.0, 0.0, 0.0], angles[0]);
	let qy = quat_axis_angle([0.0, 1.0, 0.0], angles[1]);
	let qz = quat_axis_angle([0.0, 0.0, 1.0], angles[2]);
	quat_mul(qz, quat_mul(qy, qx))
}

pub fn quat_rotate(q: Quat, v: V3) -> V3 {
	let p = quat_mul(quat_mul(q, [v[0], v[1], v[2], 0.0]), quat_conj(q));
	[p[0], p[1], p[2]]
}

pub fn quat_matrix(q: Quat) -> M3 {
	let [x, y, z, w] = quat_normalized(q);
	M3([
		[1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
		[2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
		[2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
	])
}

/// The rotation of an orthonormal matrix.
pub fn quat_from_matrix(m: &M3) -> Quat {
	let r = &m.0;
	let trace = r[0][0] + r[1][1] + r[2][2];
	let q = if trace > 0.0 {
		let s = (trace + 1.0).sqrt() * 2.0;
		[(r[2][1] - r[1][2]) / s, (r[0][2] - r[2][0]) / s, (r[1][0] - r[0][1]) / s, 0.25 * s]
	} else if r[0][0] > r[1][1] && r[0][0] > r[2][2] {
		let s = (1.0 + r[0][0] - r[1][1] - r[2][2]).sqrt() * 2.0;
		[0.25 * s, (r[0][1] + r[1][0]) / s, (r[0][2] + r[2][0]) / s, (r[2][1] - r[1][2]) / s]
	} else if r[1][1] > r[2][2] {
		let s = (1.0 + r[1][1] - r[0][0] - r[2][2]).sqrt() * 2.0;
		[(r[0][1] + r[1][0]) / s, 0.25 * s, (r[1][2] + r[2][1]) / s, (r[0][2] - r[2][0]) / s]
	} else {
		let s = (1.0 + r[2][2] - r[0][0] - r[1][1]).sqrt() * 2.0;
		[(r[0][2] + r[2][0]) / s, (r[1][2] + r[2][1]) / s, 0.25 * s, (r[1][0] - r[0][1]) / s]
	};
	quat_normalized(q)
}

/// The shorter way from a to b, u of the way.
pub fn quat_slerp(a: Quat, b: Quat, u: f64) -> Quat {
	let mut d = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
	let b = if d < 0.0 {
		d = -d;
		[-b[0], -b[1], -b[2], -b[3]]
	} else {
		b
	};
	if d > 0.9995 {
		return quat_normalized([a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u, a[2] + (b[2] - a[2]) * u, a[3] + (b[3] - a[3]) * u]);
	}
	let theta = d.acos();
	let s = theta.sin();
	let (wa, wb) = (((1.0 - u) * theta).sin() / s, (u * theta).sin() / s);
	[a[0] * wa + b[0] * wb, a[1] * wa + b[1] * wb, a[2] * wa + b[2] * wb, a[3] * wa + b[3] * wb]
}

/// The angle in radians between two rotations.
pub fn quat_angle(a: Quat, b: Quat) -> f64 {
	let d = (a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]).abs().min(1.0);
	2.0 * d.acos()
}

/// translation, rotation, scale as one matrix (scale first, then the turn, then the move).
pub fn trs_matrix(t: V3, r: Quat, s: V3) -> M4 {
	M4::translation(t).mul(&quat_matrix(r).to_4x4()).mul(&M4::diagonal([s[0], s[1], s[2], 1.0]))
}

// ------------------------------------------------------------------ authoring axes to glTF's
/// A position or direction: (x, y, z) Z up becomes (x, z, -y) Y up.
pub fn gltf_vec(v: V3) -> V3 {
	[v[0], v[2], -v[1]]
}

/// The same turn in glTF's axes (the axis change is itself a rotation, so the axis maps like a vector).
pub fn gltf_quat(q: Quat) -> Quat {
	[q[0], q[2], -q[1], q[3]]
}

/// A scale along the authoring axes, along glTF's.
pub fn gltf_scale(s: V3) -> V3 {
	[s[0], s[2], s[1]]
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::maths::euler;

	fn close(a: &[f64], b: &[f64]) -> bool {
		a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
	}

	#[test]
	fn euler_matches_the_matrix_partscript_uses() {
		let angles = [0.3, -0.7, 1.9];
		let m = euler(angles);
		let q = quat_matrix(quat_euler(angles));
		for r in 0..3 {
			assert!(close(&m.0[r], &q.0[r]), "{:?} vs {:?}", m.0, q.0);
		}
	}

	#[test]
	fn matrix_round_trip() {
		let q = quat_normalized([0.2, -0.4, 0.1, 0.8]);
		let back = quat_from_matrix(&quat_matrix(q));
		let back = if back[3] * q[3] < 0.0 { [-back[0], -back[1], -back[2], -back[3]] } else { back };
		assert!(close(&q, &back));
	}

	#[test]
	fn rotate_and_axes() {
		let q = quat_axis_angle([0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_2);
		assert!(close(&quat_rotate(q, [1.0, 0.0, 0.0]), &[0.0, 1.0, 0.0]));
		// the same turn in glTF's axes moves the converted vector to the converted result
		let v = [0.3, 0.5, -0.2];
		let turned = gltf_vec(quat_rotate(q, v));
		assert!(close(&quat_rotate(gltf_quat(q), gltf_vec(v)), &turned));
	}

	#[test]
	fn slerp_ends() {
		let a = quat_euler([0.1, 0.2, 0.3]);
		let b = quat_euler([1.0, -0.5, 0.25]);
		assert!(close(&quat_slerp(a, b, 0.0), &a));
		assert!(close(&quat_slerp(a, b, 1.0), &b));
		assert!((quat_angle(a, a)).abs() < 1e-6);
	}
}
