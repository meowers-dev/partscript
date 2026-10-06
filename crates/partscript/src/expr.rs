//! Expressions: the arithmetic PartScript accepts anywhere a number goes (Python's expression grammar,
//! numbers only), and the per-copy draws: rand(), pick(), odds(), noise() and rough().
//!
//! Every draw is seeded by the copy it runs for (its __seed__), the expression's text and where in the
//! text the call stands, so the same file always draws the same numbers.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use kitlib::noise::{noise, rough};
use kitlib::py::{self, PyRandom};

use crate::value::{env_frame, Env, Value};

// ------------------------------------------------------------------ the syntax tree
#[derive(Clone, Debug)]
enum Expr {
	Num(f64),
	Constant(String),
	Name(String),
	Attr(Box<Expr>, String),
	Unary(char, Box<Expr>),
	Not(Box<Expr>),
	Bin(&'static str, Box<Expr>, Box<Expr>),
	Compare(Box<Expr>, Vec<(&'static str, Expr)>),
	Bool(bool, Vec<Expr>),
	If(Box<Expr>, Box<Expr>, Box<Expr>),
	Call { func: Box<Expr>, args: Vec<Expr>, keywords: bool, col: usize },
	Other(String, Vec<Expr>),
}

fn dump(e: &Expr) -> String {
	match e {
		Expr::Num(v) => {
			if v.fract() == 0.0 && v.abs() < 1e16 {
				format!("Constant(value={})", *v as i64)
			} else {
				format!("Constant(value={})", py::repr(*v))
			}
		}
		Expr::Constant(text) => format!("Constant(value={text})"),
		Expr::Name(id) => format!("Name(id='{id}', ctx=Load())"),
		Expr::Attr(v, attr) => format!("Attribute(value={}, attr='{attr}', ctx=Load())", dump(v)),
		Expr::Unary(op, v) => format!("UnaryOp(op={}(), operand={})", match op { '-' => "USub", '+' => "UAdd", _ => "Invert" }, dump(v)),
		Expr::Not(v) => format!("UnaryOp(op=Not(), operand={})", dump(v)),
		Expr::Bin(op, a, b) => format!("BinOp(left={}, op={}(), right={})", dump(a), bin_name(op), dump(b)),
		Expr::Compare(left, rest) => format!(
			"Compare(left={}, ops=[{}], comparators=[{}])",
			dump(left),
			rest.iter().map(|(op, _)| format!("{}()", cmp_name(op))).collect::<Vec<_>>().join(", "),
			rest.iter().map(|(_, e)| dump(e)).collect::<Vec<_>>().join(", ")
		),
		Expr::Bool(and, values) => {
			format!("BoolOp(op={}(), values=[{}])", if *and { "And" } else { "Or" }, values.iter().map(dump).collect::<Vec<_>>().join(", "))
		}
		Expr::If(t, b, o) => format!("IfExp(test={}, body={}, orelse={})", dump(t), dump(b), dump(o)),
		Expr::Call { func, args, .. } => {
			format!("Call(func={}, args=[{}], keywords=[])", dump(func), args.iter().map(dump).collect::<Vec<_>>().join(", "))
		}
		Expr::Other(kind, items) => format!("{kind}(elts=[{}], ctx=Load())", items.iter().map(dump).collect::<Vec<_>>().join(", ")),
	}
}

fn bin_name(op: &str) -> &'static str {
	match op {
		"+" => "Add",
		"-" => "Sub",
		"*" => "Mult",
		"/" => "Div",
		"**" => "Pow",
		"%" => "Mod",
		"//" => "FloorDiv",
		"@" => "MatMult",
		"&" => "BitAnd",
		"|" => "BitOr",
		"^" => "BitXor",
		"<<" => "LShift",
		_ => "RShift",
	}
}

fn cmp_name(op: &str) -> &'static str {
	match op {
		"==" => "Eq",
		"!=" => "NotEq",
		"<" => "Lt",
		"<=" => "LtE",
		">" => "Gt",
		">=" => "GtE",
		"is" => "Is",
		"is not" => "IsNot",
		"in" => "In",
		_ => "NotIn",
	}
}

// ------------------------------------------------------------------ tokens
#[derive(Clone, Debug, PartialEq)]
enum Tok {
	Num(f64),
	Imaginary,
	Str(String),
	Name(String),
	Op(&'static str),
}

const OPS: [&str; 36] = [
	"**=", "//=", ">>=", "<<=", "**", "//", "<=", ">=", "==", "!=", "->", ":=", "<<", ">>", "+=", "-=", "*=", "/=", "+", "-", "*", "/", "%", "@",
	"(", ")", "[", "]", "{", "}", ",", ":", ".", "<", ">", "=",
];
const OPS2: [&str; 6] = ["&", "|", "^", "~", ";", "!"];

fn is_name_start(c: char) -> bool {
	c == '_' || c.is_alphabetic()
}

fn is_name_char(c: char) -> bool {
	c == '_' || c.is_alphanumeric()
}

/// Python's lexer for one expression: tokens and their byte offsets. None on a lexical error.
fn lex(text: &str) -> Option<Vec<(Tok, usize)>> {
	let bytes = text.as_bytes();
	let mut out = Vec::new();
	let mut i = 0;
	while i < text.len() {
		let c = text[i..].chars().next()?;
		if c == ' ' || c == '\t' || c == '\x0c' {
			i += 1;
			continue;
		}
		let start = i;
		if c.is_ascii_digit() || (c == '.' && bytes.get(i + 1).is_some_and(|b| b.is_ascii_digit())) {
			let (tok, len) = number(&text[i..])?;
			out.push((tok, start));
			i += len;
			// a number runs straight into a name: 1x is an error (1if is allowed by Python, rarely wanted)
			if let Some(next) = text[i..].chars().next() {
				if is_name_char(next) && !text[i..].starts_with("if") && !text[i..].starts_with("else") && !text[i..].starts_with("and")
					&& !text[i..].starts_with("or") && !text[i..].starts_with("not") && !text[i..].starts_with("in") && !text[i..].starts_with("is") {
					return None;
				}
			}
			continue;
		}
		if is_name_start(c) {
			let mut j = i;
			while let Some(ch) = text[j..].chars().next() {
				if !is_name_char(ch) {
					break;
				}
				j += ch.len_utf8();
			}
			let word = &text[i..j];
			// string prefixes: r"..", b'..', f"..."
			if j < text.len() && (bytes[j] == b'"' || bytes[j] == b'\'') && word.len() <= 2 && word.chars().all(|ch| "rRbBfFuU".contains(ch)) {
				let (s, len) = string(&text[j..])?;
				out.push((Tok::Str(s), start));
				i = j + len;
				continue;
			}
			out.push((Tok::Name(word.to_string()), start));
			i = j;
			continue;
		}
		if c == '"' || c == '\'' {
			let (s, len) = string(&text[i..])?;
			out.push((Tok::Str(s), start));
			i += len;
			continue;
		}
		if let Some(op) = OPS.iter().find(|op| text[i..].starts_with(**op)) {
			out.push((Tok::Op(op), start));
			i += op.len();
			continue;
		}
		if let Some(op) = OPS2.iter().find(|op| text[i..].starts_with(**op)) {
			if *op == "!" || *op == ";" {
				return None;
			}
			out.push((Tok::Op(op), start));
			i += op.len();
			continue;
		}
		return None;
	}
	Some(out)
}

fn string(text: &str) -> Option<(String, usize)> {
	let quote = text.chars().next()?;
	let mut i = 1;
	let mut out = String::new();
	while i < text.len() {
		let c = text[i..].chars().next()?;
		if c == '\\' {
			i += 1;
			let next = text[i..].chars().next()?;
			out.push(next);
			i += next.len_utf8();
			continue;
		}
		if c == quote {
			return Some((out, i + 1));
		}
		out.push(c);
		i += c.len_utf8();
	}
	None
}

/// A Python number literal at the start of text: (token, its length).
fn number(text: &str) -> Option<(Tok, usize)> {
	let b = text.as_bytes();
	let digits = |from: usize, ok: &dyn Fn(u8) -> bool| -> usize {
		// digits with single underscores between them
		let mut i = from;
		while i < b.len() {
			if ok(b[i]) {
				i += 1;
			} else if b[i] == b'_' && i > from && i + 1 < b.len() && ok(b[i + 1]) && ok(b[i - 1]) {
				i += 1;
			} else {
				break;
			}
		}
		i
	};
	if b.len() > 1 && b[0] == b'0' && matches!(b[1], b'x' | b'X' | b'o' | b'O' | b'b' | b'B') {
		let (radix, ok): (u32, Box<dyn Fn(u8) -> bool>) = match b[1] {
			b'x' | b'X' => (16, Box::new(|c: u8| c.is_ascii_hexdigit())),
			b'o' | b'O' => (8, Box::new(|c: u8| (b'0'..=b'7').contains(&c))),
			_ => (2, Box::new(|c: u8| c == b'0' || c == b'1')),
		};
		let mut from = 2;
		if b.get(2) == Some(&b'_') {
			from = 3;
		}
		let end = digits(from, &*ok);
		if end == from {
			return None;
		}
		let clean: String = text[from..end].chars().filter(|c| *c != '_').collect();
		let value = u128::from_str_radix(&clean, radix).ok()? as f64;
		return Some((Tok::Num(value), end));
	}
	let is_digit = |c: u8| c.is_ascii_digit();
	let int_end = digits(0, &is_digit);
	let mut end = int_end;
	let mut float = false;
	if b.get(end) == Some(&b'.') {
		float = true;
		end += 1;
		if b.get(end).is_some_and(|c| c.is_ascii_digit()) {
			end = digits(end, &is_digit);
		}
	}
	if matches!(b.get(end), Some(b'e') | Some(b'E')) {
		let mut k = end + 1;
		if matches!(b.get(k), Some(b'+') | Some(b'-')) {
			k += 1;
		}
		if b.get(k).is_some_and(|c| c.is_ascii_digit()) {
			end = digits(k, &is_digit);
			float = true;
		}
	}
	if matches!(b.get(end), Some(b'j') | Some(b'J')) {
		return Some((Tok::Imaginary, end + 1));
	}
	let clean: String = text[..end].chars().filter(|c| *c != '_').collect();
	if !float && clean.len() > 1 && clean.starts_with('0') && clean.chars().any(|c| c != '0') {
		return None; // leading zeros in a decimal integer
	}
	let value: f64 = clean.parse().ok()?;
	Some((Tok::Num(value), end))
}

// ------------------------------------------------------------------ the parser
struct Parser {
	toks: Vec<(Tok, usize)>,
	pos: usize,
}

type P<T> = Option<T>;

impl Parser {
	fn peek(&self) -> Option<&Tok> {
		self.toks.get(self.pos).map(|(t, _)| t)
	}

