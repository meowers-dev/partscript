//! JSON: a value type, a writer that writes what Python's json.dumps writes (separators, indent, ASCII
//! escapes, float repr, key order or sorted keys) and a reader.

use crate::py;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
	Null,
	Bool(bool),
	Int(i64),
	Float(f64),
	Str(String),
	List(Vec<Json>),
	Dict(Vec<(String, Json)>),
}

impl Json {
	pub fn dict() -> Json {
		Json::Dict(Vec::new())
	}

	/// Sets a key (replacing it where it is, else adding it at the end, as a Python dict does).
	pub fn set(&mut self, key: &str, value: impl Into<Json>) -> &mut Json {
		if let Json::Dict(items) = self {
			let value = value.into();
			if let Some(item) = items.iter_mut().find(|(k, _)| k == key) {
				item.1 = value;
			} else {
				items.push((key.to_string(), value));
			}
		}
		self
	}

	pub fn get(&self, key: &str) -> Option<&Json> {
		match self {
			Json::Dict(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
			_ => None,
		}
	}

	pub fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
		match self {
			Json::Dict(items) => items.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
			_ => None,
		}
	}

	pub fn remove(&mut self, key: &str) -> Option<Json> {
		match self {
			Json::Dict(items) => items.iter().position(|(k, _)| k == key).map(|i| items.remove(i).1),
			_ => None,
		}
	}

	pub fn idx(&self, i: usize) -> &Json {
		match self {
			Json::List(items) => &items[i],
			_ => panic!("not a list"),
		}
	}

	pub fn as_f64(&self) -> f64 {
		match self {
			Json::Int(v) => *v as f64,
			Json::Float(v) => *v,
			Json::Bool(b) => *b as i64 as f64,
			_ => panic!("not a number: {self:?}"),
		}
	}

	pub fn as_i64(&self) -> i64 {
		match self {
			Json::Int(v) => *v,
			Json::Float(v) => *v as i64,
			Json::Bool(b) => *b as i64,
			_ => panic!("not a number: {self:?}"),
		}
	}

	pub fn as_str(&self) -> &str {
		match self {
			Json::Str(s) => s,
			_ => panic!("not a string: {self:?}"),
		}
	}

	pub fn as_list(&self) -> &[Json] {
		match self {
			Json::List(items) => items,
			_ => panic!("not a list: {self:?}"),
		}
	}

	pub fn as_dict(&self) -> &[(String, Json)] {
		match self {
			Json::Dict(items) => items,
			_ => panic!("not a dict: {self:?}"),
		}
	}

	pub fn is_empty_container(&self) -> bool {
		matches!(self, Json::List(v) if v.is_empty()) || matches!(self, Json::Dict(v) if v.is_empty())
	}

	/// json.dumps(value, separators=(",", ":")) when compact, else the default separators (", ", ": ").
	pub fn dumps(&self, compact: bool) -> String {
		let mut out = String::new();
		self.write(&mut out, &Style { item: if compact { "," } else { ", " }, key: if compact { ":" } else { ": " }, indent: None, sort: false }, 0);
		out
	}

	/// json.dumps(value, sort_keys=True)
	pub fn dumps_sorted(&self) -> String {
		let mut out = String::new();
		self.write(&mut out, &Style { item: ", ", key: ": ", indent: None, sort: true }, 0);
		out
	}

	/// json.dumps(value, indent=n)
	pub fn dumps_indent(&self, indent: usize) -> String {
		let mut out = String::new();
		self.write(&mut out, &Style { item: ",", key: ": ", indent: Some(indent), sort: false }, 0);
		out
	}

	fn write(&self, out: &mut String, style: &Style, depth: usize) {
		match self {
			Json::Null => out.push_str("null"),
			Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
			Json::Int(v) => out.push_str(&v.to_string()),
			Json::Float(v) => out.push_str(&float_text(*v)),
			Json::Str(s) => write_str(out, s),
			Json::List(items) => {
				if items.is_empty() {
					out.push_str("[]");
					return;
				}
				out.push('[');
				for (i, item) in items.iter().enumerate() {
					if i > 0 {
						out.push_str(style.item);
					}
					style.newline(out, depth + 1);
					item.write(out, style, depth + 1);
				}
				style.newline(out, depth);
				out.push(']');
			}
			Json::Dict(items) => {
				if items.is_empty() {
					out.push_str("{}");
					return;
				}
				let mut order: Vec<&(String, Json)> = items.iter().collect();
				if style.sort {
					order.sort_by(|a, b| a.0.cmp(&b.0));
				}
				out.push('{');
				for (i, (key, value)) in order.into_iter().enumerate() {
					if i > 0 {
						out.push_str(style.item);
					}
					style.newline(out, depth + 1);
					write_str(out, key);
					out.push_str(style.key);
					value.write(out, style, depth + 1);
				}
				style.newline(out, depth);
				out.push('}');
			}
		}
	}

