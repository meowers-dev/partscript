//! Expressions: the arithmetic PartScript accepts anywhere a number goes (Python's expression grammar,
//! numbers only), and the per-copy draws: rand(), pick(), odds(), noise() and rough().
//!
//! Every draw is seeded by the copy it runs for (its __seed__), the expression's text and where in the
//! text the call stands, so the same file always draws the same numbers.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use kitlib::noise::{try_noise, try_rough};
use kitlib::py::{self, PyRandom};

use crate::pyexpr::{self, Const, Node};

use crate::value::{env_frame, Env, Value};

// ------------------------------------------------------------------ the syntax tree

thread_local! {
	static PARSED: RefCell<HashMap<String, Rc<Option<Node>>>> = RefCell::new(HashMap::new());
}

/// ast.parse(text, mode="eval"), remembered (the same few texts are evaluated over and over).
fn parse(text: &str) -> Rc<Option<Node>> {
	if let Some(found) = PARSED.with(|cache| cache.borrow().get(text).cloned()) {
		return found;
	}
	let parsed = Rc::new(pyexpr::parse(text));
	PARSED.with(|cache| {
		let mut cache = cache.borrow_mut();
		if cache.len() > 20000 {
			cache.clear();
		}
		cache.insert(text.to_string(), parsed.clone());
	});
	parsed
}

// ------------------------------------------------------------------ numbers

/// What an expression works out to on the way: a float, or (from a negative number to a fractional
/// power) a complex number, which Python carries on with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Num {
	F(f64),
	C(f64, f64),
}

impl Num {
	fn parts(self) -> (f64, f64) {
		match self {
			Num::F(x) => (x, 0.0),
			Num::C(re, im) => (re, im),
		}
	}

	fn type_name(self) -> &'static str {
		match self {
			Num::F(_) => "float",
			Num::C(..) => "complex",
		}
	}

	pub fn truthy(self) -> bool {
		match self {
			Num::F(x) => x != 0.0,
			Num::C(re, im) => re != 0.0 || im != 0.0,
		}
	}

	/// The value as Python's repr() writes it.
	pub fn repr(self) -> String {
		match self {
			Num::F(x) => py::repr(x),
			Num::C(re, im) => pyexpr::complex_repr(re, im),
		}
	}
}

enum Fail {
	/// a name nobody defined (reported with the whole expression)
	Unknown(String),
	/// ZeroDivisionError, OverflowError, TypeError: reported as 'text': message
	Arith(String),
	/// a ValueError, reported as it is
	Value(String),
}

type R<T> = Result<T, Fail>;

fn real(v: Num) -> R<f64> {
	match v {
		Num::F(x) => Ok(x),
		Num::C(..) => Err(Fail::Arith("must be real number, not complex".into())),
	}
}

fn unsupported_operands(op: &str, a: Num, b: Num) -> Fail {
	Fail::Arith(format!("unsupported operand type(s) for {op}: '{}' and '{}'", a.type_name(), b.type_name()))
}

/// Smith's division, as CPython's complex division does it (None: dividing by zero).
fn c_quot(a: (f64, f64), b: (f64, f64)) -> Option<(f64, f64)> {
	let (abs_re, abs_im) = (b.0.abs(), b.1.abs());
	if abs_re >= abs_im {
		if abs_re == 0.0 {
			return None;
		}
		let ratio = b.1 / b.0;
		let denom = b.0 + b.1 * ratio;
		Some(((a.0 + a.1 * ratio) / denom, (a.1 - a.0 * ratio) / denom))
	} else if abs_im >= abs_re {
		let ratio = b.0 / b.1;
		let denom = b.0 * ratio + b.1;
		Some(((a.0 * ratio + a.1) / denom, (a.1 * ratio - a.0) / denom))
	} else {
		Some((f64::NAN, f64::NAN))
	}
}

fn c_prod(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
	(a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0)
}