	fn start(&self) -> usize {
		self.toks.get(self.pos).map(|(_, s)| *s).unwrap_or(0)
	}

	fn eat_op(&mut self, op: &str) -> bool {
		if self.peek() == Some(&Tok::Op(OPS.iter().chain(OPS2.iter()).find(|o| **o == op).copied().unwrap_or("?"))) {
			self.pos += 1;
			true
		} else {
			false
		}
	}

	fn eat_word(&mut self, word: &str) -> bool {
		if matches!(self.peek(), Some(Tok::Name(w)) if w == word) {
			self.pos += 1;
			true
		} else {
			false
		}
	}

	fn is_word(&self, word: &str) -> bool {
		matches!(self.peek(), Some(Tok::Name(w)) if w == word)
	}

	fn expression(&mut self) -> P<Expr> {
		if self.is_word("lambda") {
			return None;
		}
		let body = self.disjunction()?;
		if self.eat_word("if") {
			let test = self.disjunction()?;
			if !self.eat_word("else") {
				return None;
			}
			let orelse = self.expression()?;
			return Some(Expr::If(Box::new(test), Box::new(body), Box::new(orelse)));
		}
		Some(body)
	}

	fn disjunction(&mut self) -> P<Expr> {
		let first = self.conjunction()?;
		let mut values = vec![first];
		while self.eat_word("or") {
			values.push(self.conjunction()?);
		}
		Some(if values.len() == 1 { values.pop().unwrap() } else { Expr::Bool(false, values) })
	}