	/// Parses JSON text.
	pub fn parse(text: &str) -> Result<Json, String> {
		let mut reader = Reader { bytes: text.as_bytes(), pos: 0 };
		let value = reader.value()?;
		reader.space();
		if reader.pos != reader.bytes.len() {
			return Err(format!("extra data at {}", reader.pos));
		}
		Ok(value)
	}
}

struct Style {
	item: &'static str,
	key: &'static str,
	indent: Option<usize>,
	sort: bool,
}

impl Style {
	fn newline(&self, out: &mut String, depth: usize) {
		if let Some(n) = self.indent {
			out.push('\n');
			out.push_str(&" ".repeat(n * depth));
		}
	}
}

fn float_text(v: f64) -> String {
	if v.is_nan() {
		"NaN".into()
	} else if v.is_infinite() {
		if v > 0.0 { "Infinity".into() } else { "-Infinity".into() }
	} else {
		py::repr(v)
	}
}

fn write_str(out: &mut String, s: &str) {
	out.push('"');
	for ch in s.chars() {
		match ch {
			'"' => out.push_str("\\\""),
			'\\' => out.push_str("\\\\"),
			'\n' => out.push_str("\\n"),
			'\r' => out.push_str("\\r"),
			'\t' => out.push_str("\\t"),
			'\u{08}' => out.push_str("\\b"),
			'\u{0c}' => out.push_str("\\f"),
			c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
				let mut buf = [0u16; 2];
				for unit in c.encode_utf16(&mut buf) {
					out.push_str(&format!("\\u{:04x}", unit));
				}
			}
			c => out.push(c),
		}
	}
	out.push('"');
}

impl From<f64> for Json {
	fn from(v: f64) -> Json {
		Json::Float(v)
	}
}
impl From<i64> for Json {
	fn from(v: i64) -> Json {
		Json::Int(v)
	}
}
impl From<usize> for Json {
	fn from(v: usize) -> Json {
		Json::Int(v as i64)
	}
}
impl From<i32> for Json {
	fn from(v: i32) -> Json {
		Json::Int(v as i64)
	}
}
impl From<u32> for Json {
	fn from(v: u32) -> Json {
		Json::Int(v as i64)
	}
}
impl From<bool> for Json {
	fn from(v: bool) -> Json {
		Json::Bool(v)
	}
}
impl From<&str> for Json {
	fn from(v: &str) -> Json {
		Json::Str(v.to_string())
	}
}
impl From<String> for Json {
	fn from(v: String) -> Json {
		Json::Str(v)
	}
}
impl<T: Into<Json>> From<Vec<T>> for Json {
	fn from(v: Vec<T>) -> Json {
		Json::List(v.into_iter().map(Into::into).collect())
	}
}

struct Reader<'a> {
	bytes: &'a [u8],
	pos: usize,
}