/// complex ** complex, as CPython's complex_pow works it out.
fn complex_pow(a: (f64, f64), b: (f64, f64)) -> R<Num> {
	let mut edom = false;
	let r = if b.1 == 0.0 && b.0 == b.0.floor() && b.0.abs() <= 100.0 {
		let n = b.0 as i64;
		let powu = |x: (f64, f64), n: i64| {
			let (mut r, mut p, mut mask) = ((1.0, 0.0), x, 1i64);
			while mask > 0 && n >= mask {
				if n & mask != 0 {
					r = c_prod(r, p);
				}
				mask <<= 1;
				p = c_prod(p, p);
			}
			r
		};
		if n > 0 {
			powu(a, n)
		} else {
			c_quot((1.0, 0.0), powu(a, -n)).unwrap_or_else(|| {
				edom = true;
				(0.0, 0.0)
			})
		}
	} else if b.0 == 0.0 && b.1 == 0.0 {
		(1.0, 0.0)
	} else if a.0 == 0.0 && a.1 == 0.0 {
		if b.1 != 0.0 || b.0 < 0.0 {
			edom = true;
		}
		(0.0, 0.0)
	} else {
		let vabs = a.0.hypot(a.1);
		let mut len = vabs.powf(b.0);
		let at = a.1.atan2(a.0);
		let mut phase = at * b.0;
		if b.1 != 0.0 {
			len *= (-at * b.1).exp();
			phase += b.1 * vabs.ln();
		}
		if phase.is_infinite() {
			edom = true; // cos() and sin() of an infinity set EDOM
		}
		(len * phase.cos(), len * phase.sin())
	};
	if r.0.is_infinite() || r.1.is_infinite() {
		if !edom {
			return Err(Fail::Arith("complex exponentiation".into()));
		}
	}
	if edom {
		return Err(Fail::Arith("0.0 to a negative or complex power".into()));
	}
	Ok(Num::C(r.0, r.1))
}

/// float ** float, as CPython's float_pow works it out.
fn float_pow(iv: f64, iw: f64) -> R<Num> {
	let odd = |x: f64| x.abs() % 2.0 == 1.0;
	if iw == 0.0 {
		return Ok(Num::F(1.0));
	}
	if iv.is_nan() {
		return Ok(Num::F(iv));
	}
	if iw.is_nan() {
		return Ok(Num::F(if iv == 1.0 { 1.0 } else { iw }));
	}
	if iw.is_infinite() {
		let v = iv.abs();
		return Ok(Num::F(if v == 1.0 {
			1.0
		} else if (iw > 0.0) == (v > 1.0) {
			iw.abs()
		} else {
			0.0
		}));
	}
	if iv.is_infinite() {
		return Ok(Num::F(if iw > 0.0 {
			if odd(iw) {
				iv
			} else {
				iv.abs()
			}
		} else if odd(iw) {
			0.0f64.copysign(iv)
		} else {
			0.0
		}));
	}
	if iv == 0.0 {
		if iw < 0.0 {
			return Err(Fail::Arith("0.0 cannot be raised to a negative power".into()));
		}
		return Ok(Num::F(if odd(iw) { iv } else { 0.0 }));
	}
	let mut v = iv;
	let mut negate = false;
	if iv < 0.0 {
		if iw != iw.floor() {
			return complex_pow((iv, 0.0), (iw, 0.0));
		}
		v = -iv;
		negate = odd(iw);
	}
	if v == 1.0 {
		return Ok(Num::F(if negate { -1.0 } else { 1.0 }));
	}
	let mut x = v.powf(iw);
	if negate {
		x = -x;
	}
	if x.is_infinite() {
		return Err(Fail::Arith("(34, 'Numerical result out of range')".into()));
	}
	Ok(Num::F(x))
}

fn binary(op: &str, a: Num, b: Num) -> R<Num> {
	if let (Num::F(x), Num::F(y)) = (a, b) {
		return Ok(Num::F(match op {
			"Add" => x + y,
			"Sub" => x - y,
			"Mult" => x * y,
			"Div" => {
				if y == 0.0 {
					return Err(Fail::Arith("float division by zero".into()));
				}
				x / y
			}
			"Mod" => py::modulo(x, y).ok_or_else(|| Fail::Arith("float modulo by zero".into()))?,
			"FloorDiv" => py::floordiv(x, y).ok_or_else(|| Fail::Arith("float floor division by zero".into()))?,
			_ => return float_pow(x, y),
		}));
	}
	let (p, q) = (a.parts(), b.parts());
	Ok(match op {
		"Add" => Num::C(p.0 + q.0, p.1 + q.1),
		"Sub" => Num::C(p.0 - q.0, p.1 - q.1),
		"Mult" => {
			let (re, im) = c_prod(p, q);
			Num::C(re, im)
		}
		"Div" => {
			let (re, im) = c_quot(p, q).ok_or_else(|| Fail::Arith("complex division by zero".into()))?;
			Num::C(re, im)
		}
		"Mod" => return Err(unsupported_operands("%", a, b)),
		"FloorDiv" => return Err(unsupported_operands("//", a, b)),
		_ => return complex_pow(p, q),
	})
}

