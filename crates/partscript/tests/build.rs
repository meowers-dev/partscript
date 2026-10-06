mod common;

use std::collections::{HashMap, HashSet};

use common::*;
use kitlib::json::Json;
use partscript::{BasicProvider, BuildOptions, Host, Project};

const PROPS: &str = "
prop stall \"Market Stall\" street
  use stall
prop cone_{c} \"Traffic Cone\" street for c=e25a1a,d8c020
  c 0,0,~ .18 .7 #{c}/plastic rt=.03 s=8
  b 0,0,.02 .4,.4,.04 rubber
prop pub_sign \"Pub Sign\"
  sign 0,-.03,2 1.6 .3 \"THE HOPE & ANCHOR\" sub=\"FREE HOUSE\"
  b 0,0,2 1.7,.04,.4 wood
prop lamp_post \"Lamp Post\"
  use post h=3
  use lamp 0,-.3,2.9
  b 0,-.15,2.95 .04,.3,.04 steel_dark
  part glass smooth=1
  sph 0,0,3.3 .1 glass_opaque s=8 rings=4
";

fn props(host: Host) -> Project {
	Project::from_text(PROPS, "test.parts", host)
}

#[test]
fn every_prop_builds_to_a_valid_glb() {
	let mut p = props(Host::default());
	for prop in p.props() {
		let built = p.build(&prop.id, &BuildOptions::default()).unwrap();
		let (document, blob) = read_glb(&built.glb);
		assert_eq!(document.get("asset").unwrap().get("version").unwrap().as_str(), "2.0");
		assert!(built.triangles() > 0);
		let accessors = document.get("accessors").unwrap().as_list();
		let mut indices = 0;
		for mesh in document.get("meshes").unwrap().as_list() {
			for prim in mesh.get("primitives").unwrap().as_list() {
				indices += accessors[prim.get("indices").unwrap().as_i64() as usize].get("count").unwrap().as_i64();
			}
		}
		assert_eq!(indices as usize, built.triangles() * 3);
		assert_eq!(blob.len() as i64, document.get("buffers").unwrap().idx(0).get("byteLength").unwrap().as_i64());
		assert!(built.warnings.is_empty(), "{:?}", built.warnings);
		let views = document.get("bufferViews").unwrap().as_list();
		for image in document.get("images").unwrap().as_list() {
			let view = &views[image.get("bufferView").unwrap().as_i64() as usize];
			let offset = view.get("byteOffset").unwrap().as_i64() as usize;
			assert_eq!(&blob[offset..offset + 8], b"\x89PNG\r\n\x1a\n");
		}
	}
}

#[test]
fn builds_are_deterministic() {
	let a = props(Host::default()).build("stall", &BuildOptions::default()).unwrap().glb;
	let b = props(Host::default()).build("stall", &BuildOptions::default()).unwrap().glb;
	assert_eq!(a, b);
}

#[test]
fn parts_become_nodes_under_the_asset() {
	let (document, _) = read_glb(&props(Host::default()).build("lamp_post", &BuildOptions::default()).unwrap().glb);
	let root_index = document.get("scenes").unwrap().idx(0).get("nodes").unwrap().idx(0).as_i64() as usize;
	let root = document.get("nodes").unwrap().idx(root_index);
	assert_eq!(root.get("name").unwrap().as_str(), "lamp_post");
	assert_eq!(root.get("children").unwrap().as_list().len(), 2);
}

#[test]
fn steps_tag_faces_with_their_statement() {
	let built = props(Host::default()).build("lamp_post", &BuildOptions { steps: true, ..Default::default() }).unwrap();
	assert_eq!(built.steps.iter().map(|s| s.stmt.op.as_str()).collect::<Vec<_>>(), vec!["use", "use", "b", "part", "sph"]);
	let (document, _) = read_glb(&built.glb);
	for mesh in document.get("meshes").unwrap().as_list() {
		for prim in mesh.get("primitives").unwrap().as_list() {
			assert!(prim.get("attributes").unwrap().get("TEXCOORD_1").is_some());
		}
	}
}

