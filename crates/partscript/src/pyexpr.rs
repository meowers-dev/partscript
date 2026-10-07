//! Python 3.13's expression syntax, read exactly as `ast.parse(text, mode="eval")` reads it: the tokens
//! (numbers, strings and f-strings, names normalised as Python normalises them, line joining inside
//! brackets and after a backslash, comments), the grammar of every expression Python has, and the tree,
//! with `ast.dump`'s text for it.
//!
//! PartScript began in Python and evaluated its expressions over Python's own syntax tree, so what is a
//! syntax error, what parses but is not arithmetic, and how that is reported all follow from Python's
//! grammar. Only the arithmetic subset is evaluated (expr.rs); the rest exists to be rejected the same way.
//!
//! Not reproduced: `\N{name}` escapes in string literals, which need Unicode's name table. A string
//! holding one is reported as a syntax error.

use kitlib::py;

// ------------------------------------------------------------------ tokens

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
	/// a name or keyword as written (keywords are told apart by the parser)
	Name(String),
	/// a number as written
	Number(String),
	Str(Box<StrTok>),
	Op(&'static str),
	Newline,
	Indent,
	End,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StrTok {
	pub bytes: bool,
	pub raw: bool,
	/// written with a lower-case u prefix (the constant's kind)
	pub u: bool,
	/// the text between the quotes, as written (plain strings)
	pub body: String,
	/// the pieces of an f-string
	pub fstring: Option<Vec<FPart>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FPart {
	/// literal text as written, with {{ and }} already halved
	Lit(String),
	Field(Box<Field>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
	pub tokens: Vec<Token>,
	/// the text before = and = itself, for {expr=}
	pub debug: Option<String>,
	pub conversion: Option<String>,
	pub spec: Option<Vec<FPart>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
	pub tok: Tok,
	pub line: usize,
	/// UTF-8 bytes from the start of its line
	pub col: usize,
}

const OPS: [&str; 48] = [
	"**=", "//=", ">>=", "<<=", "...", "!=", "%=", "&=", "**", "*=", "+=", "-=", "->", "//", "/=", ":=", "<<", "<=", "<>", "==", ">=", ">>", "@=", "^=",
	"|=", "%", "&", "(", ")", "*", "+", ",", "-", ".", "/", ":", ";", "<", "=", ">", "@", "[", "]", "^", "{", "|", "}", "~",
];

pub const KEYWORDS: [&str; 35] = [
	"False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except",
	"finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try",
	"while", "with", "yield",
];

const MAX_LEVEL: usize = 200;

type Fail = ();
type R<T> = Result<T, Fail>;

struct Lexer<'a> {
	s: &'a str,
	b: &'a [u8],
	i: usize,
	line: usize,
	line_start: usize,
	fstring_depth: usize,
}

/// Where a run of tokens stops: the end of the text, or the end of an f-string replacement field.
#[derive(Clone, Copy, PartialEq)]
enum Mode {
	Top,
	Field,
}

/// What ended a replacement field's expression.
#[derive(Clone, Copy, PartialEq, Debug)]
enum FieldEnd {
	Close,
	Bang,
	Colon,
	Equals,
}

fn is_id_start(c: char) -> bool {
	c == '_' || c.is_ascii_alphabetic() || c as u32 >= 128
}

fn is_id_char(c: char) -> bool {
	c == '_' || c.is_ascii_alphanumeric() || c as u32 >= 128
}

impl<'a> Lexer<'a> {
	fn peek(&self) -> Option<char> {
		self.s[self.i..].chars().next()
	}

	fn peek_at(&self, k: usize) -> Option<char> {
		self.s.get(self.i + k..).and_then(|t| t.chars().next())
	}

	fn at(&self, i: usize) -> u8 {
		self.b.get(i).copied().unwrap_or(0)
	}

	fn newline(&mut self) {
		self.line += 1;
		self.line_start = self.i;
	}

	fn token(&self, tok: Tok, start: usize, line: usize, line_start: usize) -> Token {
		Token { tok, line, col: start - line_start }
	}

	/// Tokens up to the end (Top) or the end of a replacement field (Field).
	fn run(&mut self, mode: Mode, quote: Option<(u8, bool)>) -> R<(Vec<Token>, FieldEnd)> {
		let mut out: Vec<Token> = Vec::new();
		let mut levels: Vec<u8> = Vec::new();
		let mut at_line_start = mode == Mode::Top;
		let mut continued = false;
		let mut line_has_tokens = false;
		loop {
			if at_line_start && levels.is_empty() && !continued {
				at_line_start = false;
				let mut col = 0;
				while matches!(self.at(self.i), b' ' | b'\t' | b'\x0c') {
					self.i += 1;
					col += 1;
				}
				let c = self.at(self.i);
				let blank = (self.i < self.b.len() && c == b'#') || c == b'\n' || (self.i >= self.b.len() && col == 0);
				if !blank && col > 0 {
					out.push(self.token(Tok::Indent, self.i, self.line, self.line_start));
				}
			}
			continued = false;
			while matches!(self.at(self.i), b' ' | b'\t' | b'\x0c') {
				self.i += 1;
			}
			let Some(c) = self.peek() else {
				if mode == Mode::Field || !levels.is_empty() {
					return Err(());
				}
				if line_has_tokens {
					out.push(self.token(Tok::Newline, self.i, self.line, self.line_start));
				}
				out.push(self.token(Tok::End, self.i, self.line, self.line_start));
				return Ok((out, FieldEnd::Close));
			};
			let start = self.i;
			let (line, line_start) = (self.line, self.line_start);
			if c == '#' {
				while self.i < self.b.len() && self.b[self.i] != b'\n' {
					self.i += 1;
				}
				continue;
			}
			if c == '\n' {
				self.i += 1;
				let had = line_has_tokens;
				self.newline();
				if mode == Mode::Top && levels.is_empty() {
					if had {
						out.push(self.token(Tok::Newline, start, line, line_start));
					}
					line_has_tokens = false;
					at_line_start = true;
				} else if let Some((_, false)) = quote {
					// a single-quoted f-string's field may run over lines
				}
				continue;
			}
			if c == '\\' {
				if self.at(self.i + 1) == b'\n' {
					self.i += 2;
					self.newline();
					if self.i >= self.b.len() {
						return Err(());
					}
					continued = true;
					continue;
				}
				return Err(());
			}
			if mode == Mode::Field && levels.is_empty() {
				match c {
					'}' => {
						self.i += 1;
						return Ok((out, FieldEnd::Close));
					}
					'!' if self.at(self.i + 1) != b'=' => {
						self.i += 1;
						return Ok((out, FieldEnd::Bang));
					}
					':' => {
						self.i += 1;
						return Ok((out, FieldEnd::Colon));
					}
					'=' if self.at(self.i + 1) != b'=' => {
						self.i += 1;
						return Ok((out, FieldEnd::Equals));
					}
					_ => {}
				}
			}
			line_has_tokens = true;
			// numbers
			if c.is_ascii_digit() || (c == '.' && self.at(self.i + 1).is_ascii_digit()) {
				let text = self.number()?;
				out.push(self.token(Tok::Number(text), start, line, line_start));
				continue;
			}
			if is_id_start(c) {
				// string prefixes
				let mut j = self.i;
				let (mut saw_b, mut saw_r, mut saw_u, mut saw_f) = (false, false, false, false);
				loop {
					let ch = self.at(j);
					let lower = ch.to_ascii_lowercase();
					if lower == b'b' && !(saw_b || saw_u || saw_f) {
						saw_b = true;
					} else if lower == b'u' && !(saw_b || saw_u || saw_r || saw_f) {
						saw_u = true;
					} else if lower == b'r' && !(saw_r || saw_u) {
						saw_r = true;
					} else if lower == b'f' && !(saw_f || saw_b || saw_u) {
						saw_f = true;
					} else {
						break;
					}
					j += 1;
					if matches!(self.at(j), b'"' | b'\'') {
						let u = self.at(self.i) == b'u';
						self.i = j;
						let tok = self.string(saw_b, saw_r, u, saw_f)?;
						out.push(self.token(Tok::Str(Box::new(tok)), start, line, line_start));
						break;
					}
				}
				if self.i != start {
					continue;
				}
				let mut j = self.i;
				while let Some(ch) = self.s[j..].chars().next() {
					if !is_id_char(ch) {
						break;
					}
					j += ch.len_utf8();
				}
				let word = &self.s[self.i..j];
				if !word.is_ascii() {
					let normal = py::nfkc(word);
					let mut chars = normal.chars();
					let ok = chars.next().is_some_and(py::is_name_start) && chars.all(py::is_name_continue);
					if !ok {
						return Err(());
					}
				}
				self.i = j;
				out.push(self.token(Tok::Name(word.to_string()), start, line, line_start));
				continue;
			}
			if c == '"' || c == '\'' {
				let tok = self.string(false, false, false, false)?;
				out.push(self.token(Tok::Str(Box::new(tok)), start, line, line_start));
				continue;
			}
			if let Some(op) = OPS.iter().find(|op| self.s[self.i..].starts_with(**op)) {
				self.i += op.len();
				match *op {
					"(" | "[" | "{" => {
						if levels.len() >= MAX_LEVEL {
							return Err(());
						}
						levels.push(op.as_bytes()[0]);
					}
					")" | "]" | "}" => {
						let want = match *op {
							")" => b'(',
							"]" => b'[',
							_ => b'{',
						};
						if levels.pop() != Some(want) {
							return Err(());
						}
					}
					_ => {}
				}
				out.push(self.token(Tok::Op(op), start, line, line_start));
				continue;
			}
			return Err(());
		}
	}

	/// The end of a number the tokenizer accepts after a literal: (true, so far) or an error.
	fn end_of_number(&self, j: usize) -> R<()> {
		let c = self.at(j);
		let follows = |word: &str| self.s.get(j..).is_some_and(|t| t.starts_with(word));
		let keyword = match c {
			b'a' => follows("and"),
			b'e' => follows("else"),
			b'f' => follows("for"),
			b'i' => matches!(self.at(j + 1), b'f' | b'n' | b's'),
			b'o' => follows("or"),
			b'n' => follows("not"),
			_ => false,
		};
		if !keyword && c < 128 && (c == b'_' || c.is_ascii_alphanumeric()) {
			return Err(());
		}
		Ok(())
	}

	fn decimal_tail(&self, mut j: usize) -> R<usize> {
		loop {
			while self.at(j).is_ascii_digit() {
				j += 1;
			}
			if self.at(j) != b'_' {
				return Ok(j);
			}
			j += 1;
			if !self.at(j).is_ascii_digit() {
				return Err(());
			}
		}
	}

	fn number(&mut self) -> R<String> {
		let start = self.i;
		let mut j = self.i;
		let radix = |c: u8, base: u8| match base {
			16 => c.is_ascii_hexdigit(),
			8 => (b'0'..b'8').contains(&c),
			_ => c == b'0' || c == b'1',
		};
		let c0 = self.at(j);
		if c0 == b'0' && matches!(self.at(j + 1), b'x' | b'X' | b'o' | b'O' | b'b' | b'B') {
			let base = match self.at(j + 1) {
				b'x' | b'X' => 16,
				b'o' | b'O' => 8,
				_ => 2,
			};
			j += 2;
			loop {
				if self.at(j) == b'_' {
					j += 1;
				}
				if !radix(self.at(j), base) {
					return Err(());
				}
				while radix(self.at(j), base) {
					j += 1;
				}
				if self.at(j) != b'_' {
					break;
				}
			}
			if base != 16 && self.at(j).is_ascii_digit() {
				return Err(());
			}
			self.end_of_number(j)?;
			self.i = j;
			return Ok(self.s[start..j].to_string());
		}
		let fraction_or_exponent = |mut j: usize, me: &Self| -> R<usize> {
			// at '.', 'e' or 'j' after the integer part
			if me.at(j) == b'.' {
				j += 1;
				if me.at(j).is_ascii_digit() {
					j = me.decimal_tail(j)?;
				}
			}
			if matches!(me.at(j), b'e' | b'E') {
				let e = j;
				j += 1;
				if matches!(me.at(j), b'+' | b'-') {
					j += 1;
					if !me.at(j).is_ascii_digit() {
						return Err(());
					}
				} else if !me.at(j).is_ascii_digit() {
					me.end_of_number(e)?;
					return Ok(e);
				}
				j = me.decimal_tail(j)?;
			}
			if matches!(me.at(j), b'j' | b'J') {
				j += 1;
				me.end_of_number(j)?;
				return Ok(j);
			}
			me.end_of_number(j)?;
			Ok(j)
		};
		if c0 == b'0' {
			// zeros, maybe with underscores, then perhaps more digits (only as a float or imaginary)
			j += 1;
			loop {
				if self.at(j) == b'_' {
					j += 1;
					if !self.at(j).is_ascii_digit() {
						return Err(());
					}
				}
				if self.at(j) != b'0' {
					break;
				}
				j += 1;
			}
			let mut nonzero = false;
			if self.at(j).is_ascii_digit() {
				nonzero = true;
				j = self.decimal_tail(j)?;
			}
			if matches!(self.at(j), b'.' | b'e' | b'E' | b'j' | b'J') {
				let end = fraction_or_exponent(j, self)?;
				self.i = end;
				return Ok(self.s[start..end].to_string());
			}
			if nonzero {
				return Err(());
			}
			self.end_of_number(j)?;
			self.i = j;
			return Ok(self.s[start..j].to_string());
		}
		if c0 != b'.' {
			j = self.decimal_tail(j)?;
		}
		let end = fraction_or_exponent(j, self)?;
		self.i = end;
		Ok(self.s[start..end].to_string())
	}

	/// A string literal from its opening quote.
	fn string(&mut self, bytes: bool, raw: bool, u: bool, f: bool) -> R<StrTok> {
		let quote = self.at(self.i);
		let triple = self.at(self.i + 1) == quote && self.at(self.i + 2) == quote;
		self.i += if triple { 3 } else { 1 };
		if f {
			self.fstring_depth += 1;
			if self.fstring_depth > 150 {
				return Err(());
			}
			let parts = self.fstring_parts(quote, triple, raw, false)?;
			self.fstring_depth -= 1;
			return Ok(StrTok { bytes, raw, u, body: String::new(), fstring: Some(parts) });
		}
		let body_start = self.i;
		loop {
			let Some(c) = self.peek() else { return Err(()) };
			if c == quote as char {
				if !triple {
					let body = self.s[body_start..self.i].to_string();
					self.i += 1;
					return Ok(StrTok { bytes, raw, u, body, fstring: None });
				}
				if self.at(self.i + 1) == quote && self.at(self.i + 2) == quote {
					let body = self.s[body_start..self.i].to_string();
					self.i += 3;
					return Ok(StrTok { bytes, raw, u, body, fstring: None });
				}
			}
			if c == '\n' {
				if !triple {
					return Err(());
				}
				self.i += 1;
				self.newline();
				continue;
			}
			if c == '\\' {
				self.i += 1;
				match self.peek() {
					None => return Err(()),
					Some('\n') => {
						self.i += 1;
						self.newline();
					}
					Some(next) => self.i += next.len_utf8(),
				}
				continue;
			}
			self.i += c.len_utf8();
		}
	}

	/// The pieces of an f-string (or of a format spec, spec = true) up to its end.
	fn fstring_parts(&mut self, quote: u8, triple: bool, raw: bool, spec: bool) -> R<Vec<FPart>> {
		let mut parts = Vec::new();
		let mut lit = String::new();
		loop {
			let Some(c) = self.peek() else { return Err(()) };
			if c as u32 == quote as u32 && (!triple || (self.at(self.i + 1) == quote && self.at(self.i + 2) == quote)) {
				if spec {
					return Err(());
				}
				self.i += if triple { 3 } else { 1 };
				if !lit.is_empty() {
					parts.push(FPart::Lit(lit));
				}
				return Ok(parts);
			}
			if c == '\n' {
				if !triple {
					return Err(());
				}
				lit.push('\n');
				self.i += 1;
				self.newline();
				continue;
			}
			if c == '{' {
				if !spec && self.peek_at(1) == Some('{') {
					lit.push('{');
					self.i += 2;
					continue;
				}
				self.i += 1;
				if !lit.is_empty() {
					parts.push(FPart::Lit(std::mem::take(&mut lit)));
				}
				parts.push(FPart::Field(Box::new(self.field(quote, triple, raw)?)));
				continue;
			}
			if c == '}' {
				if spec {
					if !lit.is_empty() {
						parts.push(FPart::Lit(lit));
					}
					return Ok(parts);
				}
				if self.peek_at(1) == Some('}') {
					lit.push('}');
					self.i += 2;
					continue;
				}
				return Err(());
			}
			if c == '\\' && !raw {
				// an escape: its next character is literal (a \N{...} keeps its braces)
				lit.push('\\');
				self.i += 1;
				let Some(next) = self.peek() else { return Err(()) };
				if next == 'N' && self.peek_at(1) == Some('{') {
					let close = self.s[self.i..].find('}').ok_or(())?;
					lit.push_str(&self.s[self.i..self.i + close + 1]);
					self.i += close + 1;
					continue;
				}
				if next == '\n' {
					lit.push('\n');
					self.i += 1;
					self.newline();
					continue;
				}
				if next == '{' || next == '}' {
					continue;
				}
				lit.push(next);
				self.i += next.len_utf8();
				continue;
			}
			lit.push(c);
			self.i += c.len_utf8();
		}
	}

	/// A replacement field after its '{'.
	fn field(&mut self, quote: u8, triple: bool, raw: bool) -> R<Field> {
		let expr_start = self.i;
		let (mut tokens, mut end) = self.run(Mode::Field, Some((quote, triple)))?;
		if tokens.is_empty() {
			return Err(());
		}
		let mut debug = None;
		if end == FieldEnd::Equals {
			let text_end = self.i;
			// whitespace after '=' belongs to the debug text
			while matches!(self.at(self.i), b' ' | b'\t' | b'\x0c' | b'\n') {
				if self.at(self.i) == b'\n' {
					self.i += 1;
					self.newline();
				} else {
					self.i += 1;
				}
			}
			debug = Some(self.s[expr_start..text_end].to_string() + &self.s[text_end..self.i]);
			end = match self.peek() {
				Some('}') => {
					self.i += 1;
					FieldEnd::Close
				}
				Some('!') if self.at(self.i + 1) != b'=' => {
					self.i += 1;
					FieldEnd::Bang
				}
				Some(':') => {
					self.i += 1;
					FieldEnd::Colon
				}
				_ => return Err(()),
			};
		}
		let mut conversion = None;
		if end == FieldEnd::Bang {
			let mut j = self.i;
			while let Some(ch) = self.s[j..].chars().next() {
				if !is_id_char(ch) {
					break;
				}
				j += ch.len_utf8();
			}
			if j == self.i {
				return Err(());
			}
			conversion = Some(self.s[self.i..j].to_string());
			self.i = j;
			while matches!(self.at(self.i), b' ' | b'\t' | b'\x0c') {
				self.i += 1;
			}
			end = match self.peek() {
				Some('}') => {
					self.i += 1;
					FieldEnd::Close
				}
				Some(':') => {
					self.i += 1;
					FieldEnd::Colon
				}
				_ => return Err(()),
			};
		}
		let mut spec = None;
		if end == FieldEnd::Colon {
			spec = Some(self.fstring_parts(quote, triple, raw, true)?);
			if self.peek() != Some('}') {
				return Err(());
			}
			self.i += 1;
		}
		let line = tokens.last().map(|t| t.line).unwrap_or(self.line);
		tokens.push(Token { tok: Tok::End, line, col: 0 });
		Ok(Field { tokens, debug, conversion, spec })
	}
}

/// Python's tokens for an expression (None: a syntax error). Line ends as Python reads them.
pub fn tokenize(text: &str) -> Option<Vec<Token>> {
	let text = text.replace("\r\n", "\n").replace('\r', "\n");
	let mut lexer = Lexer { s: &text, b: text.as_bytes(), i: 0, line: 1, line_start: 0, fstring_depth: 0 };
	lexer.run(Mode::Top, None).ok().map(|(tokens, _)| tokens)
}

// ------------------------------------------------------------------ the tree

#[derive(Clone, Debug, PartialEq)]
pub enum Const {
	/// an integer, in decimal
	Int(String),
	Float(f64),
	/// an imaginary number: its imaginary part
	Imag(f64),
	/// a string, as code points (it may hold lone surrogates, as Python's can)
	Str(Vec<u32>),
	Bytes(Vec<u8>),
	True,
	False,
	None,
	Ellipsis,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Arguments {
	pub posonly: Vec<String>,
	pub args: Vec<String>,
	pub vararg: Option<String>,
	pub kwonly: Vec<String>,
	pub kw_defaults: Vec<Option<Node>>,
	pub kwarg: Option<String>,
	pub defaults: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comp {
	pub target: Node,
	pub iter: Node,
	pub ifs: Vec<Node>,
	pub is_async: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
	BoolOp { and: bool, values: Vec<Node> },
	NamedExpr { target: Box<Node>, value: Box<Node> },
	BinOp { left: Box<Node>, op: &'static str, right: Box<Node> },
	UnaryOp { op: &'static str, operand: Box<Node> },
	Lambda { args: Box<Arguments>, body: Box<Node> },
	IfExp { test: Box<Node>, body: Box<Node>, orelse: Box<Node> },
	Dict { keys: Vec<Option<Node>>, values: Vec<Node> },
	Set(Vec<Node>),
	ListComp { elt: Box<Node>, gens: Vec<Comp> },
	SetComp { elt: Box<Node>, gens: Vec<Comp> },
	DictComp { key: Box<Node>, value: Box<Node>, gens: Vec<Comp> },
	GeneratorExp { elt: Box<Node>, gens: Vec<Comp> },
	Await(Box<Node>),
	Yield(Option<Box<Node>>),
	YieldFrom(Box<Node>),
	Compare { left: Box<Node>, ops: Vec<&'static str>, comparators: Vec<Node> },
	Call { func: Box<Node>, args: Vec<Node>, keywords: Vec<(Option<String>, Node)>, col: usize },
	FormattedValue { value: Box<Node>, conversion: i64, spec: Option<Box<Node>> },
	JoinedStr(Vec<Node>),
	Constant { value: Const, u: bool },
	Attribute { value: Box<Node>, attr: String, store: bool },
	Subscript { value: Box<Node>, slice: Box<Node>, store: bool },
	Starred { value: Box<Node>, store: bool },
	Name { id: String, store: bool },
	List { elts: Vec<Node>, store: bool },
	Tuple { elts: Vec<Node>, store: bool },
	Slice { lower: Option<Box<Node>>, upper: Option<Box<Node>>, step: Option<Box<Node>> },
}

// ------------------------------------------------------------------ ast.dump

/// The decimal digits Python will write an int with (sys.get_int_max_str_digits()).
const MAX_STR_DIGITS: usize = 4300;

/// `ast.dump(node)`, or the ValueError repr() of a too-long integer raises on the way.
pub fn dump(node: &Node) -> Result<String, String> {
	let mut out = String::new();
	write_node(node, &mut out)?;
	Ok(out)
}

fn ctx(store: bool) -> &'static str {
	if store {
		"Store()"
	} else {
		"Load()"
	}
}

struct Fields<'a> {
	out: &'a mut String,
	first: bool,
}

impl Fields<'_> {
	fn key(&mut self, name: &str) {
		if !self.first {
			self.out.push_str(", ");
		}
		self.first = false;
		self.out.push_str(name);
		self.out.push('=');
	}

	fn node(&mut self, name: &str, node: &Node) -> Result<(), String> {
		self.key(name);
		write_node(node, self.out)
	}

	fn opt(&mut self, name: &str, node: &Option<Box<Node>>) -> Result<(), String> {
		if let Some(n) = node {
			self.node(name, n)?;
		}
		Ok(())
	}

	fn list(&mut self, name: &str, nodes: &[Node]) -> Result<(), String> {
		if nodes.is_empty() {
			return Ok(());
		}
		self.key(name);
		self.out.push('[');
		for (k, n) in nodes.iter().enumerate() {
			if k > 0 {
				self.out.push_str(", ");
			}
			write_node(n, self.out)?;
		}
		self.out.push(']');
		Ok(())
	}

	fn raw(&mut self, name: &str, text: &str) {
		self.key(name);
		self.out.push_str(text);
	}
}

fn class(out: &mut String, name: &str, body: impl FnOnce(&mut Fields) -> Result<(), String>) -> Result<(), String> {
	out.push_str(name);
	out.push('(');
	let mut fields = Fields { out, first: true };
	body(&mut fields)?;
	fields.out.push(')');
	Ok(())
}

fn write_arg(out: &mut String, name: &str) {
	out.push_str(&format!("arg(arg={})", py::repr_str(name)));
}

fn write_comps(f: &mut Fields, gens: &[Comp]) -> Result<(), String> {
	f.key("generators");
	f.out.push('[');
	for (k, g) in gens.iter().enumerate() {
		if k > 0 {
			f.out.push_str(", ");
		}
		class(f.out, "comprehension", |c| {
			c.node("target", &g.target)?;
			c.node("iter", &g.iter)?;
			c.list("ifs", &g.ifs)?;
			c.raw("is_async", if g.is_async { "1" } else { "0" });
			Ok(())
		})?;
	}
	f.out.push(']');
	Ok(())
}

/// repr() of an int written in decimal.
fn int_repr(digits: &str) -> Result<String, String> {
	if digits.len() > MAX_STR_DIGITS {
		return Err(format!(
			"Exceeds the limit ({MAX_STR_DIGITS} digits) for integer string conversion; use sys.set_int_max_str_digits() to increase the limit"
		));
	}
	Ok(digits.to_string())
}

/// repr() of a float as complex()'s parts write it: like repr(), without a trailing .0.
fn complex_part(x: f64) -> String {
	let r = py::repr(x);
	r.strip_suffix(".0").map(str::to_string).unwrap_or(r)
}

/// repr(complex(re, im)).
pub fn complex_repr(re: f64, im: f64) -> String {
	if re == 0.0 && re.is_sign_positive() {
		return format!("{}j", complex_part(im));
	}
	let imag = complex_part(im);
	let sign = if imag.starts_with('-') { "" } else { "+" };
	format!("({}{sign}{imag}j)", complex_part(re))
}

/// repr() of a str held as code points.
pub fn str_repr(points: &[u32]) -> String {
	let has = |c: char| points.contains(&(c as u32));
	let quote = if has('\'') && !has('"') { '"' } else { '\'' };
	let mut out = String::new();
	out.push(quote);
	for &p in points {
		match char::from_u32(p) {
			Some('\\') => out.push_str("\\\\"),
			Some('\n') => out.push_str("\\n"),
			Some('\r') => out.push_str("\\r"),
			Some('\t') => out.push_str("\\t"),
			Some(c) if c == quote => {
				out.push('\\');
				out.push(c);
			}
			Some(c) if p >= 0x20 && p != 0x7f && (p < 0x7f || py::is_printable(c)) => out.push(c),
			_ if p <= 0xff => out.push_str(&format!("\\x{p:02x}")),
			_ if p <= 0xffff => out.push_str(&format!("\\u{p:04x}")),
			_ => out.push_str(&format!("\\U{p:08x}")),
		}
	}
	out.push(quote);
	out
}

fn bytes_repr(bytes: &[u8]) -> String {
	let quote = if bytes.contains(&b'\'') && !bytes.contains(&b'"') { b'"' } else { b'\'' };
	let mut out = String::from("b");
	out.push(quote as char);
	for &b in bytes {
		match b {
			b'\\' => out.push_str("\\\\"),
			b'\t' => out.push_str("\\t"),
			b'\n' => out.push_str("\\n"),
			b'\r' => out.push_str("\\r"),
			_ if b == quote => {
				out.push('\\');
				out.push(b as char);
			}
			0x20..=0x7e => out.push(b as char),
			_ => out.push_str(&format!("\\x{b:02x}")),
		}
	}
	out.push(quote as char);
	out
}

pub fn const_repr(value: &Const) -> Result<String, String> {
	Ok(match value {
		Const::Int(d) => int_repr(d)?,
		Const::Float(x) => py::repr(*x),
		Const::Imag(x) => complex_repr(0.0, *x),
		Const::Str(s) => str_repr(s),
		Const::Bytes(b) => bytes_repr(b),
		Const::True => "True".into(),
		Const::False => "False".into(),
		Const::None => "None".into(),
		Const::Ellipsis => "Ellipsis".into(),
	})
}

fn write_node(node: &Node, out: &mut String) -> Result<(), String> {
	match node {
		Node::BoolOp { and, values } => class(out, "BoolOp", |f| {
			f.raw("op", if *and { "And()" } else { "Or()" });
			f.list("values", values)
		}),
		Node::NamedExpr { target, value } => class(out, "NamedExpr", |f| {
			f.node("target", target)?;
			f.node("value", value)
		}),
		Node::BinOp { left, op, right } => class(out, "BinOp", |f| {
			f.node("left", left)?;
			f.raw("op", &format!("{op}()"));
			f.node("right", right)
		}),
		Node::UnaryOp { op, operand } => class(out, "UnaryOp", |f| {
			f.raw("op", &format!("{op}()"));
			f.node("operand", operand)
		}),
		Node::Lambda { args, body } => class(out, "Lambda", |f| {
			f.key("args");
			class(f.out, "arguments", |a| {
				let list = |a: &mut Fields, name: &str, names: &[String]| {
					if !names.is_empty() {
						a.key(name);
						a.out.push('[');
						for (k, n) in names.iter().enumerate() {
							if k > 0 {
								a.out.push_str(", ");
							}
							write_arg(a.out, n);
						}
						a.out.push(']');
					}
				};
				list(a, "posonlyargs", &args.posonly);
				list(a, "args", &args.args);
				if let Some(v) = &args.vararg {
					a.key("vararg");
					write_arg(a.out, v);
				}
				list(a, "kwonlyargs", &args.kwonly);
				if !args.kw_defaults.is_empty() {
					a.key("kw_defaults");
					a.out.push('[');
					for (k, d) in args.kw_defaults.iter().enumerate() {
						if k > 0 {
							a.out.push_str(", ");
						}
						match d {
							Some(n) => write_node(n, a.out)?,
							None => a.out.push_str("None"),
						}
					}
					a.out.push(']');
				}
				if let Some(k) = &args.kwarg {
					a.key("kwarg");
					write_arg(a.out, k);
				}
				a.list("defaults", &args.defaults)
			})?;
			f.node("body", body)
		}),
		Node::IfExp { test, body, orelse } => class(out, "IfExp", |f| {
			f.node("test", test)?;
			f.node("body", body)?;
			f.node("orelse", orelse)
		}),
		Node::Dict { keys, values } => class(out, "Dict", |f| {
			if !keys.is_empty() {
				f.key("keys");
				f.out.push('[');
				for (k, key) in keys.iter().enumerate() {
					if k > 0 {
						f.out.push_str(", ");
					}
					match key {
						Some(n) => write_node(n, f.out)?,
						None => f.out.push_str("None"),
					}
				}
				f.out.push(']');
			}
			f.list("values", values)
		}),
		Node::Set(elts) => class(out, "Set", |f| f.list("elts", elts)),
		Node::ListComp { elt, gens } => class(out, "ListComp", |f| {
			f.node("elt", elt)?;
			write_comps(f, gens)
		}),
		Node::SetComp { elt, gens } => class(out, "SetComp", |f| {
			f.node("elt", elt)?;
			write_comps(f, gens)
		}),
		Node::GeneratorExp { elt, gens } => class(out, "GeneratorExp", |f| {
			f.node("elt", elt)?;
			write_comps(f, gens)
		}),
		Node::DictComp { key, value, gens } => class(out, "DictComp", |f| {
			f.node("key", key)?;
			f.node("value", value)?;
			write_comps(f, gens)
		}),
		Node::Await(v) => class(out, "Await", |f| f.node("value", v)),
		Node::Yield(v) => class(out, "Yield", |f| f.opt("value", v)),
		Node::YieldFrom(v) => class(out, "YieldFrom", |f| f.node("value", v)),
		Node::Compare { left, ops, comparators } => class(out, "Compare", |f| {
			f.node("left", left)?;
			f.raw("ops", &format!("[{}]", ops.iter().map(|o| format!("{o}()")).collect::<Vec<_>>().join(", ")));
			f.list("comparators", comparators)
		}),
		Node::Call { func, args, keywords, .. } => class(out, "Call", |f| {
			f.node("func", func)?;
			f.list("args", args)?;
			if !keywords.is_empty() {
				f.key("keywords");
				f.out.push('[');
				for (k, (name, value)) in keywords.iter().enumerate() {
					if k > 0 {
						f.out.push_str(", ");
					}
					class(f.out, "keyword", |kw| {
						if let Some(name) = name {
							kw.raw("arg", &py::repr_str(name));
						}
						kw.node("value", value)
					})?;
				}
				f.out.push(']');
			}
			Ok(())
		}),
		Node::FormattedValue { value, conversion, spec } => class(out, "FormattedValue", |f| {
			f.node("value", value)?;
			f.raw("conversion", &conversion.to_string());
			f.opt("format_spec", spec)
		}),
		Node::JoinedStr(values) => class(out, "JoinedStr", |f| f.list("values", values)),
		Node::Constant { value, u } => class(out, "Constant", |f| {
			f.raw("value", &const_repr(value)?);
			if *u {
				f.raw("kind", "'u'");
			}
			Ok(())
		}),
		Node::Attribute { value, attr, store } => class(out, "Attribute", |f| {
			f.node("value", value)?;
			f.raw("attr", &py::repr_str(attr));
			f.raw("ctx", ctx(*store));
			Ok(())
		}),
		Node::Subscript { value, slice, store } => class(out, "Subscript", |f| {
			f.node("value", value)?;
			f.node("slice", slice)?;
			f.raw("ctx", ctx(*store));
			Ok(())
		}),
		Node::Starred { value, store } => class(out, "Starred", |f| {
			f.node("value", value)?;
			f.raw("ctx", ctx(*store));
			Ok(())
		}),
		Node::Name { id, store } => class(out, "Name", |f| {
			f.raw("id", &py::repr_str(id));
			f.raw("ctx", ctx(*store));
			Ok(())
		}),
		Node::List { elts, store } => class(out, "List", |f| {
			f.list("elts", elts)?;
			f.raw("ctx", ctx(*store));
			Ok(())
		}),
		Node::Tuple { elts, store } => class(out, "Tuple", |f| {
			f.list("elts", elts)?;
			f.raw("ctx", ctx(*store));
			Ok(())
		}),
		Node::Slice { lower, upper, step } => class(out, "Slice", |f| {
			f.opt("lower", lower)?;
			f.opt("upper", upper)?;
			f.opt("step", step)
		}),
	}
}

// ------------------------------------------------------------------ literals

/// An integer literal's value in decimal (hex, octal and binary converted; underscores dropped).
fn int_digits(text: &str) -> String {
	let clean: String = text.chars().filter(|c| *c != '_').collect();
	let lower = clean.to_ascii_lowercase();
	let (radix, digits) = if let Some(d) = lower.strip_prefix("0x") {
		(16, d)
	} else if let Some(d) = lower.strip_prefix("0o") {
		(8, d)
	} else if let Some(d) = lower.strip_prefix("0b") {
		(2, d)
	} else {
		let trimmed = lower.trim_start_matches('0');
		return if trimmed.is_empty() { "0".into() } else { trimmed.to_string() };
	};
	// base conversion on little-endian base-10^9 limbs
	let mut limbs: Vec<u64> = vec![0];
	for ch in digits.chars() {
		let mut carry = ch.to_digit(radix).unwrap() as u64;
		for limb in limbs.iter_mut() {
			let v = *limb * radix as u64 + carry;
			*limb = v % 1_000_000_000;
			carry = v / 1_000_000_000;
		}
		while carry > 0 {
			limbs.push(carry % 1_000_000_000);
			carry /= 1_000_000_000;
		}
	}
	let mut out = limbs.last().unwrap().to_string();
	for limb in limbs.iter().rev().skip(1) {
		out.push_str(&format!("{limb:09}"));
	}
	out
}

fn number_const(text: &str) -> Option<Const> {
	let lower = text.to_ascii_lowercase();
	if lower.starts_with("0x") || lower.starts_with("0o") || lower.starts_with("0b") {
		return Some(Const::Int(int_digits(text)));
	}
	let clean: String = text.chars().filter(|c| *c != '_').collect();
	if let Some(imag) = clean.strip_suffix(['j', 'J']) {
		return Some(Const::Imag(parse_float(imag)));
	}
	if clean.contains(['.', 'e', 'E']) {
		return Some(Const::Float(parse_float(&clean)));
	}
	let digits = int_digits(text);
	if digits.len() > MAX_STR_DIGITS {
		return None; // Python refuses to read so long an int (a syntax error)
	}
	Some(Const::Int(digits))
}

fn parse_float(text: &str) -> f64 {
	let t = if text.ends_with('.') { format!("{text}0") } else { text.to_string() };
	let t = if t.starts_with('.') { format!("0{t}") } else { t };
	t.parse().unwrap_or(f64::NAN)
}

/// A string literal's value: escapes worked out (unless raw); None when Python rejects it.
fn decode(body: &str, raw: bool, bytes: bool) -> Option<Const> {
	if bytes && !body.is_ascii() {
		return None;
	}
	let mut out: Vec<u32> = Vec::new();
	if raw {
		out.extend(body.chars().map(|c| c as u32));
	} else {
		let chars: Vec<char> = body.chars().collect();
		let mut k = 0;
		while k < chars.len() {
			let c = chars[k];
			if c != '\\' {
				out.push(c as u32);
				k += 1;
				continue;
			}
			k += 1;
			let Some(&e) = chars.get(k) else { return None };
			k += 1;
			match e {
				'\n' => {}
				'\\' => out.push('\\' as u32),
				'\'' => out.push('\'' as u32),
				'"' => out.push('"' as u32),
				'a' => out.push(7),
				'b' => out.push(8),
				'f' => out.push(12),
				'n' => out.push(10),
				'r' => out.push(13),
				't' => out.push(9),
				'v' => out.push(11),
				'0'..='7' => {
					let mut v = e.to_digit(8).unwrap();
					for _ in 0..2 {
						match chars.get(k).and_then(|d| d.to_digit(8)) {
							Some(d) => {
								v = v * 8 + d;
								k += 1;
							}
							None => break,
						}
					}
					out.push(if bytes { v & 0xff } else { v });
				}
				'x' | 'u' | 'U' if !(bytes && e != 'x') => {
					let n = match e {
						'x' => 2,
						'u' => 4,
						_ => 8,
					};
					let hex: String = chars.get(k..k + n)?.iter().collect();
					if hex.len() != n || !hex.chars().all(|h| h.is_ascii_hexdigit()) {
						return None;
					}
					let v = u32::from_str_radix(&hex, 16).ok()?;
					if v > 0x10FFFF {
						return None;
					}
					out.push(v);
					k += n;
				}
				'N' if !bytes => return None,
				other => {
					out.push('\\' as u32);
					out.push(other as u32);
				}
			}
		}
	}
	Some(if bytes { Const::Bytes(out.iter().map(|&p| p as u8).collect()) } else { Const::Str(out) })
}

// ------------------------------------------------------------------ the parser

struct Parser<'t> {
	toks: &'t [Token],
	pos: usize,
	depth: usize,
}

const MAX_DEPTH: usize = 900;
/// The longest run of one operator (or of trailers) read: a deeper tree than Python could walk.
const MAX_CHAIN: usize = 3000;

fn op_name(op: &str) -> &'static str {
	match op {
		"+" => "Add",
		"-" => "Sub",
		"*" => "Mult",
		"/" => "Div",
		"//" => "FloorDiv",
		"%" => "Mod",
		"**" => "Pow",
		"@" => "MatMult",
		"<<" => "LShift",
		">>" => "RShift",
		"&" => "BitAnd",
		"|" => "BitOr",
		_ => "BitXor",
	}
}

impl<'t> Parser<'t> {
	fn peek(&self) -> &Tok {
		&self.toks[self.pos.min(self.toks.len() - 1)].tok
	}

	fn peek2(&self) -> &Tok {
		&self.toks[(self.pos + 1).min(self.toks.len() - 1)].tok
	}

	fn col(&self) -> usize {
		self.toks[self.pos.min(self.toks.len() - 1)].col
	}

	fn is_op(&self, op: &str) -> bool {
		matches!(self.peek(), Tok::Op(o) if *o == op)
	}

	fn eat(&mut self, op: &str) -> bool {
		if self.is_op(op) {
			self.pos += 1;
			true
		} else {
			false
		}
	}

	fn expect(&mut self, op: &str) -> R<()> {
		if self.eat(op) {
			Ok(())
		} else {
			Err(())
		}
	}

	fn is_kw(&self, word: &str) -> bool {
		matches!(self.peek(), Tok::Name(w) if w == word)
	}

	fn eat_kw(&mut self, word: &str) -> bool {
		if self.is_kw(word) {
			self.pos += 1;
			true
		} else {
			false
		}
	}

	/// A NAME token (not a keyword): its identifier, normalised as Python normalises names.
	fn name(&mut self) -> R<String> {
		match self.peek() {
			Tok::Name(w) if !KEYWORDS.contains(&w.as_str()) => {
				let id = py::nfkc(w);
				self.pos += 1;
				Ok(id)
			}
			_ => Err(()),
		}
	}

	fn is_name(&self) -> bool {
		matches!(self.peek(), Tok::Name(w) if !KEYWORDS.contains(&w.as_str()))
	}

	fn deeper(&mut self) -> R<()> {
		self.depth += 1;
		if self.depth > MAX_DEPTH {
			return Err(());
		}
		Ok(())
	}

	// expressions: expression (',' expression)* [','] -> a tuple when there is a comma
	fn expressions(&mut self) -> R<Node> {
		let first = self.expression()?;
		if !self.is_op(",") {
			return Ok(first);
		}
		let mut elts = vec![first];
		while self.eat(",") {
			if self.starts_expression() {
				elts.push(self.expression()?);
			} else {
				break;
			}
		}
		Ok(Node::Tuple { elts, store: false })
	}

	fn starts_expression(&self) -> bool {
		match self.peek() {
			Tok::Name(w) => {
				!KEYWORDS.contains(&w.as_str()) || matches!(w.as_str(), "True" | "False" | "None" | "not" | "lambda" | "await")
			}
			Tok::Number(_) | Tok::Str(_) => true,
			Tok::Op(o) => matches!(*o, "(" | "[" | "{" | "-" | "+" | "~" | "..."),
			_ => false,
		}
	}

	// star_expressions (an f-string field, a yield's value)
	fn star_expressions(&mut self) -> R<Node> {
		let first = self.star_expression()?;
		if !self.is_op(",") {
			return Ok(first);
		}
		let mut elts = vec![first];
		while self.eat(",") {
			if self.starts_expression() || self.is_op("*") {
				elts.push(self.star_expression()?);
			} else {
				break;
			}
		}
		Ok(Node::Tuple { elts, store: false })
	}

	fn star_expression(&mut self) -> R<Node> {
		if self.eat("*") {
			let value = self.bitwise_or()?;
			return Ok(Node::Starred { value: Box::new(value), store: false });
		}
		self.expression()
	}

	fn star_named_expression(&mut self) -> R<Node> {
		if self.eat("*") {
			let value = self.bitwise_or()?;
			return Ok(Node::Starred { value: Box::new(value), store: false });
		}
		self.named_expression()
	}

	fn named_expression(&mut self) -> R<Node> {
		if self.is_name() && matches!(self.peek2(), Tok::Op(":=")) {
			let id = self.name()?;
			self.pos += 1;
			let value = self.expression()?;
			return Ok(Node::NamedExpr { target: Box::new(Node::Name { id, store: true }), value: Box::new(value) });
		}
		let e = self.expression()?;
		if self.is_op(":=") {
			return Err(());
		}
		Ok(e)
	}

	fn expression(&mut self) -> R<Node> {
		self.deeper()?;
		let out = self.expression_inner();
		self.depth -= 1;
		out
	}

	fn expression_inner(&mut self) -> R<Node> {
		if self.is_kw("lambda") {
			return self.lambdef();
		}
		let body = self.disjunction()?;
		if self.eat_kw("if") {
			let test = self.disjunction()?;
			if !self.eat_kw("else") {
				return Err(());
			}
			let orelse = self.expression()?;
			return Ok(Node::IfExp { test: Box::new(test), body: Box::new(body), orelse: Box::new(orelse) });
		}
		Ok(body)
	}

	fn lambdef(&mut self) -> R<Node> {
		self.pos += 1;
		let args = self.lambda_params()?;
		self.expect(":")?;
		let body = self.expression()?;
		Ok(Node::Lambda { args: Box::new(args), body: Box::new(body) })
	}

	/// After a parameter: ',' (taken) or ':' (left for the caller).
	fn param_end(&mut self) -> R<()> {
		if self.eat(",") || self.is_op(":") {
			Ok(())
		} else {
			Err(())
		}
	}

	fn lambda_params(&mut self) -> R<Arguments> {
		let mut a = Arguments::default();
		// positional (and positional-only) parameters
		let mut names: Vec<String> = Vec::new();
		let mut defaults: Vec<Node> = Vec::new();
		let mut slash = false;
		while self.is_name() {
			let name = self.name()?;
			if self.eat("=") {
				defaults.push(self.expression()?);
			} else if !defaults.is_empty() {
				return Err(());
			}
			names.push(name);
			self.param_end()?;
			if self.is_op("/") {
				if slash || names.is_empty() {
					return Err(());
				}
				self.pos += 1;
				self.param_end()?;
				slash = true;
				a.posonly = std::mem::take(&mut names);
			}
		}
		if self.is_op("/") {
			return Err(());
		}
		a.args = names;
		a.defaults = defaults;
		// * [vararg] kwonly...
		if self.eat("*") {
			if self.is_name() {
				a.vararg = Some(self.name()?);
				self.param_end()?;
			} else {
				if !self.eat(",") {
					return Err(());
				}
				if !self.is_name() {
					return Err(());
				}
			}
			while self.is_name() {
				a.kwonly.push(self.name()?);
				a.kw_defaults.push(if self.eat("=") { Some(self.expression()?) } else { None });
				self.param_end()?;
			}
		}
		if self.eat("**") {
			a.kwarg = Some(self.name()?);
			self.param_end()?;
		}
		Ok(a)
	}

	fn disjunction(&mut self) -> R<Node> {
		let first = self.conjunction()?;
		if !self.is_kw("or") {
			return Ok(first);
		}
		let mut values = vec![first];
		while self.eat_kw("or") {
			values.push(self.conjunction()?);
		}
		Ok(Node::BoolOp { and: false, values })
	}

	fn conjunction(&mut self) -> R<Node> {
		let first = self.inversion()?;
		if !self.is_kw("and") {
			return Ok(first);
		}
		let mut values = vec![first];
		while self.eat_kw("and") {
			values.push(self.inversion()?);
		}
		Ok(Node::BoolOp { and: true, values })
	}

	fn inversion(&mut self) -> R<Node> {
		if self.eat_kw("not") {
			self.deeper()?;
			let operand = self.inversion();
			self.depth -= 1;
			return Ok(Node::UnaryOp { op: "Not", operand: Box::new(operand?) });
		}
		self.comparison()
	}

	fn comparison(&mut self) -> R<Node> {
		let left = self.bitwise_or()?;
		let mut ops = Vec::new();
		let mut comparators = Vec::new();
		loop {
			let op = match self.peek() {
				Tok::Op("==") => "Eq",
				Tok::Op("!=") => "NotEq",
				Tok::Op("<=") => "LtE",
				Tok::Op("<") => "Lt",
				Tok::Op(">=") => "GtE",
				Tok::Op(">") => "Gt",
				Tok::Name(w) if w == "in" => "In",
				Tok::Name(w) if w == "not" && matches!(self.peek2(), Tok::Name(n) if n == "in") => "NotIn",
				Tok::Name(w) if w == "is" => {
					if matches!(self.peek2(), Tok::Name(n) if n == "not") {
						"IsNot"
					} else {
						"Is"
					}
				}
				_ => break,
			};
			self.pos += if op == "NotIn" || op == "IsNot" { 2 } else { 1 };
			ops.push(op);
			comparators.push(self.bitwise_or()?);
		}
		if ops.is_empty() {
			return Ok(left);
		}
		Ok(Node::Compare { left: Box::new(left), ops, comparators })
	}

	fn binary(&mut self, ops: &[&str], next: fn(&mut Self) -> R<Node>) -> R<Node> {
		let mut left = next(self)?;
		let mut chain = 0;
		loop {
			let Tok::Op(op) = self.peek() else { break };
			let Some(op) = ops.iter().find(|o| **o == *op) else { break };
			let op = op_name(op);
			self.pos += 1;
			chain += 1;
			if chain > MAX_CHAIN {
				return Err(()); // Python runs out of stack long before
			}
			let right = next(self)?;
			left = Node::BinOp { left: Box::new(left), op, right: Box::new(right) };
		}
		Ok(left)
	}

	fn bitwise_or(&mut self) -> R<Node> {
		self.binary(&["|"], Self::bitwise_xor)
	}

	fn bitwise_xor(&mut self) -> R<Node> {
		self.binary(&["^"], Self::bitwise_and)
	}

	fn bitwise_and(&mut self) -> R<Node> {
		self.binary(&["&"], Self::shift)
	}

	fn shift(&mut self) -> R<Node> {
		self.binary(&["<<", ">>"], Self::sum)
	}

	fn sum(&mut self) -> R<Node> {
		self.binary(&["+", "-"], Self::term)
	}

	fn term(&mut self) -> R<Node> {
		self.binary(&["*", "/", "//", "%", "@"], Self::factor)
	}

	fn factor(&mut self) -> R<Node> {
		let op = match self.peek() {
			Tok::Op("+") => "UAdd",
			Tok::Op("-") => "USub",
			Tok::Op("~") => "Invert",
			_ => return self.power(),
		};
		self.pos += 1;
		self.deeper()?;
		let operand = self.factor();
		self.depth -= 1;
		Ok(Node::UnaryOp { op, operand: Box::new(operand?) })
	}

	fn power(&mut self) -> R<Node> {
		let base = if self.eat_kw("await") { Node::Await(Box::new(self.primary()?)) } else { self.primary()? };
		if self.eat("**") {
			self.deeper()?;
			let exponent = self.factor();
			self.depth -= 1;
			return Ok(Node::BinOp { left: Box::new(base), op: "Pow", right: Box::new(exponent?) });
		}
		Ok(base)
	}

	fn primary(&mut self) -> R<Node> {
		let col = self.col();
		let mut node = self.atom()?;
		let mut chain = 0;
		loop {
			chain += 1;
			if chain > MAX_CHAIN {
				return Err(());
			}
			if self.eat(".") {
				let attr = self.name()?;
				node = Node::Attribute { value: Box::new(node), attr, store: false };
			} else if self.is_op("(") {
				self.pos += 1;
				let (args, keywords) = self.call_args()?;
				node = Node::Call { func: Box::new(node), args, keywords, col };
			} else if self.eat("[") {
				let slice = self.slices()?;
				self.expect("]")?;
				node = Node::Subscript { value: Box::new(node), slice: Box::new(slice), store: false };
			} else {
				return Ok(node);
			}
		}
	}

	/// After '(': a call's arguments (or its one generator expression) and the ')'.
	fn call_args(&mut self) -> R<(Vec<Node>, Vec<(Option<String>, Node)>)> {
		let mut args = Vec::new();
		let mut keywords = Vec::new();
		#[derive(PartialEq)]
		enum State {
			Positional,
			Keywords,
			DoubleStarred,
		}
		let mut state = State::Positional;
		let mut first = true;
		while !self.is_op(")") {
			if self.eat("*") {
				if state == State::DoubleStarred {
					return Err(());
				}
				let value = self.expression()?;
				args.push(Node::Starred { value: Box::new(value), store: false });
			} else if self.eat("**") {
				let value = self.expression()?;
				keywords.push((None, value));
				state = State::DoubleStarred;
			} else if self.is_name() && matches!(self.peek2(), Tok::Op("=")) {
				let name = self.name()?;
				self.pos += 1;
				let value = self.expression()?;
				keywords.push((Some(name), value));
				if state == State::Positional {
					state = State::Keywords;
				}
			} else {
				if state != State::Positional {
					return Err(());
				}
				let value = self.named_expression()?;
				if self.is_op("=") {
					return Err(());
				}
				if first && (self.is_kw("for") || self.is_kw("async")) {
					let gens = self.for_if_clauses()?;
					self.expect(")")?;
					return Ok((vec![Node::GeneratorExp { elt: Box::new(value), gens }], vec![]));
				}
				args.push(value);
			}
			first = false;
			if !self.eat(",") {
				break;
			}
		}
		self.expect(")")?;
		Ok((args, keywords))
	}

	/// Inside '[...]' after a primary.
	fn slices(&mut self) -> R<Node> {
		let first = self.slice_item()?;
		if !self.is_op(",") {
			if matches!(first, Node::Starred { .. }) {
				return Ok(Node::Tuple { elts: vec![first], store: false });
			}
			return Ok(first);
		}
		let mut elts = vec![first];
		while self.eat(",") {
			if self.is_op("]") {
				break;
			}
			elts.push(self.slice_item()?);
		}
		Ok(Node::Tuple { elts, store: false })
	}

	fn slice_item(&mut self) -> R<Node> {
		if self.eat("*") {
			let value = self.expression()?;
			return Ok(Node::Starred { value: Box::new(value), store: false });
		}
		let mut lower = None;
		if !self.is_op(":") {
			let e = self.named_expression()?;
			if !self.is_op(":") {
				return Ok(e);
			}
			if matches!(e, Node::NamedExpr { .. }) {
				return Err(());
			}
			lower = Some(Box::new(e));
		}
		self.expect(":")?;
		let upper = if self.starts_expression() { Some(Box::new(self.expression()?)) } else { None };
		let mut step = None;
		if self.eat(":") && self.starts_expression() {
			step = Some(Box::new(self.expression()?));
		}
		Ok(Node::Slice { lower, upper, step })
	}

	fn atom(&mut self) -> R<Node> {
		let tok = self.peek().clone();
		match tok {
			Tok::Name(w) => match w.as_str() {
				"True" | "False" | "None" => {
					self.pos += 1;
					let value = match w.as_str() {
						"True" => Const::True,
						"False" => Const::False,
						_ => Const::None,
					};
					Ok(Node::Constant { value, u: false })
				}
				_ => {
					let id = self.name()?;
					Ok(Node::Name { id, store: false })
				}
			},
			Tok::Number(text) => {
				self.pos += 1;
				Ok(Node::Constant { value: number_const(&text).ok_or(())?, u: false })
			}
			Tok::Str(_) => self.strings(),
			Tok::Op("...") => {
				self.pos += 1;
				Ok(Node::Constant { value: Const::Ellipsis, u: false })
			}
			Tok::Op("(") => {
				self.pos += 1;
				self.deeper()?;
				let out = self.paren();
				self.depth -= 1;
				out
			}
			Tok::Op("[") => {
				self.pos += 1;
				self.deeper()?;
				let out = self.bracket();
				self.depth -= 1;
				out
			}
			Tok::Op("{") => {
				self.pos += 1;
				self.deeper()?;
				let out = self.brace();
				self.depth -= 1;
				out
			}
			_ => Err(()),
		}
	}

	fn paren(&mut self) -> R<Node> {
		if self.eat(")") {
			return Ok(Node::Tuple { elts: vec![], store: false });
		}
		if self.is_kw("yield") {
			let y = self.yield_expr()?;
			self.expect(")")?;
			return Ok(y);
		}
		let first = self.star_named_expression()?;
		if self.is_kw("for") || self.is_kw("async") {
			if matches!(first, Node::Starred { .. }) {
				return Err(());
			}
			let gens = self.for_if_clauses()?;
			self.expect(")")?;
			return Ok(Node::GeneratorExp { elt: Box::new(first), gens });
		}
		if self.eat(")") {
			if matches!(first, Node::Starred { .. }) {
				return Err(());
			}
			return Ok(first);
		}
		self.expect(",")?;
		let mut elts = vec![first];
		while !self.is_op(")") {
			elts.push(self.star_named_expression()?);
			if !self.eat(",") {
				break;
			}
		}
		self.expect(")")?;
		Ok(Node::Tuple { elts, store: false })
	}

	fn yield_expr(&mut self) -> R<Node> {
		self.pos += 1;
		if self.eat_kw("from") {
			return Ok(Node::YieldFrom(Box::new(self.expression()?)));
		}
		if self.starts_expression() || self.is_op("*") {
			return Ok(Node::Yield(Some(Box::new(self.star_expressions()?))));
		}
		Ok(Node::Yield(None))
	}

	fn bracket(&mut self) -> R<Node> {
		if self.eat("]") {
			return Ok(Node::List { elts: vec![], store: false });
		}
		let first = self.star_named_expression()?;
		if self.is_kw("for") || self.is_kw("async") {
			if matches!(first, Node::Starred { .. }) {
				return Err(());
			}
			let gens = self.for_if_clauses()?;
			self.expect("]")?;
			return Ok(Node::ListComp { elt: Box::new(first), gens });
		}
		let mut elts = vec![first];
		while self.eat(",") {
			if self.is_op("]") {
				break;
			}
			elts.push(self.star_named_expression()?);
		}
		self.expect("]")?;
		Ok(Node::List { elts, store: false })
	}

	fn brace(&mut self) -> R<Node> {
		if self.eat("}") {
			return Ok(Node::Dict { keys: vec![], values: vec![] });
		}
		if self.is_op("**") {
			return self.dict_rest(vec![], vec![]);
		}
		if self.is_op("*") {
			let first = self.star_named_expression()?;
			return self.set_rest(first);
		}
		let first = self.named_expression()?;
		if self.eat(":") {
			if matches!(first, Node::NamedExpr { .. }) {
				return Err(());
			}
			let value = self.expression()?;
			if self.is_kw("for") || self.is_kw("async") {
				let gens = self.for_if_clauses()?;
				self.expect("}")?;
				return Ok(Node::DictComp { key: Box::new(first), value: Box::new(value), gens });
			}
			if self.eat("}") {
				return Ok(Node::Dict { keys: vec![Some(first)], values: vec![value] });
			}
			self.expect(",")?;
			return self.dict_rest(vec![Some(first)], vec![value]);
		}
		if self.is_kw("for") || self.is_kw("async") {
			let gens = self.for_if_clauses()?;
			self.expect("}")?;
			return Ok(Node::SetComp { elt: Box::new(first), gens });
		}
		self.set_rest(first)
	}

	fn set_rest(&mut self, first: Node) -> R<Node> {
		let mut elts = vec![first];
		while self.eat(",") {
			if self.is_op("}") {
				break;
			}
			elts.push(self.star_named_expression()?);
		}
		self.expect("}")?;
		Ok(Node::Set(elts))
	}

	fn dict_rest(&mut self, mut keys: Vec<Option<Node>>, mut values: Vec<Node>) -> R<Node> {
		while !self.is_op("}") {
			if self.eat("**") {
				keys.push(None);
				values.push(self.bitwise_or()?);
			} else {
				keys.push(Some(self.expression()?));
				self.expect(":")?;
				values.push(self.expression()?);
			}
			if !self.eat(",") {
				break;
			}
		}
		self.expect("}")?;
		Ok(Node::Dict { keys, values })
	}

	fn for_if_clauses(&mut self) -> R<Vec<Comp>> {
		let mut gens = Vec::new();
		loop {
			let is_async = self.is_kw("async") && matches!(self.peek2(), Tok::Name(w) if w == "for");
			if is_async {
				self.pos += 1;
			}
			if !self.eat_kw("for") {
				break;
			}
			let target = self.star_targets()?;
			if !self.eat_kw("in") {
				return Err(());
			}
			let iter = self.disjunction()?;
			let mut ifs = Vec::new();
			while self.eat_kw("if") {
				ifs.push(self.disjunction()?);
			}
			gens.push(Comp { target, iter, ifs, is_async });
		}
		if gens.is_empty() {
			return Err(());
		}
		Ok(gens)
	}

	fn star_targets(&mut self) -> R<Node> {
		let first = self.star_target()?;
		if !self.is_op(",") {
			return Ok(first);
		}
		let mut elts = vec![first];
		while self.eat(",") {
			if !self.starts_target() {
				break;
			}
			elts.push(self.star_target()?);
		}
		Ok(Node::Tuple { elts, store: true })
	}

	fn starts_target(&self) -> bool {
		self.is_name() || matches!(self.peek(), Tok::Op("(" | "[" | "*")) || self.starts_expression()
	}

	fn star_target(&mut self) -> R<Node> {
		if self.is_op("*") && !matches!(self.peek2(), Tok::Op("*")) {
			self.pos += 1;
			let inner = self.star_target()?;
			return Ok(Node::Starred { value: Box::new(inner), store: true });
		}
		self.target_with_star_atom()
	}

	fn target_with_star_atom(&mut self) -> R<Node> {
		self.deeper()?;
		let out = self.target_inner();
		self.depth -= 1;
		out
	}

	fn target_inner(&mut self) -> R<Node> {
		// t_primary '.' NAME | t_primary '[' slices ']' (no further trailer)
		let save = self.pos;
		if let Ok(node) = self.primary() {
			match node {
				Node::Attribute { value, attr, .. } => return Ok(Node::Attribute { value, attr, store: true }),
				Node::Subscript { value, slice, .. } => return Ok(Node::Subscript { value, slice, store: true }),
				_ => {}
			}
		}
		self.pos = save;
		// star_atom
		if self.is_name() {
			let id = self.name()?;
			return Ok(Node::Name { id, store: true });
		}
		if self.eat("(") {
			if self.eat(")") {
				return Ok(Node::Tuple { elts: vec![], store: true });
			}
			let inner_save = self.pos;
			if let Ok(t) = self.target_with_star_atom() {
				if self.eat(")") {
					return Ok(t);
				}
			}
			self.pos = inner_save;
			let first = self.star_target()?;
			self.expect(",")?;
			let mut elts = vec![first];
			while !self.is_op(")") {
				elts.push(self.star_target()?);
				if !self.eat(",") {
					break;
				}
			}
			self.expect(")")?;
			return Ok(Node::Tuple { elts, store: true });
		}
		if self.eat("[") {
			let mut elts = Vec::new();
			while !self.is_op("]") {
				elts.push(self.star_target()?);
				if !self.eat(",") {
					break;
				}
			}
			self.expect("]")?;
			return Ok(Node::List { elts, store: true });
		}
		Err(())
	}

	/// One or more adjacent string literals as one constant (or f-string).
	fn strings(&mut self) -> R<Node> {
		let mut pieces: Vec<Node> = Vec::new();
		let mut any_bytes = false;
		let mut any_text = false;
		let mut any_f = false;
		while let Tok::Str(s) = self.peek().clone() {
			self.pos += 1;
			if s.bytes {
				any_bytes = true;
			} else {
				any_text = true;
			}
			match &s.fstring {
				Some(parts) => {
					any_f = true;
					pieces.push(Node::JoinedStr(self.fstring_values(parts, s.raw)?));
				}
				None => {
					let value = decode(&s.body, s.raw, s.bytes).ok_or(())?;
					pieces.push(Node::Constant { value, u: s.u });
				}
			}
		}
		if any_bytes && (any_text || any_f) {
			return Err(());
		}
		if any_bytes {
			let mut all = Vec::new();
			for p in &pieces {
				if let Node::Constant { value: Const::Bytes(b), .. } = p {
					all.extend_from_slice(b);
				}
			}
			return Ok(Node::Constant { value: Const::Bytes(all), u: false });
		}
		if !any_f && pieces.len() == 1 {
			return Ok(pieces.pop().unwrap());
		}
		let mut flat: Vec<Node> = Vec::new();
		for p in pieces {
			match p {
				Node::JoinedStr(values) => flat.extend(values),
				other => flat.push(other),
			}
		}
		let mut values: Vec<Node> = Vec::new();
		let mut k = 0;
		while k < flat.len() {
			if let Node::Constant { u, .. } = &flat[k] {
				let kind = *u;
				let mut text: Vec<u32> = Vec::new();
				while let Some(Node::Constant { value: Const::Str(s), .. }) = flat.get(k) {
					text.extend_from_slice(s);
					k += 1;
				}
				if any_f && text.is_empty() {
					continue;
				}
				values.push(Node::Constant { value: Const::Str(text), u: kind });
				continue;
			}
			values.push(flat[k].clone());
			k += 1;
		}
		if !any_f {
			return Ok(values.pop().unwrap_or(Node::Constant { value: Const::Str(vec![]), u: false }));
		}
		Ok(Node::JoinedStr(values))
	}

	/// An f-string's values: literal text and formatted fields.
	fn fstring_values(&mut self, parts: &[FPart], raw: bool) -> R<Vec<Node>> {
		let mut out = Vec::new();
		for part in parts {
			match part {
				FPart::Lit(text) => {
					let Some(Const::Str(s)) = decode(text, raw, false) else { return Err(()) };
					if !s.is_empty() {
						out.push(Node::Constant { value: Const::Str(s), u: false });
					}
				}
				FPart::Field(field) => {
					let mut sub = Parser { toks: &field.tokens, pos: 0, depth: self.depth };
					let value = if sub.is_kw("yield") { sub.yield_expr()? } else { sub.star_expressions()? };
					if !matches!(sub.peek(), Tok::End) {
						return Err(());
					}
					let conversion = match field.conversion.as_deref() {
						None => {
							if field.debug.is_some() && field.spec.is_none() {
								'r' as i64
							} else {
								-1
							}
						}
						Some(c @ ("s" | "r" | "a")) => c.as_bytes()[0] as i64,
						Some(_) => return Err(()),
					};
					let spec = match &field.spec {
						None => None,
						Some(parts) => {
							let values = self.fstring_values(parts, raw)?;
							Some(Box::new(Node::JoinedStr(merge_constants(values))))
						}
					};
					if let Some(debug) = &field.debug {
						out.push(Node::Constant { value: Const::Str(debug.chars().map(|c| c as u32).collect()), u: false });
					}
					out.push(Node::FormattedValue { value: Box::new(value), conversion, spec });
				}
			}
		}
		Ok(merge_constants(out))
	}
}

/// Adjacent string constants run together, as Python folds them.
fn merge_constants(values: Vec<Node>) -> Vec<Node> {
	let mut out: Vec<Node> = Vec::new();
	for v in values {
		if let (Some(Node::Constant { value: Const::Str(prev), .. }), Node::Constant { value: Const::Str(next), .. }) = (out.last_mut(), &v) {
			prev.extend_from_slice(next);
			continue;
		}
		out.push(v);
	}
	out
}

/// `ast.parse(text, mode="eval").body`, or None for a SyntaxError.
pub fn parse(text: &str) -> Option<Node> {
	let toks = tokenize(text)?;
	let mut p = Parser { toks: &toks, pos: 0, depth: 0 };
	let node = p.expressions().ok()?;
	while matches!(p.peek(), Tok::Newline) {
		p.pos += 1;
	}
	if !matches!(p.peek(), Tok::End) {
		return None;
	}
	Some(node)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn d(text: &str) -> String {
		parse(text).map(|n| dump(&n).unwrap()).unwrap_or_else(|| "SyntaxError".into())
	}

	#[test]
	fn dumps_like_python() {
		assert_eq!(d("1,"), "Tuple(elts=[Constant(value=1)], ctx=Load())");
		assert_eq!(d("lambda: 1"), "Lambda(args=arguments(), body=Constant(value=1))");
		assert_eq!(d("f(**a, b=1)"), "Call(func=Name(id='f', ctx=Load()), keywords=[keyword(value=Name(id='a', ctx=Load())), keyword(arg='b', value=Constant(value=1))])");
		assert_eq!(d("u'a' 'b'"), "Constant(value='ab', kind='u')");
		assert_eq!(d("f''"), "JoinedStr()");
		assert_eq!(d("0x1for x in y"), "BoolOp(op=Or(), values=[Constant(value=31), Compare(left=Name(id='x', ctx=Load()), ops=[In()], comparators=[Name(id='y', ctx=Load())])])");
		assert_eq!(d("1if 1else 2"), "IfExp(test=Constant(value=1), body=Constant(value=1), orelse=Constant(value=2))");
		assert_eq!(d("\u{ff4e}"), "Name(id='n', ctx=Load())");
		for bad in ["1<>2", "1__0", "07", "(*a)", "a := 1", "f(a=1, b)", "lambda *: 0", "1\n2", "1\n  ", "1 \\\n", "b'a' 'b'", "f'{}'", "f'}'"] {
			assert_eq!(d(bad), "SyntaxError", "{bad:?}");
		}
	}
}