fn compare(op: &str, a: Num, b: Num) -> R<bool> {
	if let (Num::F(x), Num::F(y)) = (a, b) {
		return Ok(match op {
			"Eq" => x == y,
			"NotEq" => x != y,
			"Lt" => x < y,
			"LtE" => x <= y,
			"Gt" => x > y,
			_ => x >= y,
		});
	}
	let (p, q) = (a.parts(), b.parts());
	match op {
		"Eq" => Ok(p.0 == q.0 && p.1 == q.1),
		"NotEq" => Ok(!(p.0 == q.0 && p.1 == q.1)),
		_ => {
			let symbol = match op {
				"Lt" => "<",
				"LtE" => "<=",
				"Gt" => ">",
				_ => ">=",
			};
			Err(not_supported(symbol, a.type_name(), b.type_name()))
		}
	}
}

fn not_supported(symbol: &str, a: &str, b: &str) -> Fail {
	Fail::Arith(format!("'{symbol}' not supported between instances of '{a}' and '{b}'"))
}

// ------------------------------------------------------------------ evaluation

/// A random stream for one rand()/pick()/odds() call: seeded by the copy it runs for, the text and
/// where in the text the call stands.
pub fn rng(env: &Env, text: &str, at: usize) -> PyRandom {
	let seed = match env.get("__seed__") {
		Some(Value::Str(s)) => s.to_string(),
		Some(other) => other.text(),
		None => String::new(),
	};
	PyRandom::from_str(&format!("{seed}#{text}#{at}"))
}

/// Python's float(text): Unicode digits and spaces as Python reads them, underscores between digits,
/// inf, infinity and nan in any case.
pub fn py_float(text: &str) -> Option<f64> {
	let t: String = py_strip(text).chars().map(|c| py::decimal(c).map(|d| (b'0' + d) as char).unwrap_or(c)).collect();
	if t.is_empty() || !t.is_ascii() {
		return None;
	}
	let (sign, body) = match t.as_bytes()[0] {
		b'-' => (-1.0, &t[1..]),
		b'+' => (1.0, &t[1..]),
		_ => (1.0, t.as_str()),
	};
	let lower = body.to_ascii_lowercase();
	if lower == "inf" || lower == "infinity" {
		return Some(sign * f64::INFINITY);
	}
	if lower == "nan" {
		return Some(f64::NAN.copysign(sign));
	}
	let b = body.as_bytes();
	if b.is_empty() {
		return None;
	}
	// digits, one point, an exponent; underscores only between digits
	let mut seen_digit = false;
	let mut clean = String::with_capacity(body.len());
	let mut i = 0;
	let mut point = false;
	let mut exponent = false;
	while i < b.len() {
		let c = b[i];
		match c {
			b'0'..=b'9' => {
				seen_digit = true;
				clean.push(c as char);
			}
			b'_' => {
				if i == 0 || i + 1 >= b.len() || !b[i - 1].is_ascii_digit() || !b[i + 1].is_ascii_digit() {
					return None;
				}
			}
			b'.' if !point && !exponent => {
				point = true;
				clean.push('.');
			}
			b'e' | b'E' if !exponent && seen_digit => {
				exponent = true;
				clean.push('e');
				if i + 1 < b.len() && (b[i + 1] == b'+' || b[i + 1] == b'-') {
					clean.push(b[i + 1] as char);
					i += 1;
				}
				if i + 1 >= b.len() || !b[i + 1].is_ascii_digit() {
					return None;
				}
			}
			_ => return None,
		}
		i += 1;
	}
	if !seen_digit {
		return None;
	}
	clean.parse::<f64>().ok().map(|v| sign * v)
}

