mod common;

use kitlib::gltf::{decode_png, encode_png};
use kitlib::json::Json;
use partscript::textures::{sign_layout, text_mask};
use partscript::{BasicProvider, BuildOptions, Host, Project, Recipe, TextureProvider, TextureStore};

fn noise_image(seed: u64, len: usize) -> Vec<u8> {
	let mut g = partscript::NumpyGenerator::new(seed);
	(0..len).map(|_| (g.next_u32() % 255) as u8).collect()
}

#[test]
fn png_round_trip() {
	let image = noise_image(1, 16 * 32 * 3);
	assert_eq!(decode_png(&encode_png(&image, 32, 16, 3, 6)).unwrap(), (32, 16, 3, image));
	let rgba = noise_image(2, 8 * 8 * 4);
	assert_eq!(decode_png(&encode_png(&rgba, 8, 8, 4, 6)).unwrap(), (8, 8, 4, rgba));
}

#[test]
fn every_finish_makes_a_power_of_two_texture() {
	let provider = BasicProvider::default();
	for finish in ["paint", "metal", "plastic", "rubber", "fabric", "wood", "plaster", "concrete", "stone", "glow", "glass", "brick", "planks", "rust", "hazard"] {
		let image = provider.make(&Recipe::Surface { finish: finish.into(), hex: "8a5a32".into() }).unwrap();
		assert_eq!(image.channels, 3);
		assert!(image.width == image.height && image.width & (image.width - 1) == 0, "{finish}");
	}
}

fn spec(text: &str, sub: &str, tex: (i64, i64), lit: f64) -> Json {
	let mut s = Json::dict();
	s.set("text", text).set("sub", sub).set("bg", vec![0i64, 0, 0]).set("fg", vec![255i64, 255, 255]).set("lit", lit).set("tex", vec![tex.0, tex.1]);
	s
}

#[test]
fn signs_draw_their_text() {
	let image = BasicProvider::default().make(&Recipe::Sign(spec("HI", "", (64, 32), 1.1))).unwrap();
	assert_eq!((image.height, image.width, image.channels), (32, 64, 3));
	let lit = image.pixels.chunks(3).filter(|p| *p.iter().max().unwrap() > 128).count();
	let layout = sign_layout("HI", "", 64, 32, false).unwrap();
	assert_eq!(layout.len(), 1);
	let scale = layout[0].2;
	assert_eq!(scale, 3); // the biggest that fits inside the margin
	assert_eq!(lit, text_mask("HI").bits.iter().filter(|b| **b).count() * scale * scale);
}

#[test]
fn store_makes_each_texture_once() {
	let provider = BasicProvider::default();
	let mut store = TextureStore::new(Some(common::scratch("store")));
	store.add("a", Recipe::Surface { finish: "metal".into(), hex: "336699".into() });
	store.add("b", Recipe::Surface { finish: "metal".into(), hex: "336699".into() });
	assert_eq!(store.png(&provider, "a"), store.png(&provider, "b"));
	assert_eq!(store.made, 1);
	assert!(store.png(&provider, "missing").is_none());
}

#[test]
fn sign_text_never_leaves_the_label() {
	let examples = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
	let mut project = Project::from_paths(&[examples], Host::default()).unwrap();
	for prop in project.props() {
		project.build(&prop.id, &BuildOptions::default()).unwrap();
	}
	let mut specs: Vec<Json> = project.host.textures.borrow().recipes.values().filter_map(|r| if let Recipe::Sign(s) = r { Some(s.clone()) } else { None }).collect();
	specs.push(spec("THE HOPE AND ANCHOR FREE HOUSE", "EST 1887  REAL ALES  GOOD FOOD", (64, 32), 1.0));
	specs.push(spec("TIMES", "EVERY 10 MIN", (64, 64), 0.0));
	let provider = BasicProvider::default();
	for s in specs {
		let image = provider.make(&Recipe::Sign(s.clone())).unwrap();
		let (text, sub) = (s.get("text").unwrap().as_str(), s.get("sub").unwrap().as_str());
		let layout = sign_layout(text, sub, image.width, image.height, false).unwrap_or_else(|| panic!("{text}"));
		let pad = 2;
		for (mask, top, scale, _) in layout {
			let line_width = mask.width * scale;
			assert!(line_width + 2 * pad <= image.width && top >= pad as i64 && top + 7 * scale as i64 <= image.height as i64 - pad as i64, "{text}");
		}
	}
}

#[test]
fn a_crowded_label_is_drawn_at_a_higher_resolution() {
	let image = BasicProvider::default().make(&Recipe::Sign(spec("TIMES", "EVERY 10 MIN", (32, 32), 0.0))).unwrap();
	assert_eq!((image.height, image.width), (64, 64));
}