impl Reader<'_> {
	fn space(&mut self) {
		while self.pos < self.bytes.len() && matches!(self.bytes[self.pos], b' ' | b'\n' | b'\r' | b'\t') {
			self.pos += 1;
		}
	}

	fn value(&mut self) -> Result<Json, String> {
		self.space();
		let Some(&c) = self.bytes.get(self.pos) else { return Err("unexpected end".into()) };
		match c {
			b'{' => {
				self.pos += 1;
				let mut items = Vec::new();
				self.space();
				if self.bytes.get(self.pos) == Some(&b'}') {
					self.pos += 1;
					return Ok(Json::Dict(items));
				}
				loop {
					self.space();
					let key = match self.value()? {
						Json::Str(s) => s,
						_ => return Err("a key must be a string".into()),
					};
					self.space();
					if self.bytes.get(self.pos) != Some(&b':') {
						return Err(format!("expected : at {}", self.pos));
					}
					self.pos += 1;
					let value = self.value()?;
					items.push((key, value));
					self.space();
					match self.bytes.get(self.pos) {
						Some(b',') => self.pos += 1,
						Some(b'}') => {
							self.pos += 1;
							return Ok(Json::Dict(items));
						}
						_ => return Err(format!("expected , or }} at {}", self.pos)),
					}
				}
			}
			b'[' => {
				self.pos += 1;
				let mut items = Vec::new();
				self.space();
				if self.bytes.get(self.pos) == Some(&b']') {
					self.pos += 1;
					return Ok(Json::List(items));
				}
				loop {
					items.push(self.value()?);
					self.space();
					match self.bytes.get(self.pos) {
						Some(b',') => self.pos += 1,
						Some(b']') => {
							self.pos += 1;
							return Ok(Json::List(items));
						}
						_ => return Err(format!("expected , or ] at {}", self.pos)),
					}
				}
			}
			b'"' => self.string().map(Json::Str),
			b't' if self.bytes[self.pos..].starts_with(b"true") => {
				self.pos += 4;
				Ok(Json::Bool(true))
			}
			b'f' if self.bytes[self.pos..].starts_with(b"false") => {
				self.pos += 5;
				Ok(Json::Bool(false))
			}
			b'n' if self.bytes[self.pos..].starts_with(b"null") => {
				self.pos += 4;
				Ok(Json::Null)
			}
			b'N' if self.bytes[self.pos..].starts_with(b"NaN") => {
				self.pos += 3;
				Ok(Json::Float(f64::NAN))
			}
			b'I' if self.bytes[self.pos..].starts_with(b"Infinity") => {
				self.pos += 8;
				Ok(Json::Float(f64::INFINITY))
			}
			b'-' if self.bytes[self.pos..].starts_with(b"-Infinity") => {
				self.pos += 9;
				Ok(Json::Float(f64::NEG_INFINITY))
			}
			_ => self.number(),
		}
	}

	fn number(&mut self) -> Result<Json, String> {
		let start = self.pos;
		let mut float = false;
		while let Some(&c) = self.bytes.get(self.pos) {
			match c {
				b'0'..=b'9' | b'-' | b'+' => {}
				b'.' | b'e' | b'E' => float = true,
				_ => break,
			}
			self.pos += 1;
		}
		let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap();
		if !float {
			if let Ok(v) = text.parse::<i64>() {
				return Ok(Json::Int(v));
			}
		}
		text.parse::<f64>().map(Json::Float).map_err(|_| format!("bad number {text:?} at {start}"))
	}

	fn string(&mut self) -> Result<String, String> {
		self.pos += 1;
		let mut out = String::new();
		loop {
			let Some(&c) = self.bytes.get(self.pos) else { return Err("unclosed string".into()) };
			match c {
				b'"' => {
					self.pos += 1;
					return Ok(out);
				}
				b'\\' => {
					let e = self.bytes[self.pos + 1];
					self.pos += 2;
					match e {
						b'n' => out.push('\n'),
						b't' => out.push('\t'),
						b'r' => out.push('\r'),
						b'b' => out.push('\u{08}'),
						b'f' => out.push('\u{0c}'),
						b'u' => {
							let hex = std::str::from_utf8(&self.bytes[self.pos..self.pos + 4]).unwrap();
							let unit = u16::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
							self.pos += 4;
							if (0xD800..0xDC00).contains(&unit) && self.bytes.get(self.pos) == Some(&b'\\') {
								let hex = std::str::from_utf8(&self.bytes[self.pos + 2..self.pos + 6]).unwrap();
								let low = u16::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
								self.pos += 6;
								out.push_str(&String::from_utf16_lossy(&[unit, low]));
							} else {
								out.push_str(&String::from_utf16_lossy(&[unit]));
							}
						}
						other => out.push(other as char),
					}
				}
				_ => {
					let start = self.pos;
					while self.pos < self.bytes.len() && self.bytes[self.pos] != b'"' && self.bytes[self.pos] != b'\\' {
						self.pos += 1;
					}
					out.push_str(std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|e| e.to_string())?);
				}
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn round_trips_like_python() {
		let mut d = Json::dict();
		d.set("b", 1.0).set("a", vec![1i64, 2]).set("é", "x\"y");
		assert_eq!(d.dumps(false), r#"{"b": 1.0, "a": [1, 2], "\u00e9": "x\"y"}"#);
		assert_eq!(d.dumps(true), r#"{"b":1.0,"a":[1,2],"\u00e9":"x\"y"}"#);
		assert_eq!(d.dumps_sorted(), r#"{"a": [1, 2], "b": 1.0, "\u00e9": "x\"y"}"#);
		assert_eq!(Json::parse(&d.dumps(false)).unwrap(), d);
		assert_eq!(Json::dict().dumps_indent(1), "{}");
		let mut nested = Json::dict();
		nested.set("a", vec![1i64]);
		assert_eq!(nested.dumps_indent(1), "{\n \"a\": [\n  1\n ]\n}");
	}
}