/// Python's str.isspace for one character.
pub fn py_isspace(c: char) -> bool {
	py::is_space(c)
}

/// Python's str.strip().
pub fn py_strip(text: &str) -> &str {
	py::strip(text)
}

/// A number from an expression over env's numbers.
pub fn evaluate(text: &str, env: &Env) -> Result<f64, String> {
	match evaluate_number(text, env)? {
		Num::F(x) => Ok(x),
		Num::C(re, im) => Err(format!("{}: {} is a complex number, not a size or a position", py::repr_str(py_strip(text)), pyexpr::complex_repr(re, im))),
	}
}

/// An expression's value as Python works it out, complex numbers included.
pub fn evaluate_number(text: &str, env: &Env) -> Result<Num, String> {
	let text = py_strip(text);
	if text.is_empty() {
		return Err("empty number".into());
	}
	if let Some(v) = py_float(text) {
		return Ok(Num::F(v));
	}
	let resolved;
	let text = if text.contains("pick(") {
		resolved = resolve_picks(text, env);
		resolved.as_str()
	} else {
		text
	};
	let tree = parse(text);
	let Some(tree) = tree.as_ref() else {
		return Err(format!("not a number or expression: {}", py::repr_str(text)));
	};
	match walk(tree, text, env) {
		Ok(v) => Ok(v),
		Err(Fail::Unknown(name)) => Err(format!("unknown name {name} in {}", py::repr_str(text))),
		Err(Fail::Arith(message)) => Err(format!("{}: {message}", py::repr_str(text))),
		Err(Fail::Value(message)) => Err(message),
	}
}

