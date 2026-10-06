//! The PartScript language: tokens, statements and the parser. No geometry.
//!
//! parse() turns text into a Program (props, defs, styles, kits, file variables). Errors are collected on
//! the program, not raised, so one bad line never hides the rest of a file.

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use kitlib::hash::sha1_hex;
use kitlib::json::Json;
use kitlib::py;

use crate::expr::{evaluate, pick_options, py_float, py_isspace, py_strip, resolve_picks};
use crate::ordered::Ordered;
use crate::value::{Env, Value};

// ------------------------------------------------------------------ errors
#[derive(Clone, Debug, PartialEq)]
pub struct PartScriptError {
	pub message: String,
	pub file: String,
	pub line: usize,
}

impl PartScriptError {
	pub fn new(message: impl Into<String>, file: &str, line: usize) -> Self {
		PartScriptError { message: message.into(), file: file.to_string(), line }
	}

	pub fn bare(message: impl Into<String>) -> Self {
		PartScriptError { message: message.into(), file: String::new(), line: 0 }
	}
}

impl fmt::Display for PartScriptError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		if self.file.is_empty() {
			write!(f, "{}", self.message)
		} else {
			write!(f, "{}:{}: {}", self.file, self.line, self.message)
		}
	}
}

impl std::error::Error for PartScriptError {}

// ------------------------------------------------------------------ small text helpers
/// Split at sep outside parentheses.
pub fn split_top(text: &str, sep: char) -> Vec<String> {
	let mut out = Vec::new();
	let mut depth = 0i32;
	let mut current = String::new();
	for ch in text.chars() {
		if ch == '(' {
			depth += 1;
		} else if ch == ')' {
			depth -= 1;
		}
		if ch == sep && depth == 0 {
			out.push(std::mem::take(&mut current));
		} else {
			current.push(ch);
		}
	}
	out.push(current);
	out
}

/// Python's str.split() (runs of whitespace).
pub fn words(text: &str) -> Vec<&str> {
	text.split(py_isspace).filter(|w| !w.is_empty()).collect()
}

/// Python's str.splitlines().
pub fn splitlines(text: &str) -> Vec<&str> {
	let mut out = Vec::new();
	let mut start = 0;
	let mut chars = text.char_indices().peekable();
	while let Some((i, c)) = chars.next() {
		let breaks = matches!(c, '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}');
		if breaks {
			out.push(&text[start..i]);
			let mut end = i + c.len_utf8();
			if c == '\r' {
				if let Some(&(_, '\n')) = chars.peek() {
					chars.next();
					end += 1;
				}
			}
			start = end;
		}
	}
	if start < text.len() {
		out.push(&text[start..]);
	}
	out
}

pub fn strip_comment(line: &str) -> &str {
	let mut quoted = false;
	let chars: Vec<(usize, char)> = line.char_indices().collect();
	for (k, &(index, ch)) in chars.iter().enumerate() {
		if ch == '"' {
			quoted = !quoted;
		} else if ch == '#' && !quoted && (k == 0 || py_isspace(chars[k - 1].1)) && (k + 1 == chars.len() || py_isspace(chars[k + 1].1)) {
			return &line[..index];
		}
	}
	line
}

/// Statements on a line: ';' separates them, and a block opened and closed on one line becomes its
/// opener, its statements and '}'.
pub fn split_statements(line: &str) -> Vec<String> {
	let chars: Vec<char> = line.chars().collect();
	let mut out = Vec::new();
	let mut current = String::new();
	let mut quoted = false;
	for (index, &ch) in chars.iter().enumerate() {
		if ch == '"' {
			quoted = !quoted;
		}
		let before = if index > 0 { chars[index - 1] } else { ' ' };
		let after = chars.get(index + 1).copied().unwrap_or(' ');
		let brace = (ch == '{' || ch == '}') && " \t;".contains(before) && " \t;".contains(after);
		if !quoted && (ch == ';' || brace) {
			if ch == '{' {
				current.push('{');
				out.push(std::mem::take(&mut current));
			} else {
				out.push(std::mem::take(&mut current));
				if ch == '}' {
					out.push("}".to_string());
				}
			}
			current.clear();
		} else {
			current.push(ch);
		}
	}
	out.push(current);
	out.into_iter().map(|s| py_strip(&s).to_string()).filter(|s| !s.is_empty()).collect()
}

/// Whitespace tokens; "quoted strings" stay whole (key="a b" too).
pub fn tokenize(text: &str) -> Result<Vec<String>, String> {
	let mut tokens = Vec::new();
	let mut current = String::new();
	let mut quoted = false;
	for ch in text.chars() {
		if ch == '"' {
			quoted = !quoted;
			current.push(ch);
		} else if py_isspace(ch) && !quoted {
			if !current.is_empty() {
				tokens.push(std::mem::take(&mut current));
			}
		} else {
			current.push(ch);
		}
	}
	if quoted {
		return Err("unclosed quote".into());
	}
	if !current.is_empty() {
		tokens.push(current);
	}
	Ok(tokens)
}

pub fn unquote(token: &str) -> String {
	if token.len() >= 2 && token.starts_with('"') && token.ends_with('"') {
		token[1..token.len() - 1].to_string()
	} else {
		token.to_string()
	}
}

fn is_lower_word(text: &str, first: fn(char) -> bool, rest: fn(char) -> bool) -> bool {
	let mut chars = text.chars();
	matches!(chars.next(), Some(c) if first(c)) && chars.all(rest)
}

