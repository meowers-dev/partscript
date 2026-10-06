//! Rewrite PartScript between its terse form and its readable form, keeping layout and comments.
//!
//!   b 0,0,~ .4,.3,.2 wood r=0,0,45 *5@.3,0,0 mx
//!   box at=0,0,on size=.4,.3,.2 mat=wood turn=0,0,45 repeat 5 every .3,0,0 mirror x
//!
//! Only shape, use and group lines change; headers and everything a line says stay as they are.

use crate::expr::{py_isspace, py_strip};
use crate::lang::{alias, is_snake, parse_mod, parse_statement, signature, split_top, tokenize, Mod, CENTRED};

const WIDTH: usize = 120;
const TAIL: usize = 30;

pub fn readable(text: &str) -> String {
	rewrite(text, true)
}

pub fn terse(text: &str) -> String {
	rewrite(text, false)
}

fn long_op(op: &str) -> &str {
	match op {
		"b" => "box",
		"bb" => "bevel_box",
		"bx" => "box_between",
		"c" => "cylinder",
		"sph" => "sphere",
		"pan" => "panel",
		"ext" => "extrude",
		"at" => "group",
		"trim" => "decal",
		"archwall" => "arch_wall",
		other => other,
	}
}

fn short_opt(key: &str) -> &str {
	match key {
		"r" => "turn",
		"rt" => "top_radius",
		"ax" => "axis",
		"jit" => "jitter",
		"capm" => "cap_mat",
		other => other,
	}
}

const SIDED_OPS: [&str; 11] = ["c", "cone", "sph", "tube", "lathe", "pipe", "sweep", "vault", "archwall", "torus", "join"];

fn chars_len(s: &str) -> usize {
	s.chars().count()
}

fn rewrite(text: &str, long: bool) -> String {
	let lines: Vec<&str> = text.split('\n').collect();
	let joined = crate::lang::join_continued(&lines);
	joined
		.into_iter()
		.flatten()
		.map(|line| {
			let out = line_of(&line, long);
			if long {
				wrap(&out)
			} else {
				out
			}
		})
		.collect::<Vec<_>>()
		.join("\n")
}

/// A long statement line broken between words, each carried-on line indented under it.
fn wrap(line: &str) -> String {
	let code = crate::lang::strip_comment(line);
	if chars_len(line) <= WIDTH || code.contains(';') || code.contains('{') || code.contains('}') || code != line {
		return line.to_string();
	}
	let indent: String = line.chars().take_while(|c| py_isspace(*c)).collect();
	let words = words_of(py_strip(line));
	let mut rows: Vec<String> = Vec::new();
	let mut row = String::new();
	for word in words {
		if !row.is_empty() && chars_len(&indent) + chars_len(&row) + 1 + chars_len(&word) > WIDTH - 2 {
			rows.push(std::mem::take(&mut row));
			row = word;
		} else if row.is_empty() {
			row = word;
		} else {
			row = format!("{row} {word}");
		}
	}
	rows.push(row);
	while rows.len() > 1 && chars_len(rows.last().unwrap()) < TAIL {
		let tail = rows.pop().unwrap();
		let last = rows.last_mut().unwrap();
		last.push(' ');
		last.push_str(&tail);
	}
	format!("{indent}{}", rows.join(&format!(" \\\n{indent}    ")))
}

/// Words of a line, keeping quoted text and the words of a copies phrase together.
fn words_of(text: &str) -> Vec<String> {
	let tokens = tokenize(text).unwrap_or_default();
	let mut out = Vec::new();
	let mut k = 0;
	while k < tokens.len() {
		let word = tokens[k].as_str();
		let take = match word {
			"repeat" | "grid" | "ring" => 3,
			"scatter" => 5,
			"mirror" | "as" => 1,
			_ => 0,
		};
		if take > 0 && k + take < tokens.len() + 1 {
			let mut group: Vec<String> = tokens[k..(k + 1 + take).min(tokens.len())].to_vec();
			if word == "ring" && (group.len() < 3 || group[2] != "step") {
				group.truncate(2);
			}
			if (word == "repeat" || word == "grid") && (group.len() < 3 || group[2] != "every") {
				group.truncate(2);
			}
			if word == "scatter" && (group.len() < 5 || group[4] != "apart") {
				group.truncate(4);
			}
			k += group.len();
			out.push(group.join(" "));
		} else {
			out.push(word.to_string());
			k += 1;
		}
	}
	out
}