thread_local! {
	/// How deep the walk is, through names whose text is itself an expression too. Python stops a
	/// little under 1000 frames (RecursionError); so does this, rather than run out of stack.
	static WALK_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

const MAX_WALK_DEPTH: usize = 950;

fn unsupported(node: &Node) -> Fail {
	match pyexpr::dump(node) {
		Ok(text) => {
			let cut: String = text.chars().take(40).collect();
			Fail::Value(format!("unsupported in an expression: {cut}"))
		}
		Err(message) => Fail::Value(message),
	}
}

fn walk(node: &Node, text: &str, env: &Env) -> R<Num> {
	let depth = WALK_DEPTH.with(|d| {
		d.set(d.get() + 1);
		d.get()
	});
	let out = if depth > MAX_WALK_DEPTH { Err(Fail::Value("maximum recursion depth exceeded".into())) } else { walk_node(node, text, env) };
	WALK_DEPTH.with(|d| d.set(d.get() - 1));
	out
}

fn walk_node(node: &Node, text: &str, env: &Env) -> R<Num> {
	match node {
		Node::Constant { value, .. } => match value {
			Const::Int(digits) => {
				let v: f64 = digits.parse().unwrap_or(f64::INFINITY);
				if v.is_infinite() {
					Err(Fail::Arith("int too large to convert to float".into()))
				} else {
					Ok(Num::F(v))
				}
			}
			Const::Float(x) => Ok(Num::F(*x)),
			Const::True => Ok(Num::F(1.0)),
			Const::False => Ok(Num::F(0.0)),
			_ => Err(unsupported(node)),
		},
		Node::BinOp { left, op, right } if matches!(*op, "Add" | "Sub" | "Mult" | "Div" | "Pow" | "Mod" | "FloorDiv") => {
			let (a, b) = (walk(left, text, env)?, walk(right, text, env)?);
			binary(op, a, b)
		}
		Node::UnaryOp { op: op @ ("USub" | "UAdd"), operand } => {
			let v = walk(operand, text, env)?;
			Ok(match (*op, v) {
				("USub", Num::F(x)) => Num::F(-x),
				("USub", Num::C(re, im)) => Num::C(-re, -im),
				(_, v) => v,
			})
		}
		Node::UnaryOp { op: "Not", operand } => Ok(Num::F(if walk(operand, text, env)?.truthy() { 0.0 } else { 1.0 })),
		Node::Compare { left, ops, comparators } if ops.iter().all(|o| matches!(*o, "Eq" | "NotEq" | "Lt" | "LtE" | "Gt" | "GtE")) => {
			let mut left = walk(left, text, env)?;
			for (op, right) in ops.iter().zip(comparators) {
				let right = walk(right, text, env)?;
				if !compare(op, left, right)? {
					return Ok(Num::F(0.0));
				}
				left = right;
			}
			Ok(Num::F(1.0))
		}
		Node::BoolOp { and, values } => {
			let values: Vec<Num> = values.iter().map(|v| walk(v, text, env)).collect::<R<_>>()?;
			let holds = if *and { values.iter().all(|v| v.truthy()) } else { values.iter().any(|v| v.truthy()) };
			Ok(Num::F(if holds { 1.0 } else { 0.0 }))
		}
		Node::IfExp { test, body, orelse } => {
			if walk(test, text, env)?.truthy() {
				walk(body, text, env)
			} else {
				walk(orelse, text, env)
			}
		}
		Node::Call { func, args, keywords, col } if keywords.is_empty() && matches!(&**func, Node::Name { id, .. } if id == "noise" || id == "rough") => {
			let Node::Name { id: name, .. } = &**func else { unreachable!() };
			let values: Vec<Num> = args.iter().map(|a| walk(a, text, env)).collect::<R<_>>()?;
			let _ = col;
			if values.is_empty() || values.len() > 3 {
				return Err(Fail::Value(format!("{name}(x[, y[, z]]): one to three numbers, a point in space")));
			}
			let values: Vec<f64> = values.into_iter().map(real).collect::<R<_>>()?;
			let seed = match env.get("__seed__") {
				Some(v) => v.text(),
				None => String::new(),
			};
			let seed = seed.split('|').next().unwrap_or("").to_string();
			let (x, y, z) = (values[0], values.get(1).copied().unwrap_or(0.0), values.get(2).copied().unwrap_or(0.0));
			let value = if name == "noise" { try_noise(x, y, z, &seed) } else { try_rough(x, y, z, &seed, 4) };
			value.map(Num::F).map_err(|e| if e.contains("NaN") { Fail::Value(e.into()) } else { Fail::Arith(e.into()) })
		}
		Node::Call { func, args, keywords, col } if keywords.is_empty() && matches!(&**func, Node::Name { id, .. } if id == "rand") => {
			let bounds: Vec<Num> = args.iter().map(|a| walk(a, text, env)).collect::<R<_>>()?;
			let (low, high) = match bounds.len() {
				0 => (Num::F(0.0), Num::F(1.0)),
				1 => (Num::F(0.0), bounds[0]),
				_ => (bounds[0], bounds[1]),
			};
			let r = rng(env, text, *col).random();
			// random.uniform: a + (b - a) * random()
			binary("Add", low, binary("Mult", binary("Sub", high, low)?, Num::F(r))?)
		}
		Node::Call { func, args, keywords, col } if keywords.is_empty() && matches!(&**func, Node::Name { id, .. } if id == "odds") => {
			let weights: Vec<Num> = args.iter().map(|a| walk(a, text, env)).collect::<R<_>>()?;
			if weights.is_empty() {
				return Err(Fail::Value("odds(w0, w1, ...): weights of 0 or more, at least one above 0".into()));
			}
			for w in &weights {
				match w {
					Num::F(x) if *x < 0.0 => return Err(Fail::Value("odds(w0, w1, ...): weights of 0 or more, at least one above 0".into())),
					Num::C(..) => return Err(not_supported("<", "complex", "int")),
					_ => {}
				}
			}
			let weights: Vec<f64> = weights.into_iter().map(|w| if let Num::F(x) = w { x } else { 0.0 }).collect();
			let total = py::sum(weights.iter().copied());
			if total <= 0.0 {
				return Err(Fail::Value("odds(w0, w1, ...): weights of 0 or more, at least one above 0".into()));
			}
			let mut roll = rng(env, text, *col).uniform(0.0, total);
			for (index, weight) in weights.iter().enumerate() {
				roll -= weight;
				if roll < 0.0 {
					return Ok(Num::F(index as f64));
				}
			}
			match weights.iter().rposition(|w| *w > 0.0) {
				Some(k) => Ok(Num::F(k as f64)),
				None => Err(Fail::Value("max() iterable argument is empty".into())),
			}
		}
		Node::Attribute { value, attr, .. } if matches!(&**value, Node::Name { .. }) => {
			let Node::Name { id, .. } = &**value else { unreachable!() };
			match env.get(id) {
				Some(Value::Bounds(b)) => b.number(attr, env_frame(env).as_deref()).map(Num::F).map_err(Fail::Value),
				None => Err(Fail::Unknown(id.clone())),
				Some(_) => Err(Fail::Value(format!("{id}.{attr}: {id} is not a named shape"))),
			}
		}
		Node::Name { id, .. } => {
			if let Some(value) = env.get(id) {
				return match value {
					Value::Bounds(_) => Err(Fail::Value(format!(
						"{id} is a named shape: say which number (left right front back bottom top x y z w d h), as in {id}.top"
					))),
					Value::Num(v) => Ok(Num::F(*v)),
					Value::Int(v) => Ok(Num::F(*v as f64)),
					other => evaluate_number(&other.text(), env).map_err(|_| Fail::Value(format!("{id} is {}, not a number", other.repr()))),
				};
			}
			match id.as_str() {
				"pi" => Ok(Num::F(std::f64::consts::PI)),
				"tau" => Ok(Num::F(std::f64::consts::TAU)),
				_ => Err(Fail::Unknown(id.clone())),
			}
		}
		Node::Call { func, args, keywords, .. } if keywords.is_empty() && matches!(&**func, Node::Name { id, .. } if FUNCS.contains(&id.as_str())) => {
			let Node::Name { id: name, .. } = &**func else { unreachable!() };
			let values: Vec<Num> = args.iter().map(|a| walk(a, text, env)).collect::<R<_>>()?;
			function(name, &values).map(Num::F)
		}
		other => Err(unsupported(other)),
	}
}

const FUNCS: [&str; 17] = ["sin", "cos", "tan", "sqrt", "abs", "min", "max", "floor", "ceil", "round", "atan2", "hypot", "rad", "deg", "atan", "asin", "acos"];

/// A lambda's arguments, or the TypeError Python gives for the wrong number of them.
fn lambda_args<'v>(values: &'v [Num], params: &[&str]) -> R<&'v [Num]> {
	let (n, want) = (values.len(), params.len());
	if n > want {
		let s = if want == 1 { "" } else { "s" };
		return Err(Fail::Arith(format!("<lambda>() takes {want} positional argument{s} but {n} were given")));
	}
	if n < want {
		let missing = &params[n..];
		let names: Vec<String> = missing.iter().map(|p| format!("'{p}'")).collect();
		let list = match names.len() {
			1 => names[0].clone(),
			2 => format!("{} and {}", names[0], names[1]),
			_ => format!("{}, and {}", names[..names.len() - 1].join(", "), names[names.len() - 1]),
		};
		let s = if missing.len() == 1 { "" } else { "s" };
		return Err(Fail::Arith(format!("<lambda>() missing {} required positional argument{s}: {list}", missing.len())));
	}
	Ok(values)
}

