//! Paths: placing things along a line of points, and markers that show where pieces snap together.

use crate::geom::{Materials, Part};
use crate::maths::V3;
use crate::py;

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
	pub pos: V3,
	/// degrees about Z; 0 = the piece's +X runs along +X
	pub yaw: f64,
	/// along the piece's X (fit)
	pub stretch: f64,
	/// which straight it is on (or which corner)
	pub segment: usize,
}

/// Frames along a polyline. corners: one at each point, turned halfway between the straights that meet
/// there. every: one each `every` metres, centred in its slot; with fit, each straight gets a whole
/// number of slots and the pieces stretch to fill them. joints (with every): one at each end of those
/// slots instead, where the pieces meet (the posts).
pub fn path_frames(points: &[Vec<f64>], every: f64, fit: bool, corners: bool, closed: bool, joints: bool) -> Result<Vec<Frame>, String> {
	let mut pts: Vec<V3> = points
		.iter()
		.map(|p| [p.first().copied().unwrap_or(0.0), p.get(1).copied().unwrap_or(0.0), p.get(2).copied().unwrap_or(0.0)])
		.collect();
	if closed && pts.len() > 2 && pts[0] != pts[pts.len() - 1] {
		pts.push(pts[0]);
	}
	if pts.len() < 2 {
		return Err("a path needs two or more points".into());
	}
	let segments: Vec<(V3, V3)> = pts.windows(2).map(|w| (w[0], w[1])).collect();
	let headings: Vec<f64> = segments.iter().map(|(a, b)| (b[1] - a[1]).atan2(b[0] - a[0]).to_degrees()).collect();
	let lengths: Vec<f64> = segments.iter().map(|(a, b)| py::dist(a, b)).collect();
	let lerp = |a: V3, b: V3, t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
	if corners {
		let mut out = Vec::new();
		let count = pts.len() - usize::from(closed);
		for k in 0..count {
			let before = if k > 0 { Some(headings[k - 1]) } else if closed { Some(headings[headings.len() - 1]) } else { None };
			let after = headings.get(k).copied();
			let yaw = match (before, after) {
				(Some(b), Some(a)) => b + (py::modulo(a - b + 180.0, 360.0).unwrap() - 180.0) / 2.0,
				(None, Some(a)) => a,
				(Some(b), None) => b,
				(None, None) => 0.0,
			};
			out.push(Frame { pos: pts[k], yaw, stretch: 1.0, segment: k });
		}
		return Ok(out);
	}
	if every <= 0.0 {
		return Err("along a path: every=D (a piece each D metres) or corners=1 (one at each point)".into());
	}
	let mut out = Vec::new();
	if joints {
		for (k, (((a, b), &length), &yaw)) in segments.iter().zip(&lengths).zip(&headings).enumerate() {
			let count = if fit { pieces(py::round(length / every))?.max(1) } else { pieces(py::floordiv(length, every).unwrap())?.max(1) };
			let step = if fit { length / count as f64 } else { every };
			let last = if k == segments.len() - 1 && !closed { count } else { count - 1 };
			for n in 0..=last {
				let t = if length != 0.0 { n as f64 * step / length } else { 0.0 };
				out.push(Frame { pos: lerp(*a, *b, t), yaw, stretch: 1.0, segment: k });
			}
		}
		return Ok(out);
	}
	if fit {
		for (k, (((a, b), &length), &yaw)) in segments.iter().zip(&lengths).zip(&headings).enumerate() {
			let count = pieces(py::round(length / every))?.max(1);
			let step = length / count as f64;
			for n in 0..count {
				let t = if length != 0.0 { (n as f64 + 0.5) * step / length } else { 0.0 };
				out.push(Frame { pos: lerp(*a, *b, t), yaw, stretch: step / every, segment: k });
			}
		}
		return Ok(out);
	}
	let total = py::sum(lengths.iter().copied());
	if total / every > MAX_PIECES {
		return Err(too_many());
	}
	let mut distance = every / 2.0;
	while distance <= total + 1e-9 {
		let mut walked = distance;
		for (k, (((a, b), &length), &yaw)) in segments.iter().zip(&lengths).zip(&headings).enumerate() {
			if walked <= length + 1e-9 || k == segments.len() - 1 {
				let t = if length != 0.0 { py::min2(1.0, walked / length) } else { 0.0 };
				out.push(Frame { pos: lerp(*a, *b, t), yaw, stretch: 1.0, segment: k });
				break;
			}
			walked -= length;
		}
		distance += every;
	}
	Ok(out)
}