fn line_of(line: &str, long: bool) -> String {
	let code = crate::lang::strip_comment(line);
	let comment = &line[code.len()..];
	let stripped = py_strip(code);
	if stripped.is_empty() {
		return line.to_string();
	}
	let indent: String = code.chars().take_while(|c| py_isspace(*c)).collect();
	let pieces = crate::lang::split_statements(stripped);
	let mut out = Vec::new();
	for piece in &pieces {
		let opener = piece.ends_with('{');
		let mut body = if opener { py_strip(&piece[..piece.len() - 1]).to_string() } else { piece.clone() };
		let mut name = String::new();
		if let Some((left, right)) = body.split_once(" = ") {
			if is_snake(left) && !right.is_empty() {
				name = format!("{left} = ");
				body = right.to_string();
			}
		}
		let head = crate::lang::words(&body).first().map(|w| w.to_string()).unwrap_or_default();
		if !body.is_empty() && body != "}" && (signature(alias(&head)).is_some() || head == "stack") {
			if let Some(rewritten) = statement(&body, long) {
				body = rewritten;
			}
		}
		out.push(format!("{name}{body}{}", if opener { " {" } else { "" }));
	}
	let mut joined = String::new();
	for (k, piece) in out.iter().enumerate() {
		if k == 0 {
			joined = piece.clone();
		} else if out[k - 1].ends_with('{') || piece == "}" {
			joined.push(' ');
			joined.push_str(piece);
		} else {
			joined.push_str(" ; ");
			joined.push_str(piece);
		}
	}
	let trailing = &code[code.trim_end_matches(py_isspace).len()..];
	format!("{indent}{joined}{}{comment}", if comment.is_empty() { "" } else { trailing })
}

fn quote(value: &str) -> String {
	if value.contains(' ') || value.is_empty() {
		format!("\"{value}\"")
	} else {
		value.to_string()
	}
}

fn on_word(value: &str) -> String {
	split_top(value, ',')
		.iter()
		.map(|p| {
			if p == "~" {
				"on".to_string()
			} else if let Some(rest) = p.strip_prefix('~') {
				format!("on({rest})")
			} else {
				p.clone()
			}
		})
		.collect::<Vec<_>>()
		.join(",")
}

fn bare(count: &str) -> String {
	if count.starts_with('(') && count.ends_with(')') && count.len() > 2 && count[1..count.len() - 1].chars().all(crate::lang::is_word_char) {
		count[1..count.len() - 1].to_string()
	} else {
		count.to_string()
	}
}

fn modifiers(mods: &[String], facing: &str) -> Vec<String> {
	let mut out = Vec::new();
	let mut mirrors = String::new();
	for m in mods {
		if matches!(m.as_str(), "mx" | "my" | "mz") {
			mirrors.push_str(&m[1..]);
			continue;
		}
		match parse_mod(m) {
			Some(Mod::Repeat(counts, step)) => {
				let mut without = String::new();
				let mut depth = 0;
				for c in counts.chars() {
					match c {
						'(' => depth += 1,
						')' => depth -= 1,
						_ if depth == 0 => without.push(c),
						_ => {}
					}
				}
				let grid = without.contains('x');
				let text = if grid {
					format!("grid {}", split_top(&counts, 'x').iter().map(|c| bare(c)).collect::<Vec<_>>().join("x"))
				} else {
					format!("repeat {}", bare(&counts))
				};
				out.push(match step {
					Some(s) => format!("{text} every {s}"),
					None => text,
				});
			}
			Some(Mod::Ring(count, step)) => {
				let default = format!("(360/{count})");
				out.push(format!("ring {}{}", bare(&count), if step == default { String::new() } else { format!(" step {step}") }));
			}
			Some(Mod::On(count, spec)) => {
				let (target, gap) = spec.split_once(',').unwrap_or((&spec, ""));
				let mut text = format!("scatter {} on {target}", bare(&count));
				if !facing.is_empty() {
					text.push_str(&format!(" facing {facing}"));
				}
				if !gap.is_empty() {
					text.push_str(&format!(" apart {gap}"));
				}
				out.push(text);
			}
			Some(Mod::Scatter(count, spec)) => {
				let (count, values) = (bare(&count), split_top(&spec, ','));
				if values.len() == 1 {
					out.push(format!("scatter {count} within {}", values[0]));
				} else if values.len() == 3 && (values[1] == "0" || values[1] == "0.0") {
					out.push(format!("scatter {count} within {} apart {}", values[0], values[2]));
				} else {
					out.push(format!("scatter {count} over {},{}{}", values[0], values[1], if values.len() > 2 { format!(" apart {}", values[2]) } else { String::new() }));
				}
			}
			None => out.push(m.clone()),
		}
	}
	if !mirrors.is_empty() {
		out.push(format!("mirror {mirrors}"));
	}
	out
}