	fn conjunction(&mut self) -> P<Expr> {
		let first = self.inversion()?;
		let mut values = vec![first];
		while self.eat_word("and") {
			values.push(self.inversion()?);
		}
		Some(if values.len() == 1 { values.pop().unwrap() } else { Expr::Bool(true, values) })
	}

	fn inversion(&mut self) -> P<Expr> {
		if self.eat_word("not") {
			return Some(Expr::Not(Box::new(self.inversion()?)));
		}
		self.comparison()
	}

	fn comparison(&mut self) -> P<Expr> {
		let left = self.bitor()?;
		let mut rest = Vec::new();
		loop {
			let op: &'static str = match self.peek() {
				Some(Tok::Op("<")) => "<",
				Some(Tok::Op(">")) => ">",
				Some(Tok::Op("<=")) => "<=",
				Some(Tok::Op(">=")) => ">=",
				Some(Tok::Op("==")) => "==",
				Some(Tok::Op("!=")) => "!=",
				Some(Tok::Name(w)) if w == "in" => "in",
				Some(Tok::Name(w)) if w == "is" => "is",
				Some(Tok::Name(w)) if w == "not" && matches!(self.toks.get(self.pos + 1), Some((Tok::Name(n), _)) if n == "in") => "not in",
				_ => break,
			};
			self.pos += 1;
			let op = if op == "not in" {
				self.pos += 1;
				"not in"
			} else if op == "is" && self.eat_word("not") {
				"is not"
			} else {
				op
			};
			rest.push((op, self.bitor()?));
		}
		Some(if rest.is_empty() { left } else { Expr::Compare(Box::new(left), rest) })
	}