#[test]
fn textures_are_cached_on_disk() {
	let dir = std::env::temp_dir().join(format!("partscript-cache-{}", std::process::id()));
	let _ = std::fs::remove_dir_all(&dir);
	let mut first = props(Host::new(Box::new(BasicProvider::default()), "", Some(dir.clone())));
	first.write_all(&dir.join("out"), None, None);
	assert!(first.host.textures.borrow().made > 0);
	let mut second = props(Host::new(Box::new(BasicProvider::default()), "", Some(dir.clone())));
	second.write_all(&dir.join("out2"), None, None);
	assert_eq!(second.host.textures.borrow().made, 0);
	assert_eq!(std::fs::read(dir.join("out/stall.glb")).unwrap(), std::fs::read(dir.join("out2/stall.glb")).unwrap());
	let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_prop_with_errors_does_not_build() {
	let mut p = Project::from_text("prop bad\n  b 0 1 nowhere_material\n", "<text>", Host::default());
	assert!(p.build("bad", &BuildOptions::default()).is_err());
}

#[test]
fn buildings_export_placements() {
	let text = dedent(
		"
	prop wall_solid \"Wall\"
	  b 0,-.15,~ 4,.3,4 #a08060/plaster
	prop wall_door \"Door wall\"
	  b -1.5,-.15,~ 1,.3,4 #a08060/plaster mx
	prop slab \"Floor\"
	  b 0,0,-.1 4,4,.2 #707070/concrete
	kit cottage
	  wall solid=wall_solid door=wall_door
	  piece floor=slab roof=slab
	building hut \"Hut\" kit=cottage
	  room 0,0 2,1
	  open 0,0 s door
	",
	);
	let mut p = Project::from_text(&text, "<text>", Host::default());
	assert!(p.check().errors.is_empty(), "{:?}", p.check().errors);
	let data = p.building("hut").unwrap();
	assert_eq!(data.get("set").unwrap().as_str(), "cottage");
	let assets: HashSet<&str> = data.get("placements").unwrap().as_list().iter().map(|r| r.get("asset").unwrap().as_str()).collect();
	assert_eq!(assets, HashSet::from(["wall_solid", "wall_door", "slab"]));
	let kinds: Vec<&str> = data.get("openings").unwrap().as_list().iter().map(|o| o.get("kind").unwrap().as_str()).collect();
	assert_eq!(kinds, vec!["door"]);
}

const STOOLS: &str = "
def leg h=1
  b 0,0,~ .1,.1,h wood
prop stool_a \"Stool\"
  use leg .2,0,0 mx
  b 0,0,1 .5,.3,.05 wood
prop two_stools \"Two\"
  use stool_a -1,0,0
  use stool_a 1,0,0
";

#[test]
fn props_use_other_props_and_defs() {
	let mut p = Project::from_text(STOOLS, "<text>", Host::default());
	let report = p.check();
	assert!(report.errors.is_empty());
	assert_eq!(report.props.iter().map(|x| x.triangles).collect::<Vec<_>>(), vec![36, 72]);
	let two = p.build("two_stools", &BuildOptions::default()).unwrap().triangles();
	let one = p.build("stool_a", &BuildOptions::default()).unwrap().triangles();
	assert_eq!(two, 2 * one);
	let tree = p.uses("two_stools");
	let children = tree.get("children").unwrap().as_list();
	assert_eq!(
		children.iter().map(|c| (c.get("name").unwrap().as_str(), c.get("kind").unwrap().as_str())).collect::<Vec<_>>(),
		vec![("stool_a", "prop"), ("stool_a", "prop")]
	);
	let want = Json::parse(r#"{"name": "leg", "kind": "def", "file": "<text>", "line": 2, "count": 2, "at": 5, "children": []}"#).unwrap();
	assert_eq!(children[0].get("children").unwrap().idx(0), &want);
}

#[test]
fn faces_know_the_use_lines_they_came_through() {
	let built = Project::from_text(STOOLS, "t.parts", Host::default()).build("two_stools", &BuildOptions::default()).unwrap();
	let mut counts: HashMap<Vec<(String, usize)>, usize> = HashMap::new();
	for baked in &built.baked {
		for polygon in &baked.polygons {
			*counts.entry(built.origins[polygon.origin].clone()).or_insert(0) += 1;
		}
	}
	let t = |line: usize| ("t.parts".to_string(), line);
	let want: HashMap<Vec<(String, usize)>, usize> =
		HashMap::from([(vec![t(8)], 6), (vec![t(8), t(5)], 12), (vec![t(9)], 6), (vec![t(9), t(5)], 12)]);
	assert_eq!(counts, want);
}