fn statement(text: &str, long: bool) -> Option<String> {
	let stmt = parse_statement(text, "<fmt>", 0).ok()?;
	let word = crate::lang::words(text).first().map(|w| w.to_string()).unwrap_or_default();
	let op = stmt.op.as_str();
	let mut opts: Vec<(String, String)> = stmt.opts.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
	let pop = |opts: &mut Vec<(String, String)>, key: &str| -> Option<String> { opts.iter().position(|(k, _)| k == key).map(|i| opts.remove(i).1) };
	let has = |opts: &Vec<(String, String)>, key: &str| opts.iter().any(|(k, _)| k == key);
	if word == "label" {
		pop(&mut opts, "printed");
	}
	let sig = signature(op)?;
	let mut args = stmt.args.clone();
	if op == "row" {
		let mut words: Vec<String> = if args.first().map(String::as_str) == Some("z") { vec!["stack".into()] } else { std::iter::once("row".to_string()).chain(args.first().cloned()).collect() };
		let names = pop(&mut opts, "index");
		if has(&opts, "on") {
			if let Some(at) = opts.iter_mut().find(|(k, _)| k == "at") {
				if at.1.ends_with(",0") && split_top(&at.1, ',').len() == 3 {
					at.1.truncate(at.1.len() - 2);
				}
			}
		}
		let mut ordered = Vec::new();
		for key in ["on", "at"] {
			if let Some(v) = pop(&mut opts, key) {
				ordered.push((key.to_string(), v));
			}
		}
		ordered.extend(opts);
		for (key, value) in ordered {
			let name = if long { short_opt(&key).to_string() } else { key.clone() };
			words.push(format!("{name}={}", if key == "at" { on_word(&value) } else { quote(&value) }));
		}
		words.extend(if long { modifiers(&stmt.mods, "") } else { stmt.mods.clone() });
		if let Some(n) = names {
			words.push(format!("as {n}"));
		}
		return Some(words.join(" "));
	}
	let mut words: Vec<String> = vec![if word == "label" { "label".to_string() } else if long { long_op(op).to_string() } else { op.to_string() }];
	if long {
		let beam = has(&opts, "from") && has(&opts, "to") && matches!(op, "b" | "bb" | "c" | "cone") && args.first().map(String::as_str) == Some("0,0,0");
		for key in ["from", "to"] {
			if has(&opts, key) && op != "bx" {
				let v = pop(&mut opts, key).unwrap();
				words.push(format!("{key}={}", on_word(&v)));
			}
		}
		let lead: Vec<String> = match pop(&mut opts, "on") {
			Some(on) => vec![format!("on={on}")],
			None => vec![],
		};
		if beam {
			args[0] = String::new();
			if op == "c" || op == "cone" {
				args[2] = String::new();
			} else if args[1].starts_with("0,") {
				let across: Vec<String> = split_top(&args[1], ',')[1..].to_vec();
				let mut unique = across.clone();
				unique.dedup();
				args[1] = if across.iter().all(|a| *a == across[0]) { across[0].clone() } else { across.join(",") };
			}
		}
		if !lead.is_empty() && sig.contains(&"at") {
			let k = sig.iter().position(|s| *s == "at").unwrap();
			if k < args.len() {
				let parts = split_top(&args[k], ',');
				if args[k] == "0,0,~" || args[k] == "0,0,0" {
					args[k] = String::new();
				} else if parts.len() == 3 && (parts[2] == "~" || parts[2] == "0") {
					args[k] = parts[..2].join(",");
				}
			}
		}
		let at_origin = (CENTRED.contains(&op) && args.first().map(String::as_str) == Some("0,0,~"))
			|| (!CENTRED.contains(&op) && sig.first() == Some(&"at") && !matches!(op, "use" | "at" | "row" | "link") && args.first().map(String::as_str) == Some("0,0,0"));
		if at_origin {
			args[0] = String::new();
		}
		let fixed = if op == "use" { 1 } else { sig.len() };
		if op != "use" {
			words.extend(lead.iter().cloned());
		}
		for (key, value) in sig.iter().take(fixed).zip(args.iter()) {
			if value.is_empty() {
				continue;
			}
			if op == "use" && *key == "name" {
				words.push(value.clone());
				words.extend(lead.iter().cloned());
			} else if *key == "text" {
				words.push(format!("text={value}"));
			} else {
				words.push(format!("{key}={}", if matches!(*key, "at" | "from" | "to") { on_word(value) } else { value.clone() }));
			}
		}
		let mut rest: Vec<String> = args.iter().skip(fixed).cloned().collect();
		if op == "use" && !rest.is_empty() {
			if !rest[0].is_empty() {
				words.push(format!("at={}", on_word(&rest[0])));
			}
			rest.remove(0);
		}
		words.extend(rest);
	} else {
		words.extend(args.iter().cloned());
	}
	let names = if long { pop(&mut opts, "index") } else { None };
	let facing = if long && stmt.mods.iter().any(|m| m.contains('^')) { pop(&mut opts, "facing").unwrap_or_default() } else { String::new() };
	for (key, value) in &opts {
		let name = if long {
			if key == "s" {
				if SIDED_OPS.contains(&op) {
					"sides"
				} else if op == "use" || op == "at" {
					"scale"
				} else {
					key.as_str()
				}
			} else {
				short_opt(key)
			}
		} else {
			key.as_str()
		};
		words.push(format!("{name}={}", quote(value)));
	}
	words.extend(if long { modifiers(&stmt.mods, &facing) } else { stmt.mods.clone() });
	if let Some(n) = names {
		words.push(format!("as {n}"));
	}
	Some(words.join(" "))
}
