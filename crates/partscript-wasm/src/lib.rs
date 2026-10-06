//! PartScript in the browser. The playground's worker calls build() with JSON in and gets JSON and the
//! model's .glb back, through this module's memory:
//!
//!   ptr = alloc(n); write the input's UTF-8 bytes there; build(ptr, n)
//!   read out_len() bytes at out_ptr(): a 4-byte little-endian JSON length, the JSON, then the .glb
//!
//! Input: {"sources": [[name, text], ...], "prop": "wanted", "library": {"../library/x.parts": text}}.
//! Output JSON: {"errors", "props", "prop", "triangles", "warnings"} as the playground shows them.

use std::cell::RefCell;
use std::collections::HashMap;

use kitlib::json::Json;
use partscript::{BuildOptions, Host, Project};

thread_local! {
	static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
	static LIBRARY: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
	let mut buffer = Vec::<u8>::with_capacity(len.max(1));
	let ptr = buffer.as_mut_ptr();
	std::mem::forget(buffer);
	ptr
}

/// # Safety
/// ptr and len must come from alloc().
#[no_mangle]
pub unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
	drop(Vec::from_raw_parts(ptr, 0, len.max(1)));
}

#[no_mangle]
pub extern "C" fn out_ptr() -> *const u8 {
	OUT.with(|o| o.borrow().as_ptr())
}

#[no_mangle]
pub extern "C" fn out_len() -> usize {
	OUT.with(|o| o.borrow().len())
}

/// # Safety
/// ptr and len must be bytes written into memory from alloc().
#[no_mangle]
pub unsafe extern "C" fn build(ptr: *const u8, len: usize) {
	let input = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).unwrap_or("");
	let (result, glb) = run(input);
	let text = result.dumps(false).into_bytes();
	let mut out = (text.len() as u32).to_le_bytes().to_vec();
	out.extend_from_slice(&text);
	out.extend_from_slice(&glb);
	OUT.with(|o| *o.borrow_mut() = out);
}

fn answer(errors: Vec<String>, props: Vec<String>) -> Json {
	let mut out = Json::dict();
	out.set("errors", errors).set("props", props);
	out
}

/// What the playground shows for one build.
pub fn run(input: &str) -> (Json, Vec<u8>) {
	let input = match Json::parse(input) {
		Ok(j) => j,
		Err(e) => return (answer(vec![e], vec![]), vec![]),
	};
	let sources: Vec<(String, String)> = input
		.get("sources")
		.map(|s| s.as_list().iter().map(|pair| (pair.idx(0).as_str().to_string(), pair.idx(1).as_str().to_string())).collect())
		.unwrap_or_default();
	if let Some(Json::Dict(library)) = input.get("library") {
		LIBRARY.with(|l| {
			let mut l = l.borrow_mut();
			for (k, v) in library {
				l.insert(k.clone(), v.as_str().to_string());
			}
		});
	}
	let wanted = input.get("prop").map(|p| p.as_str().to_string()).unwrap_or_default();
	let reader = |target: &str| LIBRARY.with(|l| l.borrow().get(target).map(|t| vec![(target.to_string(), t.clone())]).unwrap_or_default());
	let mut project = Project::new(sources.clone(), Host::default(), true, Some(&reader));
	let edited = sources.last().map(|s| s.0.clone()).unwrap_or_default();
	let props: Vec<String> = project.props().into_iter().filter(|p| p.file == edited).map(|p| p.id).collect();
	let errors = project.errors();
	if !errors.is_empty() {
		return (answer(errors, props), vec![]);
	}
	if props.is_empty() {
		return (answer(vec!["nothing to show: start a prop with  prop NAME \"Title\"".into()], vec![]), vec![]);
	}
	let name = if props.contains(&wanted) { wanted } else { props[0].clone() };
	let report = project.check();
	let built = match project.build(&name, &BuildOptions::default()) {
		Ok(b) => b,
		Err(e) => {
			let mut out = answer(vec![e.to_string()], props);
			out.set("prop", name);
			return (out, vec![]);
		}
	};
	let errors: Vec<String> = report.errors.into_iter().filter(|e| e.starts_with(&edited)).collect();
	let warnings: Vec<String> = built.warnings.iter().chain(&report.warnings).take(12).cloned().collect();
	let mut out = answer(errors, props);
	out.set("prop", name).set("triangles", built.triangles()).set("warnings", warnings);
	(out, built.glb)
}