/// One argument for a C function that takes exactly one.
fn exactly_one(name: &str, values: &[Num]) -> R<Num> {
	if values.len() != 1 {
		return Err(Fail::Arith(format!("{name}() takes exactly one argument ({} given)", values.len())));
	}
	Ok(values[0])
}

/// A math-module function of one float: NaN out of a number in is a domain error.
fn math_1(x: f64, f: fn(f64) -> f64) -> R<f64> {
	let r = f(x);
	if (r.is_nan() && !x.is_nan()) || (r.is_infinite() && x.is_finite()) {
		return Err(Fail::Value("math domain error".into()));
	}
	Ok(r)
}

/// float(int(x)) after floor(), ceil() or round(): Python's int conversion.
fn to_int(v: f64) -> R<f64> {
	if v.is_nan() {
		return Err(Fail::Value("cannot convert float NaN to integer".into()));
	}
	if v.is_infinite() {
		return Err(Fail::Arith("cannot convert float infinity to integer".into()));
	}
	Ok(v + 0.0)
}

fn function(name: &str, values: &[Num]) -> R<f64> {
	match name {
		"sin" | "cos" | "tan" => {
			let d = real(lambda_args(values, &["d"])?[0])?;
			let f: fn(f64) -> f64 = match name {
				"sin" => f64::sin,
				"cos" => f64::cos,
				_ => f64::tan,
			};
			math_1(d.to_radians(), f)
		}
		"atan" | "asin" | "acos" => {
			let v = real(lambda_args(values, &["v"])?[0])?;
			let f: fn(f64) -> f64 = match name {
				"atan" => f64::atan,
				"asin" => f64::asin,
				_ => f64::acos,
			};
			Ok(math_1(v, f)?.to_degrees())
		}
		"atan2" => {
			let args = lambda_args(values, &["y", "x"])?;
			let (y, x) = (real(args[0])?, real(args[1])?);
			Ok(y.atan2(x).to_degrees())
		}
		"sqrt" => math_1(real(exactly_one("math.sqrt", values)?)?, f64::sqrt),
		"rad" => Ok(real(exactly_one("math.radians", values)?)?.to_radians()),
		"deg" => Ok(real(exactly_one("math.degrees", values)?)?.to_degrees()),
		"floor" => to_int(real(exactly_one("math.floor", values)?)?.floor()),
		"ceil" => to_int(real(exactly_one("math.ceil", values)?)?.ceil()),
		"abs" => match exactly_one("abs", values)? {
			Num::F(x) => Ok(x.abs()),
			Num::C(re, im) => {
				if re.is_infinite() || im.is_infinite() {
					return Ok(f64::INFINITY);
				}
				if re.is_nan() || im.is_nan() {
					return Ok(f64::NAN);
				}
				let r = re.hypot(im);
				if r.is_infinite() {
					return Err(Fail::Arith("absolute value too large".into()));
				}
				Ok(r)
			}
		},
		"round" => {
			match values.len() {
				0 => return Err(Fail::Arith("round() missing required argument 'number' (pos 1)".into())),
				1 | 2 => {}
				n => return Err(Fail::Arith(format!("round() takes at most 2 arguments ({n} given)"))),
			}
			let x = match values[0] {
				Num::F(x) => x,
				Num::C(..) => return Err(Fail::Arith("type complex doesn't define __round__ method".into())),
			};
			if let Some(nd) = values.get(1) {
				return Err(Fail::Arith(format!("'{}' object cannot be interpreted as an integer", nd.type_name())));
			}
			to_int(py::round(x))
		}
		"min" | "max" => {
			if values.is_empty() {
				return Err(Fail::Arith(format!("{name} expected at least 1 argument, got 0")));
			}
			if values.len() == 1 {
				return Err(Fail::Arith(format!("'{}' object is not iterable", values[0].type_name())));
			}
			let mut best = values[0];
			for &v in &values[1..] {
				let (symbol, op) = if name == "min" { ("<", "Lt") } else { (">", "Gt") };
				let better = match (v, best) {
					(Num::F(_), Num::F(_)) => compare(op, v, best)?,
					_ => return Err(not_supported(symbol, v.type_name(), best.type_name())),
				};
				if better {
					best = v;
				}
			}
			real(best)
		}
		"hypot" => {
			let xs: Vec<f64> = values.iter().map(|v| real(*v)).collect::<R<_>>()?;
			Ok(py::hypot(&xs))
		}
		_ => Err(Fail::Value(format!("unknown function {name}"))),
	}
}