/// [a-z_][a-z0-9_]*
pub fn is_snake(text: &str) -> bool {
	is_lower_word(text, |c| c.is_ascii_lowercase() || c == '_', |c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// [a-z][a-z0-9_]*
pub fn is_snake_letter(text: &str) -> bool {
	is_lower_word(text, |c| c.is_ascii_lowercase(), |c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Python's \w for one character.
pub fn is_word_char(c: char) -> bool {
	c == '_' || c.is_alphanumeric()
}

/// [a-z_]\w*
pub fn is_identifier_lower(text: &str) -> bool {
	is_lower_word(text, |c| c.is_ascii_lowercase() || c == '_', is_word_char)
}

// ------------------------------------------------------------------ statements
#[derive(Debug)]
pub struct Stmt {
	pub uid: usize,
	pub op: String,
	pub args: Vec<String>,
	pub opts: Ordered<String>,
	pub mods: Vec<String>,
	pub file: Rc<str>,
	pub line: usize,
	pub block: Option<Vec<Rc<Stmt>>>,
	/// desk = box ...: what the line made, by name, for later lines
	pub name: String,
	/// stool at=...: a part called by its name, as if it were a shape
	pub called: bool,
	/// what its draws are seeded by: its block, its words, which of the same words it is
	pub key: String,
}

thread_local! {
	static NEXT_UID: Cell<usize> = const { Cell::new(1) };
}

pub fn next_uid() -> usize {
	NEXT_UID.with(|n| {
		let v = n.get();
		n.set(v + 1);
		v
	})
}

impl Stmt {
	pub fn new(op: &str, args: Vec<String>, opts: Ordered<String>, mods: Vec<String>, file: &str, line: usize) -> Stmt {
		Stmt {
			uid: next_uid(),
			op: op.to_string(),
			args,
			opts,
			mods,
			file: Rc::from(file),
			line,
			block: None,
			name: String::new(),
			called: false,
			key: String::new(),
		}
	}

	pub fn where_(&self) -> String {
		format!("{}:{}", self.file, self.line)
	}

	/// The line's own part of a random seed: its words, not its line number, so a comment or a line added
	/// elsewhere leaves every other line's draws as they were.
	pub fn seed(&self) -> String {
		if self.key.is_empty() {
			format!("{}:{}", self.file, self.line)
		} else {
			format!("{}:{}", self.file, self.key)
		}
	}

	pub fn opt(&self, key: &str) -> Option<&str> {
		self.opts.get(key).map(String::as_str)
	}
}

#[derive(Debug)]
pub struct Prop {
	pub uid: usize,
	pub name: String,
	pub title: String,
	pub subcategory: String,
	pub opts: Ordered<String>,
	pub body: Vec<Rc<Stmt>>,
	pub file: String,
	pub line: usize,
	/// "prop" or "building"
	pub kind: String,
}

impl Prop {
	pub fn opt(&self, key: &str) -> Option<&str> {
		self.opts.get(key).map(String::as_str)
	}
}

#[derive(Debug)]
pub struct Macro {
	pub name: String,
	pub params: Ordered<String>,
	pub body: Vec<Rc<Stmt>>,
	pub file: String,
	pub line: usize,
}

/// A kit as written: kit NAME [opts], then wall, piece and snap lines.
#[derive(Clone, Debug)]
pub struct KitSpec {
	pub name: String,
	pub opts: Ordered<String>,
	pub walls: Ordered<String>,
	pub pieces: Ordered<String>,
	pub snaps: Ordered<Vec<KitSnap>>,
	pub file: String,
	pub line: usize,
}

#[derive(Clone, Debug)]
pub struct KitSnap {
	pub tokens: Vec<String>,
	pub file: String,
	pub line: usize,
}

/// A dressing bucket: a list of pieces (with a radius), or named pieces.
#[derive(Clone, Debug)]
pub enum Dressing {
	List(Vec<(String, Option<f64>)>),
	Named(Ordered<String>),
}

#[derive(Default)]
pub struct Program {
	pub props: Vec<Rc<Prop>>,
	pub macros: Ordered<Rc<Macro>>,
	pub errors: Vec<PartScriptError>,
	pub files: Vec<String>,
	pub styles: Ordered<Json>,
	pub dressing: Ordered<Dressing>,
	pub style_lines: Ordered<(String, usize)>,
	pub kits: Ordered<KitSpec>,
	pub file_env: Ordered<Ordered<String>>,
	pub namespaces: Ordered<String>,
	pub imports: Vec<(String, usize, String, Option<String>)>,
	pub imported: Vec<String>,
	pub std: Vec<String>,
}

impl Program {
	/// The variables a file's unindented set lines give it.
	pub fn env_of(&self, file: &str) -> Env {
		let mut env = Env::new();
		if let Some(vars) = self.file_env.get(file) {
			for (k, v) in vars.iter() {
				env.insert(k.to_string(), Value::str(v));
			}
		}
		env
	}

	/// A name defined in file, as the program knows it: furniture.table in a file imported as furniture.
	pub fn qualified(&self, name: &str, file: &str) -> String {
		match self.namespaces.get(file) {
			Some(ns) => format!("{ns}.{name}"),
			None => name.to_string(),
		}
	}

	fn candidates(&self, name: &str, file: &str) -> Vec<String> {
		let mut out = Vec::new();
		if let Some(ns) = self.namespaces.get(file) {
			out.push(format!("{ns}.{name}"));
		}
		out.push(name.to_string());
		out
	}

	/// The def a name used in file means: its own namespace's first, then the project's.
	pub fn macro_(&self, name: &str, file: &str) -> Option<Rc<Macro>> {
		self.candidates(name, file).iter().find_map(|c| self.macros.get(c).cloned())
	}

	/// The prop a name used in file means (names(prop): the names it answers to).
	pub fn find_prop(&self, name: &str, file: &str, names: &dyn Fn(&Prop) -> Vec<String>) -> Option<Rc<Prop>> {
		for candidate in self.candidates(name, file) {
			if let Some(p) = self.props.iter().find(|p| p.kind == "prop" && names(p).iter().any(|n| *n == candidate)) {
				return Some(p.clone());
			}
		}
		None
	}

	pub fn kit_name(&self, name: &str, file: &str) -> String {
		self.candidates(name, file).into_iter().find(|c| self.kits.contains(c)).unwrap_or_else(|| name.to_string())
	}

	pub fn is_imported(&self, file: &str) -> bool {
		self.imported.iter().any(|f| f == file)
	}
}

// ------------------------------------------------------------------ copies modifiers (*N@V and friends)
/// The groups of a copies modifier: *COUNTS[@STEP], *COUNT%DEG, *COUNT~AREA, *COUNT^NAME[,GAP].
#[derive(Clone, Debug, PartialEq)]
pub enum Mod {
	Repeat(String, Option<String>),
	Ring(String, String),
	Scatter(String, String),
	On(String, String),
}

/// End positions of a COUNT at pos, in the order the regex tries them.
fn count_ends(t: &[char], pos: usize) -> Vec<usize> {
	let mut out = Vec::new();
	if pos >= t.len() {
		return out;
	}
	if t[pos].is_numeric() {
		let mut end = pos;
		while end < t.len() && t[end].is_numeric() {
			end += 1;
		}
		out.extend((pos + 1..=end).rev());
	} else if t[pos] == '(' {
		for end in pos + 1..t.len() {
			let c = t[end];
			if c == ')' {
				out.push(end + 1);
			}
			if py_isspace(c) || c == '@' || c == '%' {
				break;
			}
		}
	}
	out
}

/// Matches a copies modifier as `_MOD` does.
pub fn parse_mod(token: &str) -> Option<Mod> {
	let t: Vec<char> = token.chars().collect();
	if t.first() != Some(&'*') {
		return None;
	}
	let text = |a: usize, b: usize| t[a..b].iter().collect::<String>();
	// *COUNT(xCOUNT){0,2}(@.+)?$
	fn counts_then(t: &[char], pos: usize, more: usize, out: &mut Option<(usize, Option<usize>)>) {
		for end in count_ends(t, pos) {
			if end == t.len() {
				*out = Some((end, None));
				return;
			}
			if t[end] == '@' && end + 1 < t.len() {
				*out = Some((end, Some(end + 1)));
				return;
			}
			if more > 0 && t[end] == 'x' {
				counts_then(t, end + 1, more - 1, out);
				if out.is_some() {
					return;
				}
			}
		}
	}
	let mut found = None;
	counts_then(&t, 1, 2, &mut found);
	if let Some((end, step)) = found {
		return Some(Mod::Repeat(text(1, end), step.map(|s| text(s, t.len()))));
	}
	for (mark, kind) in [('%', 0), ('~', 1), ('^', 2)] {
		for end in count_ends(&t, 1) {
			if end < t.len() && t[end] == mark && end + 1 < t.len() {
				let (count, rest) = (text(1, end), text(end + 1, t.len()));
				return Some(match kind {
					0 => Mod::Ring(count, rest),
					1 => Mod::Scatter(count, rest),
					_ => Mod::On(count, rest),
				});
			}
		}
	}
	None
}

pub fn is_mod(token: &str) -> bool {
	parse_mod(token).is_some() || matches!(token, "mx" | "my" | "mz")
}

/// The copy counts of a *N / *AxB / *(expr) modifier.
pub fn counts(text: &str, env: &Env) -> Result<Vec<i64>, String> {
	let mut out = Vec::new();
	for item in split_top(text, 'x') {
		let inner = if item.starts_with('(') && item.len() >= 2 { &item[1..item.len() - 1] } else { item.as_str() };
		let value = evaluate(inner, env)?;
		if value < 0.0 || value.is_nan() {
			return Err(format!("copy count {item} = {}", py::repr(value)));
		}
		out.push(py::round(value) as i64);
	}
	Ok(out)
}

// ------------------------------------------------------------------ the readable form
pub const USE_OPTIONS: [&str; 17] =
	["r", "s", "jit", "when", "fade", "wobble", "along", "every", "fit", "closed", "joints", "corners", "index", "twist", "bend", "shrink", "smooth"];
pub const BUILD_OPS: [&str; 7] = ["room", "open", "walls", "stair", "roof", "attach", "place"];
pub const SHAPES: [&str; 41] = [
	"b", "bb", "bx", "c", "cone", "sph", "tube", "wedge", "lathe", "ext", "pipe", "sweep", "face", "pan", "trim", "sign", "use", "at", "set",
	"part", "size", "vault", "archwall", "card", "snap", "chain", "torus", "frame", "link", "join", "row", "terrain", "room", "open", "walls",
	"stair", "roof", "attach", "place", "", "",
];

pub fn is_shape(op: &str) -> bool {
	!op.is_empty() && SHAPES.contains(&op)
}

pub fn alias(word: &str) -> &str {
	match word {
		"box" => "b",
		"bevel" | "bevel_box" => "bb",
		"box_between" => "bx",
		"cyl" | "cylinder" => "c",
		"sphere" => "sph",
		"panel" => "pan",
		"group" => "at",
		"extrude" => "ext",
		"label" => "sign",
		"decal" => "trim",
		"arch_wall" => "archwall",
		other => other,
	}
}

pub fn signature(op: &str) -> Option<&'static [&'static str]> {
	Some(match op {
		"b" => &["at", "size", "mat"],
		"bb" => &["at", "size", "bevel", "mat"],
		"bx" => &["from", "to", "mat"],
		"c" | "cone" => &["at", "radius", "height", "mat"],
		"sph" => &["at", "radius", "mat"],
		"tube" => &["at", "radius", "inner", "height", "mat"],
		"wedge" => &["at", "size", "mat"],
		"pan" => &["at", "width", "height", "mat"],
		"trim" => &["at", "width", "height", "cell"],
		"sign" => &["at", "width", "height", "text"],
		"ext" => &["at", "width", "mat"],
		"lathe" => &["at", "mat"],
		"pipe" => &["mat", "radius"],
		"sweep" | "face" => &["mat"],
		"vault" => &["at", "width", "depth", "rise", "mat"],
		"archwall" => &["at", "width", "height", "thick", "mat"],
		"use" => &["name", "at"],
		"at" => &["at"],
		"chain" => &[],
		"torus" => &["at", "radius", "thick", "mat"],
		"frame" => &["at", "width", "height", "depth", "mat"],
		"link" => &["kind", "at", "toward"],
		"join" => &["kind"],
		"row" => &["axis"],
		"terrain" => &["at", "size", "mat"],
		_ => return None,
	})
}

const POINTS: [&str; 5] = ["ext", "lathe", "pipe", "sweep", "face"];
const SIDED_OPS: [&str; 11] = ["c", "cone", "sph", "tube", "lathe", "pipe", "sweep", "vault", "archwall", "torus", "join"];
/// Shapes whose s= is a side count, not a scale.
pub const SIDED: [&str; 10] = ["c", "cone", "sph", "tube", "lathe", "pipe", "sweep", "vault", "archwall", "torus"];
const WORDS: [&str; 5] = ["repeat", "grid", "ring", "scatter", "mirror"];
pub const TOP: [&str; 9] = ["prop", "def", "kit", "building", "style", "dressing", "import", "end", "theme"];
const RESERVED: [&str; 16] = ["i", "pi", "tau", "on", "w", "d", "h", "level", "length", "rand", "pick", "here", "noise", "rough", "x", "y"];
/// Shapes on= sits on (their at= is a centre).
pub const CENTRED: [&str; 9] = ["b", "bb", "c", "cone", "sph", "tube", "wedge", "torus", "frame"];

fn option_name(key: &str) -> Option<&'static str> {
	match key {
		"turn" | "rotate" => Some("r"),
		"top_radius" => Some("rt"),
		"axis" => Some("ax"),
		"jitter" => Some("jit"),
		"cap_mat" => Some("capm"),
		"material" => Some("mat"),
		_ => None,
	}
}

fn count_word(text: &str) -> String {
	if (!text.is_empty() && text.chars().all(char::is_numeric)) || (text.starts_with('(') && text.ends_with(')')) {
		text.to_string()
	} else {
		format!("({text})")
	}
}

fn modifier_words(tokens: &[String], file: &str, line: usize) -> Result<Vec<String>, PartScriptError> {
	let mut out = Vec::new();
	let mut k = 0;
	while k < tokens.len() {
		let word = tokens[k].as_str();
		if word == "as" && k + 1 < tokens.len() && {
			let names: Vec<&str> = tokens[k + 1].split(',').collect();
			(1..=3).contains(&names.len()) && names.iter().all(|n| is_identifier_lower(n))
		} {
			out.push(format!("index={}", tokens[k + 1]));
			k += 2;
			continue;
		}
		if !WORDS.contains(&word) || k + 1 >= tokens.len() {
			out.push(word.to_string());
			k += 1;
			continue;
		}
		let rest = &tokens[k + 1..];
		match word {
			"mirror" => {
				let axes = &rest[0];
				if !((1..=3).contains(&axes.len()) && axes.chars().all(|c| "xyz".contains(c))) {
					return Err(PartScriptError::new("mirror x, mirror y, mirror z or mirror xy...", file, line));
				}
				out.extend(axes.chars().map(|a| format!("m{a}")));
				k += 2;
			}
			"repeat" | "grid" => {
				let count = &rest[0];
				let items = split_top(count, 'x');
				let counts = if (2..=3).contains(&items.len()) && items.iter().all(|c| !c.is_empty()) {
					items.iter().map(|c| count_word(c)).collect::<Vec<_>>().join("x")
				} else {
					count_word(count)
				};
				if rest.len() > 2 && rest[1] == "every" {
					out.push(format!("*{counts}@{}", rest[2]));
					k += 4;
				} else {
					out.push(format!("*{counts}"));
					k += 2;
				}
			}
			"ring" => {
				let count = count_word(&rest[0]);
				if rest.len() > 2 && rest[1] == "step" {
					out.push(format!("*{count}%{}", rest[2]));
					k += 4;
				} else {
					out.push(format!("*{count}%(360/{count})"));
					k += 2;
				}
			}
			_ if rest.len() > 2 && rest[1] == "on" => {
				let (count, target) = (count_word(&rest[0]), rest[2].clone());
				let mut gap = String::new();
				k += 4;
				while k + 1 < tokens.len() && (tokens[k] == "facing" || tokens[k] == "apart") {
					if tokens[k] == "facing" {
						out.push(format!("facing={}", tokens[k + 1]));
					} else {
						gap = tokens[k + 1].clone();
					}
					k += 2;
				}
				out.push(format!("*{count}^{target}{}", if gap.is_empty() { String::new() } else { format!(",{gap}") }));
			}
			_ => {
				if rest.len() < 3 || (rest[1] != "over" && rest[1] != "within") {
					return Err(PartScriptError::new(
						"scatter N over W,D [apart G], scatter N within R [apart G] or scatter N on NAME [facing up]",
						file,
						line,
					));
				}
				let apart = if rest.len() > 4 && rest[3] == "apart" { rest[4].clone() } else { String::new() };
				let area = if rest[1] == "over" { rest[2].clone() } else if !apart.is_empty() { format!("{},0", rest[2]) } else { rest[2].clone() };
				out.push(format!("*{}~{area}{}", count_word(&rest[0]), if apart.is_empty() { String::new() } else { format!(",{apart}") }));
				k += if apart.is_empty() { 4 } else { 6 };
			}
		}
	}
	Ok(out)
}

/// on / on(z) components of a position as ~ / ~z.
fn on_marks(token: &str) -> String {
	if !token.contains("on") || token.starts_with('"') {
		return token.to_string();
	}
	split_top(token, ',')
		.into_iter()
		.map(|part| {
			if part == "on" {
				"~".to_string()
			} else if part.starts_with("on(") && part.ends_with(')') {
				format!("~{}", &part[3..part.len() - 1])
			} else {
				part
			}
		})
		.collect::<Vec<_>>()
		.join(",")
}

fn is_key_token(token: &str) -> Option<usize> {
	// ^[A-Za-z_]\w*=
	let mut chars = token.char_indices();
	let (_, first) = chars.next()?;
	if !(first.is_ascii_alphabetic() || first == '_') {
		return None;
	}
	for (i, c) in chars {
		if c == '=' {
			return Some(i);
		}
		if !is_word_char(c) {
			return None;
		}
	}
	None
}

pub fn parse_statement(text: &str, file: &str, line: usize) -> Result<Stmt, PartScriptError> {
	if let Some(rest) = text.strip_prefix("if ") {
		let mut opts = Ordered::new();
		opts.insert("when", py_strip(rest).to_string());
		return Ok(Stmt::new("at", vec![], opts, vec![], file, line));
	}
	let mut tokens = tokenize(text).map_err(|e| PartScriptError::new(e, file, line))?;
	let mut name = String::new();
	if tokens.len() > 2 && tokens[1] == "=" {
		name = tokens[0].clone();
		tokens.drain(..2);
		if !is_snake(&name) || RESERVED.contains(&name.as_str()) {
			let mut reserved: Vec<&str> = RESERVED.to_vec();
			reserved.sort();
			return Err(PartScriptError::new(
				format!("{} can't name a shape: a lower_snake_case word that is not {}", py::repr_str(&name), reserved.join(", ")),
				file,
				line,
			));
		}
	}
	if tokens[0] == "stack" {
		tokens.splice(0..1, ["row".to_string(), "z".to_string()]);
	}
	let mut called = false;
	let first_ok = {
		let t = &tokens[0];
		t.split('.').all(is_snake) && !t.is_empty()
	};
	if !is_shape(alias(&tokens[0])) && !TOP.contains(&tokens[0].as_str()) && first_ok {
		tokens.insert(0, "use".to_string());
		called = true;
	}
	let op = alias(&tokens[0]).to_string();
	let (mut args, mut opts, mut mods) = (Vec::new(), Ordered::new(), Vec::new());
	let words = if signature(&op).is_some() { modifier_words(&tokens[1..], file, line)? } else { tokens[1..].to_vec() };
	for token in words {
		if is_mod(&token) {
			mods.push(token);
		} else if token.contains('=') && !token.starts_with('"') && is_key_token(&token).is_some() {
			let at = token.find('=').unwrap();
			let (key, value) = (&token[..at], &token[at + 1..]);
			if opts.contains(key) {
				return Err(PartScriptError::new(format!("{key}= given twice"), file, line));
			}
			opts.insert(key, unquote(value));
		} else {
			args.push(token);
		}
	}
	if tokens[0] == "label" && !opts.contains("printed") {
		opts.insert("printed", "1".to_string());
	}
	if let Some(loose) = args.iter().find(|t| matches!(t.as_str(), "and" | "or" | "not")) {
		return Err(PartScriptError::new(
			format!("'{loose}' on its own: an expression with spaces goes in quotes (when=\"i<3 or i>7\")"),
			file,
			line,
		));
	}
	if signature(&op).is_some() {
		let (a, o) = named(&op, args, opts, file, line)?;
		args = a;
		opts = o;
	}
	let mut stmt = Stmt::new(&op, args, opts, mods, file, line);
	stmt.name = name;
	stmt.called = called;
	Ok(stmt)
}

/// Named arguments moved into their places, long option names to short, on to ~.
fn named(op: &str, args: Vec<String>, opts: Ordered<String>, file: &str, line: usize) -> Result<(Vec<String>, Ordered<String>), PartScriptError> {
	let mut renamed: Ordered<String> = Ordered::new();
	let mut given: Ordered<String> = Ordered::new();
	for (key, value) in opts.iter() {
		let mut short = option_name(key).map(str::to_string);
		if key == "sides" && op == "sign" {
			short = Some("sides".into());
		} else if key == "sides" || key == "scale" {
			if (key == "sides" && !SIDED_OPS.contains(&op)) || (key == "scale" && op != "use" && op != "at") {
				return Err(PartScriptError::new(
					format!("{op}: no {key}= ({})", if key == "scale" { "scale= is for use and group" } else { "sides= is for round shapes" }),
					file,
					line,
				));
			}
			short = Some("s".into());
		}
		let short = short.unwrap_or_else(|| key.to_string());
		if renamed.contains(&short) {
			return Err(PartScriptError::new(format!("{}= and {key}= are the same option; give one", given.get(&short).unwrap()), file, line));
		}
		renamed.insert(&short, value.clone());
		given.insert(&short, key.to_string());
	}
	let mut opts = renamed;
	let mut args = args;
	let side = opts.get("on").map(|on| on.split_once('.').map(|(_, s)| s.to_string()).unwrap_or_default()).unwrap_or_default();
	let linking = matches!(op, "join" | "link" | "chain");
	if opts.contains("on") && !linking && (side.is_empty() || side == "top") {
		match opts.get("at").cloned() {
			None if args.is_empty() => opts.insert("at", if CENTRED.contains(&op) { "0,0,on" } else { "0,0,0" }.to_string()),
			Some(at) if split_top(&at, ',').len() == 2 => opts.insert("at", format!("{at}{}", if CENTRED.contains(&op) { ",on" } else { ",0" })),
			_ => {}
		}
	} else if opts.contains("on") && !linking {
		match opts.get("at").cloned() {
			None if args.is_empty() => opts.insert("at", "0,0,0".to_string()),
			Some(at) if split_top(&at, ',').len() == 2 => {
				let parts = split_top(&at, ',');
				let (a, b) = (&parts[0], &parts[1]);
				let placed = match side.as_str() {
					"front" | "back" => format!("{a},0,{b}"),
					"left" | "right" => format!("0,{a},{b}"),
					"bottom" => format!("{a},{b},0"),
					_ => at.clone(),
				};
				opts.insert("at", placed);
			}
			_ => {}
		}
	}
	if CENTRED.contains(&op) && !opts.contains("at") && args.is_empty() && !opts.contains("from") {
		opts.insert("at", "0,0,on".to_string());
	}
	let sig = signature(op).unwrap();
	if sig.first() == Some(&"at") && !matches!(op, "use" | "at" | "row" | "link") && !opts.contains("at")
		&& (args.is_empty() || (POINTS.contains(&op) && args[0].contains(':')))
		&& sig[1..].iter().any(|k| opts.contains(k))
	{
		opts.insert("at", "0,0,0".to_string());
	}
	if opts.contains("from") && opts.contains("to") && matches!(op, "b" | "bb" | "c" | "cone") && args.is_empty() {
		if !opts.contains("at") {
			opts.insert("at", "0,0,0".to_string());
		}
		if op == "c" || op == "cone" {
			if !opts.contains("height") {
				opts.insert("height", "0".to_string());
			}
		} else if let Some(size) = opts.get("size").cloned() {
			match split_top(&size, ',').len() {
				2 => opts.insert("size", format!("0,{size}")),
				1 => opts.insert("size", format!("0,{size},{size}")),
				_ => {}
			}
		}
	}
	let named: Vec<&str> = sig.iter().copied().filter(|k| opts.contains(k)).collect();
	if let Some(first) = named.first() {
		let first_index = sig.iter().position(|k| k == first).unwrap();
		if args.len() > first_index && !POINTS.contains(&op) {
			return Err(PartScriptError::new(format!("{first}= is also given by position; use one or the other"), file, line));
		}
		let mut positional: Vec<String> = args.iter().take(first_index).cloned().collect();
		let points: Vec<String> = if POINTS.contains(&op) { args[positional.len()..].to_vec() } else { Vec::new() };
		for key in &sig[positional.len()..] {
			if !opts.contains(key) {
				if matches!(op, "use" | "at") && *key == "at" {
					break;
				}
				return Err(PartScriptError::new(
					format!("{op}: {key}= is missing (it takes {})", sig.iter().map(|k| format!("{k}=")).collect::<Vec<_>>().join(" ")),
					file,
					line,
				));
			}
			let value = opts.remove(key).unwrap();
			positional.push(if *key == "text" { format!("\"{value}\"") } else { value });
		}
		positional.extend(points);
		args = positional;
	}
	if let Some(points) = opts.remove("points") {
		args.extend(words(&points).into_iter().map(str::to_string));
	}
	let args = args.iter().map(|a| on_marks(a)).collect();
	let opts = opts.iter().map(|(k, v)| (k.to_string(), if k == "at" { on_marks(v) } else { v.clone() })).collect();
	Ok((args, opts))
}

/// Lines ending in a backslash carry on into the next: the joined line keeps the first's number and the
/// lines it swallowed become None (so later line numbers do not move).
pub fn join_continued(lines: &[&str]) -> Vec<Option<String>> {
	let mut out: Vec<Option<String>> = Vec::new();
	let mut carry: Option<usize> = None;
	for line in lines {
		let code = strip_comment(line).trim_end_matches(py_isspace);
		if let Some(c) = carry {
			let previous = out[c].clone().unwrap();
			let head = previous[..previous.len() - 1].trim_end_matches(py_isspace);
			out[c] = Some(format!("{head} {}", py_strip(code)));
			out.push(None);
			if !code.ends_with('\\') {
				carry = None;
			}
			continue;
		}
		out.push(Some(if code.ends_with('\\') { code.to_string() } else { line.to_string() }));
		if code.ends_with('\\') {
			carry = Some(out.len() - 1);
		}
	}
	out
}

fn first_word(statement: &str) -> &str {
	words(statement).first().copied().unwrap_or("")
}

/// Props, defs and file variables from PartScript text. Errors are collected, not raised.
pub fn parse(text: &str, file: &str, program: &mut Program) {
	program.files.push(file.to_string());
	let mut raw: Vec<(usize, String)> = Vec::new();
	let mut top_sets: Vec<usize> = Vec::new();
	for (number, line) in join_continued(&splitlines(text)).into_iter().enumerate() {
		let number = number + 1;
		let Some(line) = line else { continue };
		if !line.starts_with(' ') && !line.starts_with('\t') && line.starts_with("set ") {
			top_sets.push(number);
		}
		let line = py_strip(strip_comment(&line));
		if line.is_empty() {
			continue;
		}
		for statement in split_statements(line) {
			raw.push((number, statement));
		}
	}
	let mut index = 0;
	while index < raw.len() {
		let (number, statement) = raw[index].clone();
		let head = first_word(&statement).to_string();
		let block_end = if matches!(head.as_str(), "prop" | "building" | "kit" | "def" | "style") { block_end_of(&raw, index + 1, &top_sets) } else { index + 1 };
		let result: Result<Option<usize>, PartScriptError> = (|| {
			match head.as_str() {
				"prop" | "building" => {
					let end = block_end_of(&raw, index + 1, &top_sets);
					let mut block: Vec<(usize, String)> = raw[index..end].to_vec();
					if head == "building" {
						block[0].1 = format!("prop{}", &block[0].1["building".len()..]);
					}
					let before = program.props.len();
					parse_prop(&block, file, program)?;
					for built in program.props[before..].iter_mut() {
						let p = Rc::get_mut(built).expect("a new prop");
						p.kind = if head == "building" { "building".into() } else { "prop".into() };
					}
					if head == "building" {
						let missing: Vec<String> = program.props[before..].iter().filter(|p| p.opt("kit").unwrap_or("").is_empty()).map(|p| p.name.clone()).collect();
						for name in missing {
							program.errors.push(PartScriptError::new(format!("building {name}: name its set of pieces with kit=NAME"), file, number));
						}
					}
					Ok(Some(end))
				}
				"kit" => {
					let end = block_end_of(&raw, index + 1, &top_sets);
					crate::building::parse_kit(&raw[index..end], file, program)?;
					Ok(Some(end))
				}
				"def" => {
					let end = block_end_of(&raw, index + 1, &top_sets);
					parse_macro(&raw[index..end], file, program)?;
					Ok(Some(end))
				}
				"style" => {
					let end = block_end_of(&raw, index + 1, &top_sets);
					parse_style(&raw[index..end], file, program)?;
					Ok(Some(end))
				}
				"dressing" => {
					let tokens = tokenize(&statement).map_err(|e| PartScriptError::new(e, file, number))?;
					if tokens.len() < 3 {
						return Err(PartScriptError::new("dressing BUCKET piece[:radius] ...", file, number));
					}
					for token in &tokens[2..] {
						if let Some((key, piece)) = token.split_once('=') {
							let bucket = program.dressing.entry_or(&tokens[1], Dressing::Named(Ordered::new()));
							match bucket {
								Dressing::Named(named) => named.insert(key, piece.to_string()),
								Dressing::List(_) => {
									return Err(PartScriptError::new(format!("dressing {}: mix of list and key=piece entries", tokens[1]), file, number))
								}
							}
							continue;
						}
						let bucket = program.dressing.entry_or(&tokens[1], Dressing::List(Vec::new()));
						let Dressing::List(items) = bucket else {
							return Err(PartScriptError::new(format!("dressing {}: mix of list and key=piece entries", tokens[1]), file, number));
						};
						let (name, radius) = match token.split_once(':') {
							Some((n, r)) => (n.to_string(), Some(py_float(r).ok_or_else(|| PartScriptError::new(format!("could not convert string to float: {}", py::repr_str(r)), file, number))?)),
							None => (token.clone(), None),
						};
						items.push((name, radius));
					}
					Ok(None)
				}
				"set" => {
					let stmt = parse_statement(&statement, file, number)?;
					let vars = program.file_env.entry_or(file, Ordered::new());
					for (k, v) in stmt.opts.iter() {
						vars.insert(k, v.clone());
					}
					Ok(None)
				}
				"import" => {
					let tokens = tokenize(&statement).map_err(|e| PartScriptError::new(e, file, number))?;
					let alias = if tokens.len() == 4 && tokens[2] == "as" { Some(tokens[3].clone()) } else { None };
					if !(tokens.len() == 2 || tokens.len() == 4) || !tokens[1].starts_with('"') || (tokens.len() == 4 && alias.is_none()) {
						return Err(PartScriptError::new("import \"path/to/file.parts\" [as NAME] (a file or a folder, from this file's folder)", file, number));
					}
					if let Some(a) = &alias {
						if !is_snake_letter(a) {
							return Err(PartScriptError::new(format!("import ... as {a}: a lower_snake_case name"), file, number));
						}
					}
					program.imports.push((file.to_string(), number, unquote(&tokens[1]), alias));
					Ok(None)
				}
				"end" | "theme" => Ok(None),
				_ => Err(PartScriptError::new(format!("'{head}' outside a prop, def, kit or building (start one with 'prop NAME')"), file, number)),
			}
		})();
		match result {
			Ok(Some(end)) => index = end,
			Ok(None) => index += 1,
			Err(error) => {
				program.errors.push(error);
				index = block_end;
			}
		}
	}
}

fn block_end_of(raw: &[(usize, String)], start: usize, top_sets: &[usize]) -> usize {
	let mut index = start;
	while index < raw.len() {
		let head = first_word(&raw[index].1);
		if matches!(head, "prop" | "def" | "style" | "dressing" | "kit" | "building") || (head == "set" && top_sets.contains(&raw[index].0)) {
			return index;
		}
		if head == "end" {
			return index + 1;
		}
		index += 1;
	}
	index
}

/// Statements with { } blocks nested. A bad statement is reported (to errors) and skipped.
pub fn body(raw: &[(usize, String)], file: &str, errors: &mut Vec<PartScriptError>, scope: &str) -> Result<Vec<Rc<Stmt>>, PartScriptError> {
	let mut stack: Vec<Vec<Rc<Stmt>>> = vec![Vec::new()];
	let mut openers: Vec<Stmt> = Vec::new();
	let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
	for (number, statement) in raw {
		if statement == "end" {
			continue;
		}
		let joined = words(statement).join(" ");
		let count = seen.entry(joined.clone()).or_insert(0);
		*count += 1;
		let key = format!("{scope}#{}#{count}", &sha1_hex(joined.as_bytes())[..12]);
		if let Err(error) = body_line(statement, *number, file, &mut stack, &mut openers, &key) {
			errors.push(error);
		}
	}
	if let Some(open) = openers.last() {
		return Err(PartScriptError::new("unclosed block (add a '}' line)", file, open.line));
	}
	Ok(stack.pop().unwrap())
}

fn body_line(statement: &str, number: usize, file: &str, stack: &mut Vec<Vec<Rc<Stmt>>>, openers: &mut Vec<Stmt>, key: &str) -> Result<(), PartScriptError> {
	if statement == "}" {
		let Some(mut opener) = openers.pop() else {
			return Err(PartScriptError::new("'}' without a block to close", file, number));
		};
		opener.block = Some(stack.pop().unwrap());
		stack.last_mut().unwrap().push(Rc::new(opener));
		return Ok(());
	}
	let opens = statement.ends_with('{');
	let text = if opens { py_strip(&statement[..statement.len() - 1]) } else { statement };
	let mut stmt = parse_statement(text, file, number)?;
	stmt.key = key.to_string();
	if !is_shape(&stmt.op) {
		let mut shapes: Vec<&str> = SHAPES.iter().copied().filter(|s| !s.is_empty() && !matches!(*s, "set" | "part" | "size")).collect();
		shapes.sort();
		return Err(PartScriptError::new(format!("unknown statement '{}' (shapes: {})", stmt.op, shapes.join(", ")), file, number));
	}
	if opens {
		if stmt.op != "at" && stmt.op != "row" {
			return Err(PartScriptError::new("only group, if, row and stack open a { block }", file, number));
		}
		openers.push(stmt);
		stack.push(Vec::new());
	} else {
		stack.last_mut().unwrap().push(Rc::new(stmt));
	}
	Ok(())
}

fn parse_prop(raw: &[(usize, String)], file: &str, program: &mut Program) -> Result<(), PartScriptError> {
	let (number, header) = &raw[0];
	let head = tokenize(header).map_err(|e| PartScriptError::new(e, file, *number))?;
	let mut fors: Vec<(String, Vec<String>)> = Vec::new();
	let mut keep: Vec<String> = Vec::new();
	let mut i = 1;
	while i < head.len() {
		if head[i] == "for" && i + 1 < head.len() && head[i + 1].contains('=') {
			let (key, values) = head[i + 1].split_once('=').unwrap();
			fors.push((key.to_string(), values.split(',').filter(|v| !v.is_empty()).map(str::to_string).collect()));
			i += 2;
		} else {
			keep.push(head[i].clone());
			i += 1;
		}
	}
	if !fors.is_empty() {
		let mut combos: Vec<Vec<(String, String)>> = vec![vec![]];
		for (key, values) in &fors {
			let mut next = Vec::new();
			for combo in &combos {
				for value in values {
					let mut c = combo.clone();
					c.retain(|(k, _)| k != key);
					c.push((key.clone(), value.clone()));
					next.push(c);
				}
			}
			combos = next;
		}
		for combo in combos {
			let sub = |text: &str| -> String {
				let mut out = text.to_string();
				for (key, value) in &combo {
					out = out.replace(&format!("{{{key}}}"), value);
				}
				out
			};
			let mut expanded = vec![(*number, sub(&format!("prop {}", keep.join(" "))))];
			expanded.extend(raw[1..].iter().map(|(n, s)| (*n, sub(s))));
			parse_prop(&expanded, file, program)?;
		}
		return Ok(());
	}
	if keep.is_empty() {
		return Err(PartScriptError::new("prop needs a name: prop NAME \"Title\" [subcategory]", file, *number));
	}
	let name = keep[0].clone();
	let ok = name.len() >= 2 && name.len() <= 61 && is_snake_letter(&name);
	if !ok {
		return Err(PartScriptError::new(format!("prop name {}: lower_snake_case (a {{v}} in it needs a 'for v=...')", py::repr_str(&name)), file, *number));
	}
	let (mut title, mut subcategory, mut opts) = (String::new(), "props".to_string(), Ordered::new());
	for token in &keep[1..] {
		if token.starts_with('"') {
			title = unquote(token);
		} else if let Some((key, value)) = token.split_once('=') {
			opts.insert(key, unquote(value));
		} else {
			subcategory = token.clone();
		}
	}
	let mut errors = Vec::new();
	let body = body(&raw[1..], file, &mut errors, &name);
	program.errors.extend(errors);
	let body = body?;
	let title = if title.is_empty() { py::title(&name.replace('_', " ")) } else { title };
	program.props.push(Rc::new(Prop {
		uid: next_uid(),
		name: program.qualified(&name, file),
		title,
		subcategory,
		opts,
		body,
		file: file.to_string(),
		line: *number,
		kind: "prop".into(),
	}));
	Ok(())
}

pub const STYLE_LAYERS: [&str; 8] = ["wall", "corner", "center", "surface", "decor", "small", "debris", "decals"];
const STYLE_BOOLS: [&str; 8] = ["hero", "once", "surface", "stack", "double", "grid", "things_on_top", "exterior"];

fn style_value(key: &str, value: &str) -> Json {
	if STYLE_BOOLS.contains(&key) {
		return Json::Bool(matches!(value.to_lowercase().as_str(), "1" | "true" | "yes"));
	}
	let one = |v: &str| -> Json {
		match py_float(v) {
			Some(f) if f.fract() == 0.0 && f.is_finite() => Json::Int(f as i64),
			Some(f) => Json::Float(f),
			None => Json::Str(v.to_string()),
		}
	};
	let items: Vec<&str> = value.split(',').collect();
	if items.len() > 1 {
		Json::List(items.into_iter().map(one).collect())
	} else {
		one(value)
	}
}

fn parse_style(raw: &[(usize, String)], file: &str, program: &mut Program) -> Result<(), PartScriptError> {
	let (number, header) = &raw[0];
	let head = tokenize(header).map_err(|e| PartScriptError::new(e, file, *number))?;
	if head.len() < 2 || !is_snake_letter(&head[1]) {
		return Err(PartScriptError::new("style needs a lower_snake_case name: style NAME [exterior=1]", file, *number));
	}
	let mut style = Json::dict();
	for token in &head[2..] {
		let (key, value) = token.split_once('=').unwrap_or((token, ""));
		style.set(key, style_value(key, value));
	}
	for (line_number, statement) in &raw[1..] {
		if statement == "end" {
			continue;
		}
		let tokens = tokenize(statement).map_err(|e| PartScriptError::new(e, file, *line_number))?;
		let layer = tokens[0].as_str();
		if !STYLE_LAYERS.contains(&layer) {
			return Err(PartScriptError::new(format!("style layer {}: one of {}", py::repr_str(layer), STYLE_LAYERS.join(", ")), file, *line_number));
		}
		if layer == "decals" {
			if style.get("decals").is_none() {
				style.set("decals", Json::List(vec![]));
			}
			if let Some(Json::List(items)) = style.get_mut("decals") {
				items.extend(tokens[1..].iter().map(|t| Json::Str(t.clone())));
			}
			continue;
		}
		if tokens.len() < 2 {
			return Err(PartScriptError::new(format!("{layer} needs a piece: {layer} PIECE [repeat=1,3 hero=1 front=PIECE ...]"), file, *line_number));
		}
		let mut entry = Json::dict();
		entry.set("piece", tokens[1].as_str());
		for token in &tokens[2..] {
			let (key, value) = token.split_once('=').unwrap_or((token, ""));
			entry.set(key, if value.is_empty() { Json::Bool(true) } else { style_value(key, value) });
		}
		let item = if entry.as_dict().len() > 1 { entry } else { Json::Str(tokens[1].clone()) };
		if style.get(layer).is_none() {
			style.set(layer, Json::List(vec![]));
		}
		if let Some(Json::List(items)) = style.get_mut(layer) {
			items.push(item);
		}
	}
	program.styles.insert(&head[1], style);
	program.style_lines.insert(&head[1], (file.to_string(), *number));
	Ok(())
}

fn parse_macro(raw: &[(usize, String)], file: &str, program: &mut Program) -> Result<(), PartScriptError> {
	let (number, header) = &raw[0];
	let head = tokenize(header).map_err(|e| PartScriptError::new(e, file, *number))?;
	if head.len() < 2 || !is_snake_letter(&head[1]) {
		return Err(PartScriptError::new("def needs a lower_snake_case name: def NAME [param=default ...]", file, *number));
	}
	let mut params = Ordered::new();
	for token in &head[2..] {
		let Some((key, value)) = token.split_once('=') else {
			return Err(PartScriptError::new(format!("def parameters need defaults: {token}=..."), file, *number));
		};
		if USE_OPTIONS.contains(&key) {
			return Err(PartScriptError::new(
				format!(
					"def parameter {} is taken by use ({} place the part); call it something else (rad for a radius)",
					py::repr_str(key),
					USE_OPTIONS.join(", ")
				),
				file,
				*number,
			));
		}
		params.insert(key, unquote(value));
	}
	let name = program.qualified(&head[1], file);
	if let Some(before) = program.macros.get(&name) {
		if before.file != "std.parts" {
			return Err(PartScriptError::new(format!("def {} defined twice (also {}:{})", head[1], before.file, before.line), file, *number));
		}
	}
	let mut errors = Vec::new();
	let body = body(&raw[1..], file, &mut errors, &name);
	program.errors.extend(errors);
	let body = body?;
	program.macros.insert(&name, Rc::new(Macro { name: name.clone(), params, body, file: file.to_string(), line: *number }));
	Ok(())
}

// ------------------------------------------------------------------ materials
pub const FINISHES: [&str; 11] = ["paint", "metal", "plastic", "rubber", "fabric", "wood", "plaster", "concrete", "stone", "glow", "glass"];

/// (material key, hex, finish) for a #rrggbb[/finish] token; Err for an unknown finish.
pub fn colour_key(prefix: &str, token: &str) -> Result<Option<(String, String, String)>, String> {
	let Some(rest) = token.strip_prefix('#') else { return Ok(None) };
	let (hex, finish) = match rest.split_once('/') {
		Some((h, f)) => (h, Some(f)),
		None => (rest, None),
	};
	if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
		return Ok(None);
	}
	if let Some(f) = finish {
		if f.is_empty() || !f.chars().all(is_word_char) {
			return Ok(None);
		}
	}
	let hex = hex.to_lowercase();
	let finish = finish.unwrap_or("paint").to_lowercase();
	if !FINISHES.contains(&finish.as_str()) {
		return Err(format!("colour finish {}: one of {}", py::repr_str(&finish), FINISHES.join(", ")));
	}
	let prefix = if prefix.is_empty() { String::new() } else { format!("{prefix}_") };
	Ok(Some((format!("{prefix}x{hex}_{finish}"), hex, finish)))
}

/// The Mat a colour material gets for its finish.
pub fn colour_mat(key: &str, hex: &str, finish: &str) -> kitlib::geom::Mat {
	let mut mat = kitlib::geom::Mat::new(key, 2.0);
	let rgb: Vec<f64> = (0..3).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap() as f64 / 255.0).collect();
	match finish {
		"glow" => {
			mat.emission_color = [rgb[0], rgb[1], rgb[2]];
			mat.emission_strength = 1.25;
			mat.ao = false;
		}
		"glass" => {
			mat.roughness = 0.3;
			mat.alpha = 0.4;
			mat.ao = false;
		}
		"metal" => mat.roughness = 0.9,
		_ => {}
	}
	mat
}

