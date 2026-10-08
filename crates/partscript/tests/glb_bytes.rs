//! Same file, same bytes: every example and golden prop writes exactly the .glb it wrote before
//! pivots, parents, marks, skins and animation came to the writer. The fixture holds a SHA-1 per build;
//! PARTSCRIPT_WRITE_GLB_HASHES=1 writes it (only ever from a commit before the change being checked).

use std::path::PathBuf;

use partscript::{BuildOptions, Host, Project};

fn root() -> PathBuf {
	PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn hashes() -> Vec<String> {
	let mut out = Vec::new();
	// the examples together (buildings furnish from the other files; they import the library), then the golden shapes
	let mut groups: Vec<Vec<PathBuf>> = vec![Vec::new()];
	let mut found: Vec<PathBuf> =
		std::fs::read_dir(root().join("examples")).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "parts")).collect();
	found.sort();
	groups[0].extend(found);
	groups.push(vec![root().join("tests/golden/shapes.parts")]);
	for group in groups {
		// files are named from the repository's root, as the CLI run there names them: rand() is seeded by the name
		let sources: Vec<(String, String)> = group
			.iter()
			.map(|p| (p.strip_prefix(root()).unwrap().to_string_lossy().replace('\\', "/"), std::fs::read_to_string(p).unwrap()))
			.collect();
		let reader = |target: &str| -> Vec<(String, String)> {
			std::fs::read_to_string(root().join(target)).map(|t| vec![(target.to_string(), t)]).unwrap_or_default()
		};
		let mut project = Project::new(sources, Host::default(), true, Some(&reader));
		let props: Vec<(String, String)> = project.props().into_iter().filter(|p| !p.imported).map(|p| (p.file, p.id)).collect();
		for (file, id) in props {
			for (kind, options) in [("plain", BuildOptions::default()), ("steps", BuildOptions { glb: true, steps: true, snaps: true, seed: None })] {
				let line = match project.build(&id, &options) {
					Ok(built) => kitlib::hash::sha1_hex(&built.glb),
					Err(e) => format!("error {}", e.message.lines().next().unwrap_or("")),
				};
				out.push(format!("{file} {id} {kind} {line}"));
			}
		}
	}
	out
}

#[test]
fn props_without_rigs_write_the_bytes_they_always_did() {
	let fixture = root().join("tests/reference/glb_bytes.txt");
	let now = hashes();
	if std::env::var("PARTSCRIPT_WRITE_GLB_HASHES").is_ok() {
		std::fs::write(&fixture, now.join("\n") + "\n").unwrap();
		return;
	}
	let before = std::fs::read_to_string(&fixture).unwrap();
	let before: Vec<&str> = before.lines().collect();
	assert_eq!(before.len(), now.len(), "a different number of builds");
	let changed: Vec<String> = before.iter().zip(&now).filter(|(a, b)| **a != b.as_str()).map(|(a, b)| format!("{a}\n  now {b}")).collect();
	assert!(changed.is_empty(), "{} builds changed:\n{}", changed.len(), changed.join("\n"));
}
