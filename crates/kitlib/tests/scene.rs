//! glb_scene: node trees, rigid skins and animation clips with events.

use std::collections::HashMap;

use kitlib::anim::{quat_axis_angle, Channel, Clip, Event, Interp, Key, Track};
use kitlib::bake::{bake, Baked};
use kitlib::geom::{Mat, Part};
use kitlib::gltf::{glb_json, glb_scene, Scene, SceneMesh, SceneNode, Skin};
use kitlib::json::Json;

fn boxed(name: &str, centre: [f64; 3], size: [f64; 3], materials: &kitlib::geom::Materials) -> Baked {
	let mut part = Part::new(name, materials.clone());
	part.box_(centre, size, "grey", [0.0; 3], &[], 1.0, [1.0, 1.0], [0.0, 0.0]);
	bake(&part, 1.4, None)
}

fn grey() -> Option<Vec<u8>> {
	Some(kitlib::gltf::encode_png(&[128; 12], 2, 2, 3, 6))
}

/// An upper and a lower leg: two joints, one skinned mesh, a knee bend and a step event.
fn leg() -> (Vec<Baked>, Scene, HashMap<String, Mat>) {
	let mut library = HashMap::new();
	library.insert("grey".to_string(), Mat::new("grey", 2.0));
	let materials = std::rc::Rc::new(std::cell::RefCell::new(library.clone()));
	let baked = vec![boxed("thigh", [0.0, 0.0, 0.75], [0.1, 0.1, 0.5], &materials), boxed("calf", [0.0, 0.0, 0.25], [0.08, 0.08, 0.5], &materials)];
	let mut scene = Scene::default();
	scene.nodes.push(SceneNode::new("leg", None));
	let mut thigh = SceneNode::new("Thigh_L", Some(0));
	thigh.translation = [0.0, 0.0, 1.0];
	scene.nodes.push(thigh);
	let mut calf = SceneNode::new("Calf_L", Some(1));
	calf.translation = [0.0, 0.0, -0.5];
	scene.nodes.push(calf);
	let mut body = SceneNode::new("leg_mesh", Some(0));
	body.mesh = Some(SceneMesh::Skinned { name: "leg_mesh".into(), parts: vec![(0, 0), (1, 1)] });
	body.skin = Some(0);
	scene.nodes.push(body);
	scene.skins.push(Skin { name: "humanoid".into(), joints: vec![1, 2], skeleton: Some(1) });
	let mut walk = Clip::new("walk", 1.0);
	let bend = |deg: f64| quat_axis_angle([1.0, 0.0, 0.0], deg.to_radians());
	walk.tracks.push(Track {
		node: 2,
		channel: Channel::Rotation,
		interp: Interp::Linear,
		keys: vec![Key::quat(0.0, bend(0.0)), Key::quat(0.5, bend(40.0)), Key::quat(1.0, bend(0.0))],
	});
	walk.tracks.push(Track {
		node: 1,
		channel: Channel::Translation,
		interp: Interp::Step,
		keys: vec![Key::vec3(0.0, [0.0, 0.0, 1.0]), Key::vec3(0.5, [0.0, 0.0, 0.98]), Key::vec3(1.0, [0.0, 0.0, 1.0])],
	});
	walk.events.push(Event { t: 0.5, name: "footstep".into(), data: vec![("mark".into(), Json::Str("Foot_L".into()))] });
	walk.extras.push(("speed".into(), Json::Float(0.55)));
	scene.clips.push(walk);
	(baked, scene, library)
}

#[test]
fn a_skinned_leg_writes_joints_weights_and_inverse_binds() {
	let (baked, scene, materials) = leg();
	let (glb, missing) = glb_scene(&baked, &scene, &materials, &|_| grey(), false, "");
	assert!(missing.is_empty());
	if let Ok(path) = std::env::var("KITLIB_SCENE_OUT") {
		std::fs::write(path, &glb).unwrap();
	}
	let doc = Json::parse(&glb_json(&glb)).unwrap();
	let skin = doc.get("skins").unwrap().idx(0);
	assert_eq!(skin.get("joints").unwrap().as_list(), &[Json::Int(1), Json::Int(2)]);
	assert_eq!(skin.get("skeleton").unwrap().as_i64(), 1);
	let mesh = doc.get("meshes").unwrap().idx(0);
	let attributes = mesh.get("primitives").unwrap().idx(0).get("attributes").unwrap();
	let accessors = doc.get("accessors").unwrap().as_list();
	let joints = &accessors[attributes.get("JOINTS_0").unwrap().as_i64() as usize];
	assert_eq!((joints.get("componentType").unwrap().as_i64(), joints.get("type").unwrap().as_str()), (5123, "VEC4"));
	let ibm = &accessors[skin.get("inverseBindMatrices").unwrap().as_i64() as usize];
	assert_eq!((ibm.get("count").unwrap().as_i64(), ibm.get("type").unwrap().as_str()), (2, "MAT4"));
	assert!(ibm.get("bufferView").is_some() && doc.get("bufferViews").unwrap().idx(ibm.get("bufferView").unwrap().as_i64() as usize).get("target").is_none());
	// the calf joint sits 0.5 m up in glTF's Y: its inverse bind moves it back down
	let blob_offset = 20 + u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize + 8;
	let view = doc.get("bufferViews").unwrap().idx(ibm.get("bufferView").unwrap().as_i64() as usize);
	let start = blob_offset + view.get("byteOffset").unwrap().as_i64() as usize + 64;
	let ty = f32::from_le_bytes(glb[start + 52..start + 56].try_into().unwrap());
	assert!((ty + 0.5).abs() < 1e-6, "{ty}");
}

#[test]
fn clips_write_samplers_channels_and_events() {
	let (baked, scene, materials) = leg();
	let (glb, _) = glb_scene(&baked, &scene, &materials, &|_| grey(), false, "");
	let doc = Json::parse(&glb_json(&glb)).unwrap();
	let walk = doc.get("animations").unwrap().idx(0);
	assert_eq!(walk.get("name").unwrap().as_str(), "walk");
	let samplers = walk.get("samplers").unwrap().as_list();
	assert_eq!(samplers.iter().map(|s| s.get("interpolation").unwrap().as_str()).collect::<Vec<_>>(), vec!["LINEAR", "STEP"]);
	// both tracks key the same times: one input accessor, with the min and max glTF asks for
	assert_eq!(samplers[0].get("input"), samplers[1].get("input"));
	let input = doc.get("accessors").unwrap().idx(samplers[0].get("input").unwrap().as_i64() as usize);
	assert_eq!(input.get("max").unwrap().idx(0).as_f64(), 1.0);
	let targets: Vec<(i64, &str)> =
		walk.get("channels").unwrap().as_list().iter().map(|c| (c.get("target").unwrap().get("node").unwrap().as_i64(), c.get("target").unwrap().get("path").unwrap().as_str())).collect();
	assert_eq!(targets, vec![(2, "rotation"), (1, "translation")]);
	let extras = walk.get("extras").unwrap();
	assert_eq!(extras.dumps(true), r#"{"events":[{"t":0.5,"name":"footstep","mark":"Foot_L"}],"speed":0.55}"#);
}