// ------------------------------------------------------------------ pick() and label text
/// Each pick(a,b,c) in text: (start char index, start byte, end byte, the items).
fn picks(text: &str) -> Vec<(usize, usize, usize, Vec<String>)> {
	let mut out = Vec::new();
	let mut search = 0;
	while let Some(found) = text[search..].find("pick(") {
		let start = search + found;
		let open = start + 5;
		let rest = &text[open..];
		match rest.find([')', '(']) {
			Some(k) if rest.as_bytes()[k] == b')' => {
				let items = rest[..k].split(',').map(|v| py_strip(v).to_string()).collect();
				out.push((text[..start].chars().count(), start, open + k + 1, items));
				search = open + k + 1;
			}
			_ => {
				search = start + 1;
			}
		}
	}
	out
}

/// Each pick(a,b,c) in text replaced by one of its items, chosen for this copy.
pub fn resolve_picks(text: &str, env: &Env) -> String {
	let found = picks(text);
	if found.is_empty() {
		return text.to_string();
	}
	let mut out = String::with_capacity(text.len());
	let mut last = 0;
	for (at, start, end, items) in found {
		out.push_str(&text[last..start]);
		let picked: &String = rng(env, text, at).choice(&items[..]);
		out.push_str(picked);
		last = end;
	}
	out.push_str(&text[last..]);
	out
}

