//! partscript: check and build .parts files.
//!
//!   partscript ref                       the language, in one page
//!   partscript check props/              parse and check every .parts file (no building)
//!   partscript build props/ -o out/      every prop as out/<id>.glb
//!   partscript list props/               props, std parts and materials
//!   partscript fmt props/a.parts -w      rewrite in words (--terse: in shorthand)

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use partscript::{Host, Project, REFERENCE};

const USAGE: &str = "usage: partscript {ref,check,build,list,fmt} ...

Low-poly props in a few words, compiled to .glb.

commands:
  ref                         the language reference
  check PATHS [--json]        parse and check, no building
  build PATHS [-o OUT] [--only A,B] [--seed S]
                              build every prop to .glb (default out/)
  list PATHS                  props, std parts and materials
  fmt FILES [-w] [--terse]    rewrite files in words (default) or in shorthand

options for check, build and list:
  --cache DIR                 texture cache directory (default ~/.cache/partscript/textures)
  --no-cache                  make every texture afresh
";

struct Args {
	command: String,
	paths: Vec<String>,
	cache: Option<String>,
	no_cache: bool,
	json: bool,
	out: String,
	only: Option<String>,
	seed: Option<String>,
	terse: bool,
	write: bool,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
	let mut args = Args {
		command: String::new(),
		paths: Vec::new(),
		cache: None,
		no_cache: false,
		json: false,
		out: "out".into(),
		only: None,
		seed: None,
		terse: false,
		write: false,
	};
	let mut it = argv.iter();
	args.command = it.next().cloned().ok_or("a command is required")?;
	if !["ref", "check", "build", "list", "fmt"].contains(&args.command.as_str()) {
		return Err(format!("invalid choice: '{}' (choose from 'ref', 'check', 'build', 'list', 'fmt')", args.command));
	}
	while let Some(arg) = it.next() {
		let mut value = |name: &str| it.next().cloned().ok_or(format!("argument {name}: expected one argument"));
		match (args.command.as_str(), arg.as_str()) {
			(_, "-h" | "--help") => return Err(String::new()),
			("check" | "build" | "list", "--cache") => args.cache = Some(value("--cache")?),
			("check" | "build" | "list", "--no-cache") => args.no_cache = true,
			("check", "--json") => args.json = true,
			("build", "-o" | "--out") => args.out = value("-o/--out")?,
			("build", "--only") => args.only = Some(value("--only")?),
			("build", "--seed") => args.seed = Some(value("--seed")?),
			("fmt", "--terse") => args.terse = true,
			("fmt", "--readable") => args.terse = false,
			("fmt", "-w" | "--write") => args.write = true,
			(_, flag) if flag.starts_with('-') && flag.len() > 1 => return Err(format!("unrecognized arguments: {flag}")),
			(_, path) => args.paths.push(path.to_string()),
		}
	}
	if args.command != "ref" && args.paths.is_empty() {
		return Err(format!("the following arguments are required: {}", if args.command == "fmt" { "files" } else { "paths" }));
	}
	Ok(args)
}

fn cache_dir(args: &Args) -> Option<PathBuf> {
	if args.no_cache {
		return None;
	}
	if let Some(c) = &args.cache {
		return Some(PathBuf::from(c));
	}
	let base = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
	Some(base.join("partscript").join("textures"))
}

fn project(args: &Args) -> Result<Project, String> {
	let missing: Vec<&str> = args.paths.iter().filter(|p| !Path::new(p).exists()).map(String::as_str).collect();
	if !missing.is_empty() {
		return Err(format!("no such file or directory: {}", missing.join(", ")));
	}
	Project::from_paths(&args.paths, Host::with_cache(cache_dir(args))).map_err(|e| e.to_string())
}

fn check(args: &Args) -> Result<bool, String> {
	let project = project(args)?;
	let report = project.check();
	if args.json {
		println!("{}", report.to_json().dumps_indent(1));
	} else {
		for prop in &report.props {
			println!("  {:40} ~{:6} tris  {}:{}", prop.id, prop.triangles, prop.file, prop.line);
		}
		for w in &report.warnings {
			println!("warning: {w}");
		}
		for e in &report.errors {
			println!("error: {e}");
		}
		println!("{} props, {} errors, {} warnings", report.props.len(), report.errors.len(), report.warnings.len());
	}
	Ok(report.errors.is_empty())
}

fn build(args: &Args) -> Result<bool, String> {
	let started = Instant::now();
	let mut project = project(args)?;
	if !project.errors().is_empty() {
		for e in project.errors() {
			println!("error: {e}");
		}
		return Ok(false);
	}
	let only: Option<Vec<String>> = args.only.as_ref().map(|o| o.split(',').map(str::to_string).collect());
	let result = project.write_all(Path::new(&args.out), only.as_deref(), args.seed.as_deref());
	for (id, triangles, seconds) in &result.built {
		println!("  {id:40} {triangles:6} tris  {:7.1} ms", seconds * 1000.0);
	}
	for w in &result.warnings {
		println!("warning: {w}");
	}
	for e in &result.errors {
		println!("error: {e}");
	}
	let made = project.host.textures.borrow().made;
	println!("{} props -> {} in {:.2} s ({made} textures made)", result.built.len(), args.out, started.elapsed().as_secs_f64());
	Ok(result.errors.is_empty())
}

fn list(args: &Args) -> Result<bool, String> {
	let project = project(args)?;
	for prop in project.props() {
		println!("  {:8} {:40} {}", prop.kind, prop.id, prop.title);
	}
	let mut std: Vec<&String> = project.program.std.iter().collect();
	std.sort();
	println!("std parts: {}", std.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
	let mut materials: Vec<String> = project.host.materials.borrow().keys().cloned().collect();
	materials.sort();
	println!("materials: {}", materials.join(", "));
	Ok(true)
}

fn fmt(args: &Args) -> Result<bool, String> {
	for file in &args.paths {
		let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
		let out = if args.terse { partscript::fmt::terse(&text) } else { partscript::fmt::readable(&text) };
		if args.write {
			if out != text {
				std::fs::write(file, &out).map_err(|e| format!("{file}: {e}"))?;
				println!("rewrote {file}");
			}
		} else if out.ends_with('\n') {
			print!("{out}");
		} else {
			println!("{out}");
		}
	}
	Ok(true)
}

fn main() -> ExitCode {
	let argv: Vec<String> = std::env::args().skip(1).collect();
	let args = match parse_args(&argv) {
		Ok(a) => a,
		Err(message) if message.is_empty() => {
			print!("{USAGE}");
			return ExitCode::SUCCESS;
		}
		Err(message) => {
			eprint!("{USAGE}");
			eprintln!("partscript: error: {message}");
			return ExitCode::from(2);
		}
	};
	let result = match args.command.as_str() {
		"ref" => {
			println!("{REFERENCE}");
			Ok(true)
		}
		"check" => check(&args),
		"build" => build(&args),
		"list" => list(&args),
		_ => fmt(&args),
	};
	match result {
		Ok(true) => ExitCode::SUCCESS,
		Ok(false) => ExitCode::from(1),
		Err(message) => {
			eprintln!("{message}");
			ExitCode::from(1)
		}
	}
}