/// The most pieces a path is cut into: past this the build would not finish (an error instead).
const MAX_PIECES: f64 = 100_000.0;

fn too_many() -> String {
	format!("along a path: more than {MAX_PIECES} pieces (every= is too small for the path)")
}

/// int() of a piece count, as Python took it (NaN an error), refused past MAX_PIECES.
fn pieces(n: f64) -> Result<i64, String> {
	if n.is_nan() {
		return Err("cannot convert float NaN to integer".into());
	}
	if n > MAX_PIECES {
		return Err(too_many());
	}
	Ok(n as i64)
}

const KIND_COLOURS: [&str; 6] = ["ffd84a", "4ad8ff", "ff6a4a", "8aff6a", "d86aff", "ffffff"];

/// A colour for each snap kind, so matching kinds read as matching: #rrggbb without the #.
pub fn kind_colour(kind: &str) -> &'static str {
	if kind == "any" || kind.is_empty() {
		return "f0f0f0";
	}
	let total: u32 = kind.chars().map(|c| c as u32).sum();
	KIND_COLOURS[(total % (KIND_COLOURS.len() as u32 - 1)) as usize]
}

/// A snap point to mark: where, which way, its kind.
pub struct Marker {
	pub pos: V3,
	pub dir: V3,
	pub kind: String,
}

/// A part with a marker per snap: a small cube at the point and a pointer along its direction.
pub fn snap_markers(snaps: &[Marker], material_of: &mut dyn FnMut(&str) -> String, size: f64) -> Part {
	let mut part = Part::new("snaps", Materials::default());
	for snap in snaps {
		let mat = material_of(&snap.kind);
		let [x, y, z] = snap.pos;
		let [dx, dy, dz] = snap.dir;
		part.simple_box([x, y, z], [size, size, size], &mat);
		let length = size * 4.0;
		let yaw = dy.atan2(dx).to_degrees();
		let pitch = dz.atan2(py::hypot2(dx, dy)).to_degrees();
		part.push_at([x + dx * length / 2.0, y + dy * length / 2.0, z + dz * length / 2.0], [0.0, (-pitch).to_radians(), yaw.to_radians()]);
		part.simple_box([0.0; 3], [length, size * 0.35, size * 0.35], &mat);
		part.box_([length / 2.0, 0.0, 0.0], [size * 0.9, size * 0.9, size * 0.9], &mat, [0.0, 90f64.to_radians(), 0.0], &[], 1.0, [0.0, 0.0], [0.0, 0.0]);
		part.pop();
	}
	part
}

/// A curve through the points (Catmull-Rom): steps pieces between each pair, through every point.
pub fn smooth_points(points: &[V3], steps: usize, closed: bool) -> Vec<V3> {
	let pts = points.to_vec();
	if steps < 2 || pts.len() < 3 {
		return pts;
	}
	let count = pts.len();
	let segments = if closed { count } else { count - 1 };
	let mut out = Vec::new();
	let mirror = |a: V3, b: V3| [2.0 * a[0] - b[0], 2.0 * a[1] - b[1], 2.0 * a[2] - b[2]];
	for k in 0..segments {
		let p0 = if closed || k > 0 { pts[(k + count - 1) % count] } else { mirror(pts[0], pts[1]) };
		let (p1, p2) = (pts[k], pts[(k + 1) % count]);
		let p3 = if closed || k + 2 < count { pts[(k + 2) % count] } else { mirror(pts[count - 1], pts[count - 2]) };
		for n in 0..steps {
			let t = n as f64 / steps as f64;
			let (t2, t3) = (t * t, t * t * t);
			out.push([0, 1, 2].map(|j| {
				let (a, b, c, d) = (p0[j], p1[j], p2[j], p3[j]);
				0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3)
			}));
		}
	}
	if !closed {
		out.push(pts[count - 1]);
	}
	out
}