/// Every text the pick()s in text can become.
pub fn pick_options(text: &str) -> Vec<String> {
	let found = picks(text);
	if found.is_empty() {
		return vec![text.to_string()];
	}
	let mut combos: Vec<Vec<&str>> = vec![vec![]];
	for (_, _, _, items) in &found {
		let mut next = Vec::new();
		for combo in &combos {
			for item in items {
				let mut c = combo.clone();
				c.push(item.as_str());
				next.push(c);
			}
		}
		combos = next;
	}
	combos
		.into_iter()
		.map(|combo| {
			let mut out = String::new();
			let mut last = 0;
			for ((_, start, end, _), value) in found.iter().zip(combo) {
				out.push_str(&text[last..*start]);
				out.push_str(value);
				last = *end;
			}
			out.push_str(&text[last..]);
			out
		})
		.collect()
}

/// {...} spans that hold no braces: (start byte, end byte, inside).
fn braces(text: &str) -> Vec<(usize, usize, String)> {
	let mut out = Vec::new();
	let bytes = text.as_bytes();
	let mut i = 0;
	while i < bytes.len() {
		if bytes[i] == b'{' {
			let mut j = i + 1;
			while j < bytes.len() && bytes[j] != b'{' && bytes[j] != b'}' {
				j += 1;
			}
			if j < bytes.len() && bytes[j] == b'}' && j > i + 1 {
				out.push((i, j + 1, text[i + 1..j].to_string()));
				i = j + 1;
				continue;
			}
		}
		i += 1;
	}
	out
}

fn replace_braces(text: &str, f: &mut dyn FnMut(&str, &str) -> Result<String, String>) -> Result<String, String> {
	let mut out = String::new();
	let mut last = 0;
	for (start, end, inner) in braces(text) {
		out.push_str(&text[last..start]);
		out.push_str(&f(&text[start..end], &inner)?);
		last = end;
	}
	out.push_str(&text[last..]);
	Ok(out)
}

/// Label text for one copy: {name} of a variable holding words put in, each pick() chosen, each
/// {expression} worked out (whole numbers without a point).
pub fn interpolate(text: &str, env: &Env) -> Result<String, String> {
	let text = replace_braces(text, &mut |whole, inner| {
		if let Some(Value::Str(value)) = env.get(py_strip(inner)) {
			if py_float(value).is_none() {
				return Ok(value.to_string());
			}
		}
		Ok(whole.to_string())
	})?;
	let text = resolve_picks(&text, env);
	replace_braces(&text, &mut |_, inner| {
		let value = evaluate(inner, env)?;
		let rounded = py::round(value);
		Ok(if (value - rounded).abs() < 1e-9 { format!("{}", rounded as i64) } else { py::g(value) })
	})
}

/// Text that changes per copy (it has a pick() or an {expression}).
pub fn is_dynamic(text: &str) -> bool {
	text.contains("pick(") || !braces(text).is_empty()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parses_like_python() {
		let env = Env::new();
		for (text, want) in [("-2**2", -4.0), ("2**3**2", 512.0), ("-7//2", -4.0), ("1 if 0 else 2 if 1 else 3", 2.0), ("0x10", 16.0), ("--1", 1.0)] {
			assert_eq!(evaluate(text, &env).unwrap(), want, "{text}");
		}
		assert!(evaluate("1/0", &env).unwrap_err().contains("float division by zero"));
		assert!(evaluate("rand(", &env).unwrap_err().starts_with("not a number or expression"));
	}
}