/// A material key for a sign's printed texture: sign_<digest of its spec>.
pub fn sign_key(prefix: &str, spec: &Json) -> String {
	let digest = sha1_hex(spec.dumps_sorted().as_bytes());
	let prefix = if prefix.is_empty() { String::new() } else { format!("{prefix}_") };
	format!("{prefix}sign_{}", &digest[..8])
}

/// A shape's point tokens with any variable that holds a line of points spelled out.
pub fn expand_points(tokens: &[String], env: &Env) -> Vec<String> {
	let mut out = Vec::new();
	for token in tokens {
		match env.get(token.as_str()) {
			Some(Value::Str(value)) if words(value).len() > 1 => out.extend(words(value).into_iter().map(str::to_string)),
			_ => out.push(token.clone()),
		}
	}
	out
}

/// A material token's cycle (a|b|c), with parameters looked up (a parameter may hold a cycle).
pub fn material_items(token: &str, env: &Env) -> Vec<String> {
	let mut out = Vec::new();
	for item in token.split('|') {
		let value = if item.starts_with('#') { item.to_string() } else { env.get(item).map(Value::text).unwrap_or_else(|| item.to_string()) };
		for picked in resolve_picks(&value, env).split('|') {
			let held = if picked.starts_with('#') { None } else { env.get(picked) };
			match held {
				Some(Value::Str(s)) => out.extend(resolve_picks(s, env).split('|').map(str::to_string)),
				_ => out.push(picked.to_string()),
			}
		}
	}
	out
}

/// Every material a token can give: each item of its cycle, parameters looked up, each pick() option.
pub fn every_material(token: &str, env: &Env) -> Vec<String> {
	let mut out = Vec::new();
	for item in token.split('|') {
		let value = if item.starts_with('#') { item.to_string() } else { env.get(item).map(Value::text).unwrap_or_else(|| item.to_string()) };
		for option in pick_options(&value) {
			for picked in option.split('|') {
				let held = if picked.starts_with('#') { None } else { env.get(picked) };
				match held {
					Some(Value::Str(s)) => {
						for found in pick_options(s) {
							out.extend(found.split('|').map(str::to_string));
						}
					}
					_ => out.push(picked.to_string()),
				}
			}
		}
	}
	out
}

/// A use/def argument: a number when it evaluates, else the text (a material, a word) or what it names.
pub fn arg_value(value: &str, env: &Env) -> Value {
	match evaluate(value, env) {
		Ok(v) => Value::Num(v),
		Err(_) => env.get(value).cloned().unwrap_or_else(|| Value::str(value)),
	}
}
