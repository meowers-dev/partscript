//! Values a line's variables hold, the variables themselves, and named shapes (Bounds).

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use kitlib::geom::Face;
use kitlib::maths::{M4, V3};
use kitlib::py;

/// One variable's value: a number, a whole number, words, a named shape or a space (the frame).
#[derive(Clone)]
pub enum Value {
	Num(f64),
	Int(i64),
	Str(Rc<str>),
	Bounds(Rc<Bounds>),
	Frame(Rc<M4>),
	/// a complex number (an argument worked out from a negative number to a fractional power)
	Complex(f64, f64),
}

impl Value {
	pub fn str(s: &str) -> Value {
		Value::Str(Rc::from(s))
	}

	pub fn as_number(&self) -> Option<f64> {
		match self {
			Value::Num(v) => Some(*v),
			Value::Int(v) => Some(*v as f64),
			_ => None,
		}
	}

	pub fn as_str(&self) -> Option<&str> {
		match self {
			Value::Str(s) => Some(s),
			_ => None,
		}
	}

	pub fn as_bounds(&self) -> Option<&Rc<Bounds>> {
		match self {
			Value::Bounds(b) => Some(b),
			_ => None,
		}
	}

	/// str(value) as Python prints it.
	pub fn text(&self) -> String {
		match self {
			Value::Num(v) => py::repr(*v),
			Value::Int(v) => v.to_string(),
			Value::Str(s) => s.to_string(),
			Value::Bounds(b) => format!("<Bounds {}>", b.name),
			Value::Frame(_) => "<Matrix>".into(),
			Value::Complex(re, im) => crate::pyexpr::complex_repr(*re, *im),
		}
	}

	/// repr(value) as Python prints it (strings quoted).
	pub fn repr(&self) -> String {
		match self {
			Value::Str(s) => py::repr_str(s),
			other => other.text(),
		}
	}
}

impl fmt::Debug for Value {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.repr())
	}
}

impl From<f64> for Value {
	fn from(v: f64) -> Value {
		Value::Num(v)
	}
}

impl From<&str> for Value {
	fn from(v: &str) -> Value {
		Value::str(v)
	}
}

/// A line's variables.
pub type Env = HashMap<String, Value>;

pub fn env_str(env: &Env, key: &str) -> Option<Rc<str>> {
	match env.get(key) {
		Some(Value::Str(s)) => Some(s.clone()),
		_ => None,
	}
}

/// The frame named shapes are read in (None: prop space).
pub fn env_frame(env: &Env) -> Option<Rc<M4>> {
	match env.get("__frame__") {
		Some(Value::Frame(m)) => Some(m.clone()),
		_ => None,
	}
}

/// What a named line made (desk = box ...): the box its shapes fill, kept in prop space and read in the
/// space of the line that asks (its group, its def), so desk.top means the same surface anywhere.
pub struct Bounds {
	pub name: String,
	pub lo: Option<V3>,
	pub hi: Option<V3>,
	/// the faces it made (scatter ... on NAME spreads copies over them)
	pub faces: Vec<Face>,
	/// here: read in prop space wherever it is asked
	pub world: bool,
	seen: RefCell<Option<(M4, (V3, V3))>>,
}

pub const BOUNDS_NUMBERS: [&str; 12] = ["left", "right", "front", "back", "bottom", "top", "x", "y", "z", "w", "d", "h"];

impl Bounds {
	pub fn new(name: &str, lo: Option<V3>, hi: Option<V3>, faces: Vec<Face>, world: bool) -> Bounds {
		Bounds { name: name.to_string(), lo, hi, faces, world, seen: RefCell::new(None) }
	}

	/// (lo, hi) in the space frame places things in (None: prop space).
	pub fn local(&self, frame: Option<&M4>) -> Result<(V3, V3), String> {
		let (Some(lo), Some(hi)) = (self.lo, self.hi) else {
			return Err(format!("{} made no shapes (when= or a none material left them all out)", self.name));
		};
		let Some(frame) = frame.filter(|_| !self.world) else { return Ok((lo, hi)) };
		if let Some((seen, out)) = &*self.seen.borrow() {
			if seen == frame {
				return Ok(*out);
			}
		}
		let inverse = frame.inverted().ok_or("matrix has no inverse")?;
		let mut corners = Vec::with_capacity(8);
		for x in [lo[0], hi[0]] {
			for y in [lo[1], hi[1]] {
				for z in [lo[2], hi[2]] {
					corners.push(inverse.point([x, y, z]));
				}
			}
		}
		let mut a = [f64::INFINITY; 3];
		let mut b = [f64::NEG_INFINITY; 3];
		for c in &corners {
			for k in 0..3 {
				if c[k] < a[k] {
					a[k] = c[k];
				}
				if c[k] > b[k] {
					b[k] = c[k];
				}
			}
		}
		*self.seen.borrow_mut() = Some((*frame, (a, b)));
		Ok((a, b))
	}

	pub fn number(&self, word: &str, frame: Option<&M4>) -> Result<f64, String> {
		let (lo, hi) = self.local(frame)?;
		let k = match word {
			"left" | "right" | "x" | "w" => 0,
			"front" | "back" | "y" | "d" => 1,
			"bottom" | "top" | "z" | "h" => 2,
			_ => {
				return Err(format!("{}.{}: a named shape gives {} (and points like {}.top_left)", self.name, word, BOUNDS_NUMBERS.join(", "), self.name));
			}
		};
		Ok(match word {
			"left" | "front" | "bottom" => lo[k],
			"right" | "back" | "top" => hi[k],
			"w" | "d" | "h" => hi[k] - lo[k],
			_ => (lo[k] + hi[k]) / 2.0,
		})
	}

	/// A point on it: desk (its centre), desk.top (the middle of its top), desk.top_left_back (a corner).
	pub fn point(&self, words: &str, frame: Option<&M4>) -> Result<V3, String> {
		let (lo, hi) = self.local(frame)?;
		let mut out = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
		for word in words.split('_').filter(|w| !w.is_empty()) {
			let k = match word {
				"left" | "right" => 0,
				"front" | "back" => 1,
				"bottom" | "top" => 2,
				"centre" | "center" => continue,
				_ => {
					return Err(format!(
						"{}.{}: a point is its centre, a side (top, left...) or sides joined (top_left, bottom_front_right)",
						self.name, words
					))
				}
			};
			out[k] = if matches!(word, "left" | "front" | "bottom") { lo[k] } else { hi[k] };
		}
		Ok(out)
	}
}
