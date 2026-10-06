//! Builds partscript.dev into site/:
//!
//!   cargo run -p site -- models     every example and std part as site/models/<id>.glb + index.json, and the
//!                                   playground's examples (the WebAssembly comes from build.sh)
//!   cargo run -p site -- docs       docs/**/*.md -> site/docs/**/*.html, search, the home page
//!   cargo run -p site -- docs --check   fail if a generated reference page is out of date
//!   cargo run -p site -- shots [names]  the gallery's pictures (docs/img/*.webp), headless, from a local preview
//!   cargo run -p site -- all        models and docs

mod docs;
mod models;
mod shots;

use std::path::PathBuf;
use std::process::ExitCode;

pub fn root() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn main() -> ExitCode {
	let args: Vec<String> = std::env::args().skip(1).collect();
	let result = match args.first().map(String::as_str) {
		Some("models") => models::build(),
		Some("docs") => docs::build(args.iter().any(|a| a == "--check")),
		Some("shots") => shots::build(&args[1..]),
		Some("all") => models::build().and_then(|_| docs::build(false)),
		_ => Err("usage: partscript-site models | docs [--check] | shots [names] | all".into()),
	};
	match result {
		Ok(()) => ExitCode::SUCCESS,
		Err(message) => {
			eprintln!("{message}");
			ExitCode::FAILURE
		}
	}
}
