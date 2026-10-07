//! Vectors, 3x3/4x4 matrices and XYZ eulers (64-bit floats), with Blender's mathutils conventions:
//! column vectors, `matrix * point`, right-handed rotations, Euler "XYZ" = Rz Ry Rx.
//!
//! The arithmetic follows the Python original operation for operation (including where it summed with
//! Python's compensated sum()), so results agree to the last bit.

use crate::py::PyMath;
use crate::py;

pub type V3 = [f64; 3];

pub fn add(a: V3, b: V3) -> V3 {
	[a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: V3, b: V3) -> V3 {
	[a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn scale(a: V3, k: f64) -> V3 {
	[a[0] * k, a[1] * k, a[2] * k]
}

pub fn neg(a: V3) -> V3 {
	[-a[0], -a[1], -a[2]]
}

/// Vector.dot (Python's sum() of the products).
pub fn dot(a: V3, b: V3) -> f64 {
	py::sum([a[0] * b[0], a[1] * b[1], a[2] * b[2]])
}

pub fn cross(a: V3, b: V3) -> V3 {
	[a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn length_squared(a: V3) -> f64 {
	py::sum([a[0] * a[0], a[1] * a[1], a[2] * a[2]])
}

pub fn length(a: V3) -> f64 {
	length_squared(a).sqrt()
}

/// Vector.normalized(): a zero vector stays zero.
pub fn normalized(a: V3) -> V3 {
	let squared = length_squared(a);
	if squared > 1.0e-35 {
		let inverse = 1.0 / squared.sqrt();
		scale(a, inverse)
	} else {
		[0.0; 3]
	}
}

/// Vector / scalar (multiplies by the inverse, as mathutils does).
pub fn div(a: V3, k: f64) -> V3 {
	scale(a, 1.0 / k)
}

/// A 4x4 matrix, rows of floats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct M4(pub [[f64; 4]; 4]);

/// A 3x3 matrix, rows of floats.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct M3(pub [[f64; 3]; 3]);

impl M4 {
	pub const IDENTITY: M4 = M4([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);

	pub fn identity() -> M4 {
		Self::IDENTITY
	}

	pub fn translation(v: V3) -> M4 {
		M4([[1.0, 0.0, 0.0, v[0]], [0.0, 1.0, 0.0, v[1]], [0.0, 0.0, 1.0, v[2]], [0.0, 0.0, 0.0, 1.0]])
	}

	pub fn diagonal(v: [f64; 4]) -> M4 {
		M4([[v[0], 0.0, 0.0, 0.0], [0.0, v[1], 0.0, 0.0], [0.0, 0.0, v[2], 0.0], [0.0, 0.0, 0.0, v[3]]])
	}

	/// Matrix.Rotation(angle, 4, "X"|"Y"|"Z")
	pub fn rotation_named(angle: f64, axis: char) -> M4 {
		M3::rotation_named(angle, axis).to_4x4()
	}

	/// Matrix.Rotation(angle, 4, axis vector)
	pub fn rotation_axis(angle: f64, axis: V3) -> M4 {
		M3::rotation_axis(angle, axis).to_4x4()
	}

	pub fn mul(&self, o: &M4) -> M4 {
		let b = &o.0;
		let mut out = [[0.0; 4]; 4];
		for (row, a) in out.iter_mut().zip(self.0.iter()) {
			let [a0, a1, a2, a3] = *a;
			*row = [
				a0 * b[0][0] + a1 * b[1][0] + a2 * b[2][0] + a3 * b[3][0],
				a0 * b[0][1] + a1 * b[1][1] + a2 * b[2][1] + a3 * b[3][1],
				a0 * b[0][2] + a1 * b[1][2] + a2 * b[2][2] + a3 * b[3][2],
				a0 * b[0][3] + a1 * b[1][3] + a2 * b[2][3] + a3 * b[3][3],
			];
		}
		M4(out)
	}

	/// A point through the matrix: (x, y, z, 1).
	pub fn point(&self, p: V3) -> V3 {
		let m = &self.0;
		let [x, y, z] = p;
		[
			m[0][0] * x + m[0][1] * y + m[0][2] * z + m[0][3],
			m[1][0] * x + m[1][1] * y + m[1][2] * z + m[1][3],
			m[2][0] * x + m[2][1] * y + m[2][2] * z + m[2][3],
		]
	}

	pub fn to_3x3(&self) -> M3 {
		let m = &self.0;
		M3([[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]])
	}

	pub fn to_translation(&self) -> V3 {
		[self.0[0][3], self.0[1][3], self.0[2][3]]
	}

	pub fn get(&self, r: usize, c: usize) -> f64 {
		self.0[r][c]
	}

	/// The inverse (Gauss-Jordan with partial pivoting), None when there is none.
	pub fn inverted(&self) -> Option<M4> {
		let size = 4;
		let mut rows: Vec<Vec<f64>> = (0..size)
			.map(|r| {
				let mut row = self.0[r].to_vec();
				row.extend((0..size).map(|c| if r == c { 1.0 } else { 0.0 }));
				row
			})
			.collect();
		for col in 0..size {
			let mut pivot = col;
			for r in col..size {
				if rows[r][col].abs() > rows[pivot][col].abs() {
					pivot = r;
				}
			}
			if rows[pivot][col].abs() < 1e-12 {
				return None;
			}
			rows.swap(col, pivot);
			let scale = rows[col][col];
			rows[col] = rows[col].iter().map(|v| v / scale).collect();
			for r in 0..size {
				if r != col && rows[r][col] != 0.0 {
					let factor = rows[r][col];
					let pivot_row = rows[col].clone();
					rows[r] = rows[r].iter().zip(&pivot_row).map(|(a, b)| a - factor * b).collect();
				}
			}
		}
		let mut out = [[0.0; 4]; 4];
		for r in 0..size {
			for c in 0..size {
				out[r][c] = rows[r][size + c];
			}
		}
		Some(M4(out))
	}

	pub fn determinant(&self) -> f64 {
		let m = &self.0;
		if m[3] == [0.0, 0.0, 0.0, 1.0] {
			return self.to_3x3().determinant();
		}
		determinant(&m.iter().map(|r| r.to_vec()).collect::<Vec<_>>())
	}

	pub fn rows(&self) -> [[f64; 4]; 4] {
		self.0
	}
}

fn determinant(rows: &[Vec<f64>]) -> f64 {
	if rows.len() == 1 {
		return rows[0][0];
	}
	if rows.len() == 2 {
		return rows[0][0] * rows[1][1] - rows[0][1] * rows[1][0];
	}
	let mut total = 0.0;
	for (column, &value) in rows[0].iter().enumerate() {
		if value == 0.0 {
			continue;
		}
		let minor: Vec<Vec<f64>> = rows[1..].iter().map(|r| r.iter().enumerate().filter(|(c, _)| *c != column).map(|(_, v)| *v).collect()).collect();
		total += (if column % 2 == 1 { -1.0 } else { 1.0 }) * value * determinant(&minor);
	}
	total
}

impl M3 {
	pub fn identity() -> M3 {
		M3([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]])
	}

	pub fn rotation_named(angle: f64, axis: char) -> M3 {
		let (c, s) = (angle.py_cos(), angle.py_sin());
		match axis.to_ascii_uppercase() {
			'X' => M3([[1.0, 0.0, 0.0], [0.0, c, -s], [0.0, s, c]]),
			'Y' => M3([[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]]),
			_ => M3([[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]),
		}
	}

	pub fn rotation_axis(angle: f64, axis: V3) -> M3 {
		let (c, s) = (angle.py_cos(), angle.py_sin());
		let [x, y, z] = normalized(axis);
		let ico = 1.0 - c;
		let (sx, sy, sz) = (x * s, y * s, z * s);
		let (n00, n01, n11) = (x * x * ico, x * y * ico, y * y * ico);
		let (n02, n12, n22) = (x * z * ico, y * z * ico, z * z * ico);
		M3([[n00 + c, n01 - sz, n02 + sy], [n01 + sz, n11 + c, n12 - sx], [n02 - sy, n12 + sx, n22 + c]])
	}

	pub fn to_4x4(&self) -> M4 {
		let m = &self.0;
		M4([[m[0][0], m[0][1], m[0][2], 0.0], [m[1][0], m[1][1], m[1][2], 0.0], [m[2][0], m[2][1], m[2][2], 0.0], [0.0, 0.0, 0.0, 1.0]])
	}

	/// Matrix @ Matrix for 3x3 (each entry Python's sum() of a row times a column).
	pub fn mul(&self, o: &M3) -> M3 {
		let mut out = [[0.0; 3]; 3];
		for r in 0..3 {
			for c in 0..3 {
				out[r][c] = py::sum((0..3).map(|k| self.0[r][k] * o.0[k][c]));
			}
		}
		M3(out)
	}

	/// Matrix @ Vector for 3x3 (each row's sum()).
	pub fn apply(&self, v: V3) -> V3 {
		let m = &self.0;
		[
			py::sum([m[0][0] * v[0], m[0][1] * v[1], m[0][2] * v[2]]),
			py::sum([m[1][0] * v[0], m[1][1] * v[1], m[1][2] * v[2]]),
			py::sum([m[2][0] * v[0], m[2][1] * v[1], m[2][2] * v[2]]),
		]
	}

	pub fn determinant(&self) -> f64 {
		let [[a, b, c], [d, e, f], [g, h, i]] = self.0;
		a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
	}
}

/// Euler((x, y, z), "XYZ").to_matrix(): radians.
pub fn euler(angles: V3) -> M3 {
	let (ci, cj, ch) = (angles[0].py_cos(), angles[1].py_cos(), angles[2].py_cos());
	let (si, sj, sh) = (angles[0].py_sin(), angles[1].py_sin(), angles[2].py_sin());
	let (cc, cs, sc, ss) = (ci * ch, ci * sh, si * ch, si * sh);
	M3([[cj * ch, sj * sc - cs, sj * cc + ss], [cj * sh, sj * ss + cc, sj * cs - sc], [-sj, cj * si, cj * ci]])
}

/// The nearest 32-bit float (what a glTF float accessor stores).
pub fn f32_round(v: f64) -> f64 {
	v as f32 as f64
}