	fn binary(&mut self, ops: &[&'static str], next: fn(&mut Parser) -> P<Expr>) -> P<Expr> {
		let mut left = next(self)?;
		loop {
			let Some(Tok::Op(op)) = self.peek() else { break };
			let Some(op) = ops.iter().find(|o| **o == *op).copied() else { break };
			self.pos += 1;
			let right = next(self)?;
			left = Expr::Bin(op, Box::new(left), Box::new(right));
		}
		Some(left)
	}

	fn bitor(&mut self) -> P<Expr> {
		self.binary(&["|"], Parser::bitxor)
	}

	fn bitxor(&mut self) -> P<Expr> {
		self.binary(&["^"], Parser::bitand)
	}

	fn bitand(&mut self) -> P<Expr> {
		self.binary(&["&"], Parser::shift)
	}

	fn shift(&mut self) -> P<Expr> {
		self.binary(&["<<", ">>"], Parser::sum)
	}

	fn sum(&mut self) -> P<Expr> {
		self.binary(&["+", "-"], Parser::term)
	}

	fn term(&mut self) -> P<Expr> {
		self.binary(&["*", "/", "//", "%", "@"], Parser::factor)
	}

	fn factor(&mut self) -> P<Expr> {
		for (op, c) in [("-", '-'), ("+", '+'), ("~", '~')] {
			if self.eat_op(op) {
				return Some(Expr::Unary(c, Box::new(self.factor()?)));
			}
		}
		self.power()
	}

	fn power(&mut self) -> P<Expr> {
		let base = self.primary()?;
		if self.eat_op("**") {
			let exponent = self.factor()?;
			return Some(Expr::Bin("**", Box::new(base), Box::new(exponent)));
		}
		Some(base)
	}

	fn primary(&mut self) -> P<Expr> {
		let start = self.start();
		let mut node = self.atom()?;
		loop {
			if self.eat_op("(") {
				let mut args = Vec::new();
				let mut keywords = false;
				if !self.eat_op(")") {
					loop {
						if matches!(self.peek(), Some(Tok::Op("*")) | Some(Tok::Op("**"))) {
							self.pos += 1;
							let inner = self.expression()?;
							args.push(Expr::Other("Starred".into(), vec![inner]));
						} else if matches!(self.peek(), Some(Tok::Name(_))) && self.toks.get(self.pos + 1).is_some_and(|(t, _)| *t == Tok::Op("=")) {
							self.pos += 2;
							self.expression()?;
							keywords = true;
						} else {
							args.push(self.expression()?);
						}
						if self.eat_op(",") {
							if self.eat_op(")") {
								break;
							}
							continue;
						}
						if self.eat_op(")") {
							break;
						}
						return None;
					}
				}
				node = Expr::Call { func: Box::new(node), args, keywords, col: start };
			} else if self.eat_op(".") {
				match self.peek().cloned() {
					Some(Tok::Name(attr)) => {
						self.pos += 1;
						node = Expr::Attr(Box::new(node), attr);
					}
					_ => return None,
				}
			} else if self.eat_op("[") {
				let index = self.expression()?;
				if !self.eat_op("]") {
					return None;
				}
				node = Expr::Other("Subscript".into(), vec![node, index]);
			} else {
				break;
			}
		}
		Some(node)
	}

	fn atom(&mut self) -> P<Expr> {
		let tok = self.peek().cloned()?;
		self.pos += 1;
		match tok {
			Tok::Num(v) => Some(Expr::Num(v)),
			Tok::Imaginary => Some(Expr::Constant("complex".into())),
			Tok::Str(s) => {
				let mut text = s;
				while let Some(Tok::Str(more)) = self.peek().cloned() {
					self.pos += 1;
					text.push_str(&more);
				}
				Some(Expr::Constant(py::repr_str(&text)))
			}
			Tok::Name(word) => match word.as_str() {
				"True" => Some(Expr::Num(1.0)),
				"False" => Some(Expr::Num(0.0)),
				"None" => Some(Expr::Constant("None".into())),
				"and" | "or" | "not" | "if" | "else" | "in" | "is" | "lambda" | "for" | "while" | "def" | "class" | "return" | "import" | "from"
				| "as" | "with" | "yield" | "await" | "async" | "del" | "pass" | "break" | "continue" | "global" | "nonlocal" | "raise" | "try"
				| "except" | "finally" | "elif" | "assert" => None,
				_ => Some(Expr::Name(word)),
			},
			Tok::Op("(") => {
				if self.eat_op(")") {
					return Some(Expr::Other("Tuple".into(), vec![]));
				}
				let first = self.expression()?;
				if self.eat_op(")") {
					return Some(first);
				}
				let mut items = vec![first];
				while self.eat_op(",") {
					if self.eat_op(")") {
						return Some(Expr::Other("Tuple".into(), items));
					}
					items.push(self.expression()?);
				}
				if !self.eat_op(")") {
					return None;
				}
				Some(Expr::Other("Tuple".into(), items))
			}
			Tok::Op("[") => {
				let mut items = Vec::new();
				if self.eat_op("]") {
					return Some(Expr::Other("List".into(), items));
				}
				loop {
					items.push(self.expression()?);
					if self.eat_op(",") {
						if self.eat_op("]") {
							break;
						}
						continue;
					}
					if self.eat_op("]") {
						break;
					}
					return None;
				}
				Some(Expr::Other("List".into(), items))
			}
			Tok::Op("{") => {
				let mut depth = 1;
				while depth > 0 {
					match self.peek()? {
						Tok::Op("{") => depth += 1,
						Tok::Op("}") => depth -= 1,
						_ => {}
					}
					self.pos += 1;
				}
				Some(Expr::Other("Dict".into(), vec![]))
			}
			_ => None,
		}
	}
}

thread_local! {
	static PARSED: RefCell<HashMap<String, Option<Rc<Expr>>>> = RefCell::new(HashMap::new());
}

fn parse(text: &str) -> Option<Rc<Expr>> {
	if let Some(found) = PARSED.with(|cache| cache.borrow().get(text).cloned()) {
		return found;
	}
	let parsed = lex(text).and_then(|toks| {
		let mut parser = Parser { toks, pos: 0 };
		let e = parser.expression()?;
		if parser.pos != parser.toks.len() {
			return None;
		}
		Some(Rc::new(e))
	});
	PARSED.with(|cache| {
		let mut cache = cache.borrow_mut();
		if cache.len() > 20000 {
			cache.clear();
		}
		cache.insert(text.to_string(), parsed.clone());
	});
	parsed
}

// ------------------------------------------------------------------ evaluation
enum Fail {
	/// a name nobody defined (reported with the whole expression)
	Unknown(String),
	/// ZeroDivisionError, OverflowError, TypeError: reported as 'text': message
	Arith(String),
	/// anything else, reported as it is
	Value(String),
}

type R<T> = Result<T, Fail>;

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

/// Python's float(text): decimals with underscores between digits, inf, infinity, nan, any case.
pub fn py_float(text: &str) -> Option<f64> {
	let t = py_strip(text);
	if t.is_empty() {
		return None;
	}
	let (sign, body) = match t.as_bytes()[0] {
		b'-' => (-1.0, &t[1..]),
		b'+' => (1.0, &t[1..]),
		_ => (1.0, t),
	};
	let lower = body.to_ascii_lowercase();
	if lower == "inf" || lower == "infinity" {
		return Some(sign * f64::INFINITY);
	}
	if lower == "nan" {
		return Some(f64::NAN);
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
	c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// Python's str.strip().
pub fn py_strip(text: &str) -> &str {
	text.trim_matches(py_isspace)
}

/// A number from an expression over env's numbers.
pub fn evaluate(text: &str, env: &Env) -> Result<f64, String> {
	let text = py_strip(text);
	if text.is_empty() {
		return Err("empty number".into());
	}
	if let Some(v) = py_float(text) {
		return Ok(v);
	}
	let resolved;
	let text = if text.contains("pick(") {
		resolved = resolve_picks(text, env);
		resolved.as_str()
	} else {
		text
	};
	let Some(tree) = parse(text) else {
		return Err(format!("not a number or expression: {}", py::repr_str(text)));
	};
	match walk(&tree, text, env) {
		Ok(v) => Ok(v),
		Err(Fail::Unknown(name)) => Err(format!("unknown name {name} in {}", py::repr_str(text))),
		Err(Fail::Arith(message)) => Err(format!("{}: {message}", py::repr_str(text))),
		Err(Fail::Value(message)) => Err(message),
	}
}

fn truthy(v: f64) -> bool {
	v != 0.0
}

fn walk(node: &Expr, text: &str, env: &Env) -> R<f64> {
	match node {
		Expr::Num(v) => Ok(*v),
		Expr::Bin(op, a, b) if matches!(*op, "+" | "-" | "*" | "/" | "**" | "%" | "//") => {
			let (x, y) = (walk(a, text, env)?, walk(b, text, env)?);
			binary(op, x, y)
		}
		Expr::Unary(op, v) if *op == '-' || *op == '+' => {
			let value = walk(v, text, env)?;
			Ok(if *op == '-' { -value } else { value })
		}
		Expr::Not(v) => Ok(if truthy(walk(v, text, env)?) { 0.0 } else { 1.0 }),
		Expr::Compare(left, rest) if rest.iter().all(|(op, _)| matches!(*op, "==" | "!=" | "<" | "<=" | ">" | ">=")) => {
			let mut left = walk(left, text, env)?;
			for (op, right) in rest {
				let right = walk(right, text, env)?;
				let holds = match *op {
					"==" => left == right,
					"!=" => left != right,
					"<" => left < right,
					"<=" => left <= right,
					">" => left > right,
					_ => left >= right,
				};
				if !holds {
					return Ok(0.0);
				}
				left = right;
			}
			Ok(1.0)
		}
		Expr::Bool(and, values) => {
			let values: Vec<f64> = values.iter().map(|v| walk(v, text, env)).collect::<R<_>>()?;
			Ok(if *and { values.iter().all(|v| truthy(*v)) as i64 as f64 } else { values.iter().any(|v| truthy(*v)) as i64 as f64 })
		}
		Expr::If(test, body, orelse) => {
			if truthy(walk(test, text, env)?) {
				walk(body, text, env)
			} else {
				walk(orelse, text, env)
			}
		}
		Expr::Call { func, args, keywords: false, col } if matches!(&**func, Expr::Name(n) if n == "noise" || n == "rough") => {
			let values: Vec<f64> = args.iter().map(|a| walk(a, text, env)).collect::<R<_>>()?;
			let Expr::Name(name) = &**func else { unreachable!() };
			let _ = col;
			if values.is_empty() || values.len() > 3 {
				return Err(Fail::Value(format!("{name}(x[, y[, z]]): one to three numbers, a point in space")));
			}
			let seed = match env.get("__seed__") {
				Some(v) => v.text(),
				None => String::new(),
			};
			let seed = seed.split('|').next().unwrap_or("").to_string();
			let (x, y, z) = (values[0], values.get(1).copied().unwrap_or(0.0), values.get(2).copied().unwrap_or(0.0));
			Ok(if name == "noise" { noise(x, y, z, &seed) } else { rough(x, y, z, &seed, 4) })
		}
		Expr::Call { func, args, keywords: false, col } if matches!(&**func, Expr::Name(n) if n == "rand") => {
			let bounds: Vec<f64> = args.iter().map(|a| walk(a, text, env)).collect::<R<_>>()?;
			let (low, high) = match bounds.len() {
				0 => (0.0, 1.0),
				1 => (0.0, bounds[0]),
				_ => (bounds[0], bounds[1]),
			};
			Ok(rng(env, text, *col).uniform(low, high))
		}
		Expr::Call { func, args, keywords: false, col } if matches!(&**func, Expr::Name(n) if n == "odds") => {
			let weights: Vec<f64> = args.iter().map(|a| walk(a, text, env)).collect::<R<_>>()?;
			let total = py::sum(weights.iter().copied());
			if weights.is_empty() || weights.iter().any(|w| *w < 0.0) || total <= 0.0 {
				return Err(Fail::Value("odds(w0, w1, ...): weights of 0 or more, at least one above 0".into()));
			}
			let mut roll = rng(env, text, *col).uniform(0.0, total);
			for (index, weight) in weights.iter().enumerate() {
				roll -= weight;
				if roll < 0.0 {
					return Ok(index as f64);
				}
			}
			Ok(weights.iter().rposition(|w| *w > 0.0).unwrap_or(0) as f64)
		}
		Expr::Attr(value, attr) if matches!(&**value, Expr::Name(_)) => {
			let Expr::Name(id) = &**value else { unreachable!() };
			match env.get(id) {
				Some(Value::Bounds(b)) => b.number(attr, env_frame(env).as_deref()).map_err(Fail::Value),
				None => Err(Fail::Unknown(id.clone())),
				Some(_) => Err(Fail::Value(format!("{id}.{attr}: {id} is not a named shape"))),
			}
		}
		Expr::Name(id) => {
			if let Some(value) = env.get(id) {
				return match value {
					Value::Bounds(_) => Err(Fail::Value(format!(
						"{id} is a named shape: say which number (left right front back bottom top x y z w d h), as in {id}.top"
					))),
					Value::Num(v) => Ok(*v),
					Value::Int(v) => Ok(*v as f64),
					other => evaluate(&other.text(), env).map_err(|_| Fail::Value(format!("{id} is {}, not a number", other.repr()))),
				};
			}
			match id.as_str() {
				"pi" => Ok(std::f64::consts::PI),
				"tau" => Ok(std::f64::consts::TAU),
				_ => Err(Fail::Unknown(id.clone())),
			}
		}
		Expr::Call { func, args, keywords: false, .. } if matches!(&**func, Expr::Name(n) if FUNCS.contains(&n.as_str())) => {
			let Expr::Name(name) = &**func else { unreachable!() };
			let values: Vec<f64> = args.iter().map(|a| walk(a, text, env)).collect::<R<_>>()?;
			function(name, &values)
		}
		other => {
			let mut text = dump(other);
			text.truncate(text.char_indices().nth(40).map(|(i, _)| i).unwrap_or(text.len()));
			Err(Fail::Value(format!("unsupported in an expression: {text}")))
		}
	}
}

const FUNCS: [&str; 17] = ["sin", "cos", "tan", "sqrt", "abs", "min", "max", "floor", "ceil", "round", "atan2", "hypot", "rad", "deg", "atan", "asin", "acos"];

fn binary(op: &str, x: f64, y: f64) -> R<f64> {
	match op {
		"+" => Ok(x + y),
		"-" => Ok(x - y),
		"*" => Ok(x * y),
		"/" => {
			if y == 0.0 {
				Err(Fail::Arith("float division by zero".into()))
			} else {
				Ok(x / y)
			}
		}
		"%" => py::modulo(x, y).ok_or_else(|| Fail::Arith("float modulo by zero".into())),
		"//" => py::floordiv(x, y).ok_or_else(|| Fail::Arith("float floor division by zero".into())),
		_ => power(x, y),
	}
}

/// Python's float ** float.
fn power(x: f64, y: f64) -> R<f64> {
	if y == 0.0 {
		return Ok(1.0);
	}
	if x.is_nan() || y.is_nan() {
		return Ok(if x == 1.0 { 1.0 } else { f64::NAN });
	}
	if x == 0.0 && y < 0.0 && y.is_finite() {
		return Err(Fail::Arith("zero to a negative power".into()));
	}
	if x < 0.0 && x.is_finite() && y.is_finite() && y.fract() != 0.0 {
		// Python gives a complex number here; nothing a size or a position can use
		return Err(Fail::Value("math domain error".into()));
	}
	let out = x.powf(y);
	if out.is_infinite() && x.is_finite() && y.is_finite() {
		return Err(Fail::Arith("(34, 'Numerical result out of range')".into()));
	}
	Ok(out)
}

fn one(name: &str, values: &[f64]) -> R<f64> {
	if values.len() != 1 {
		return Err(Fail::Arith(if values.is_empty() {
			format!("{name}() missing 1 required positional argument")
		} else {
			format!("{name}() takes 1 positional argument but {} were given", values.len())
		}));
	}
	Ok(values[0])
}

fn to_int(v: f64) -> R<f64> {
	if v.is_nan() {
		return Err(Fail::Value("cannot convert float NaN to integer".into()));
	}
	if v.is_infinite() {
		return Err(Fail::Arith("cannot convert float infinity to integer".into()));
	}
	Ok(v)
}

fn function(name: &str, values: &[f64]) -> R<f64> {
	let domain = |v: f64| if v.is_nan() { Err(Fail::Value("math domain error".into())) } else { Ok(v) };
	match name {
		"sin" => Ok(one(name, values)?.to_radians().sin()),
		"cos" => Ok(one(name, values)?.to_radians().cos()),
		"tan" => Ok(one(name, values)?.to_radians().tan()),
		"sqrt" => {
			let v = one(name, values)?;
			if v < 0.0 {
				Err(Fail::Value("math domain error".into()))
			} else {
				Ok(v.sqrt())
			}
		}
		"abs" => Ok(one(name, values)?.abs()),
		"min" | "max" => {
			if values.is_empty() {
				return Err(Fail::Arith(format!("{name} expected at least 1 argument, got 0")));
			}
			if values.len() == 1 {
				return Err(Fail::Arith("'float' object is not iterable".into()));
			}
			let mut best = values[0];
			for &v in &values[1..] {
				if (name == "min" && v < best) || (name == "max" && v > best) {
					best = v;
				}
			}
			Ok(best)
		}
		"floor" => Ok(to_int(one(name, values)?)?.floor()),
		"ceil" => Ok(to_int(one(name, values)?)?.ceil()),
		"round" => {
			if values.len() == 2 {
				return Err(Fail::Arith("'float' object cannot be interpreted as an integer".into()));
			}
			Ok(py::round(to_int(one(name, values)?)?))
		}
		"atan2" => {
			if values.len() != 2 {
				return Err(Fail::Arith(format!("<lambda>() takes 2 positional arguments but {} were given", values.len())));
			}
			Ok(values[0].atan2(values[1]).to_degrees())
		}
		"hypot" => Ok(py::hypot(values)),
		"rad" => Ok(one(name, values)?.to_radians()),
		"deg" => Ok(one(name, values)?.to_degrees()),
		"atan" => Ok(one(name, values)?.atan().to_degrees()),
		"asin" => {
			let v = one(name, values)?;
			domain(if (-1.0..=1.0).contains(&v) { v.asin() } else { f64::NAN }).map(f64::to_degrees)
		}
		"acos" => {
			let v = one(name, values)?;
			domain(if (-1.0..=1.0).contains(&v) { v.acos() } else { f64::NAN }).map(f64::to_degrees)
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
