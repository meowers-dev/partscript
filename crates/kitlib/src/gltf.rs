//! A small glTF 2.0 binary (.glb) writer for baked parts.
//!
//! One mesh per part and a primitive per material; corners are not shared between faces (flat
//! shading); nearest-filtered textures; baked tone in COLOR_0. Positions go from the authoring space
//! (Z up, +Y back) to glTF's (Y up, -Z forward).

use std::collections::HashMap;

use crate::bake::{smooth_normals, Baked};
use crate::geom::Mat;
use crate::hash::crc32;
use crate::json::Json;
use crate::maths::{cross, normalized, sub};

const FLOAT: i64 = 5126;
const USHORT: i64 = 5123;
const UINT: i64 = 5125;
const ARRAY_BUFFER: i64 = 34962;
const ELEMENT_ARRAY_BUFFER: i64 = 34963;
const NEAREST: i64 = 9728;
const NEAREST_MIPMAP_NEAREST: i64 = 9984;

/// A baked tone as Blender's exporter writes it: 8-bit sRGB held back in linear at 16 bits.
pub fn colour_word(value: f64) -> u16 {
	let value = value.clamp(0.0, 1.0);
	let encoded = if value <= 0.0031308 { 12.92 * value } else { 1.055 * value.powf(1.0 / 2.4) - 0.055 };
	let stored = ((255.0 * encoded + 0.5) as i64) as f64 / 255.0;
	let linear = if stored <= 0.04045 { stored / 12.92 } else { ((stored + 0.055) / 1.055).powf(2.4) };
	(65535.0 * linear + 0.5) as i64 as u16
}

fn emission(spec: &Mat) -> Option<(Vec<f64>, Option<f64>)> {
	let factor: Vec<f64> = if !spec.emission.is_empty() {
		vec![spec.emission_strength; 3]
	} else if spec.emission_strength > 0.0 {
		spec.emission_color.iter().map(|c| c * spec.emission_strength).collect()
	} else {
		return None;
	};
	let peak = factor.iter().copied().fold(f64::NEG_INFINITY, f64::max);
	if peak > 1.0 {
		Some((factor.iter().map(|c| c / peak).collect(), Some(peak)))
	} else {
		Some((factor, None))
	}
}

enum Values {
	F32(Vec<f64>),
	U16(Vec<u16>),
	U32(Vec<u32>),
}

struct Glb<'a> {
	materials: &'a HashMap<String, Mat>,
	textures: &'a dyn Fn(&str) -> Option<Vec<u8>>,
	material_prefix: &'a str,
	json: Json,
	blob: Vec<u8>,
	material_index: HashMap<String, usize>,
	images: HashMap<String, usize>,
	missing: Vec<String>,
}

fn list_push(json: &mut Json, key: &str, value: Json) -> usize {
	match json.get_mut(key) {
		Some(Json::List(items)) => {
			items.push(value);
			items.len() - 1
		}
		_ => panic!("no list {key}"),
	}
}

impl Glb<'_> {
	fn view(&mut self, data: &[u8], target: Option<i64>) -> usize {
		while self.blob.len() % 4 != 0 {
			self.blob.push(0);
		}
		let mut view = Json::dict();
		view.set("buffer", 0i64).set("byteLength", data.len()).set("byteOffset", self.blob.len());
		if let Some(t) = target {
			view.set("target", t);
		}
		self.blob.extend_from_slice(data);
		list_push(&mut self.json, "bufferViews", view)
	}

	fn accessor(&mut self, values: Values, width: usize, kind: &str, target: i64, normalized: bool, bounds: bool) -> usize {
		let (bytes, component, count) = match &values {
			Values::F32(v) => (v.iter().flat_map(|x| (*x as f32).to_le_bytes()).collect::<Vec<u8>>(), FLOAT, v.len() / width),
			Values::U16(v) => (v.iter().flat_map(|x| x.to_le_bytes()).collect(), USHORT, v.len() / width),
			Values::U32(v) => (v.iter().flat_map(|x| x.to_le_bytes()).collect(), UINT, v.len() / width),
		};
		let view = self.view(&bytes, Some(target));
		let mut entry = Json::dict();
		entry.set("bufferView", view).set("componentType", component).set("count", count).set("type", kind);
		if normalized {
			entry.set("normalized", true);
		}
		if bounds && count > 0 {
			if let Values::F32(v) = &values {
				let mut lo = vec![f64::INFINITY; width];
				let mut hi = vec![f64::NEG_INFINITY; width];
				for item in v.chunks(width) {
					for k in 0..width {
						lo[k] = lo[k].min(item[k]);
						hi[k] = hi[k].max(item[k]);
					}
				}
				entry.set("min", Json::List(lo.iter().map(|x| Json::Float(*x as f32 as f64)).collect()));
				entry.set("max", Json::List(hi.iter().map(|x| Json::Float(*x as f32 as f64)).collect()));
			}
		}
		list_push(&mut self.json, "accessors", entry)
	}

	fn texture(&mut self, name: &str) -> usize {
		if let Some(&index) = self.images.get(name) {
			return index;
		}
		let data = match (self.textures)(name) {
			Some(d) => d,
			None => {
				self.missing.push(name.to_string());
				grey_png()
			}
		};
		let view = self.view(&data, None);
		let mut image = Json::dict();
		image.set("bufferView", view).set("mimeType", "image/png").set("name", name.rsplit('/').next().unwrap_or(name));
		let source = list_push(&mut self.json, "images", image);
		let mut texture = Json::dict();
		texture.set("sampler", 0i64).set("source", source);
		let index = list_push(&mut self.json, "textures", texture);
		self.images.insert(name.to_string(), index);
		index
	}

	fn material(&mut self, key: &str) -> usize {
		if let Some(&index) = self.material_index.get(key) {
			return index;
		}
		let spec = self.materials.get(key).cloned().unwrap_or_else(|| Mat::new("", 2.0));
		let base = self.texture(&spec.texture);
		let mut pbr = Json::dict();
		let mut tex = Json::dict();
		tex.set("index", base);
		pbr.set("baseColorTexture", tex).set("metallicFactor", spec.metallic);
		if spec.roughness != 1.0 {
			pbr.set("roughnessFactor", spec.roughness);
		}
		let mut entry = Json::dict();
		entry.set("name", format!("{}{}", self.material_prefix, key));
		let mut extras: Vec<(String, Json)> = Vec::new();
		if let Some((factor, strength)) = emission(&spec) {
			extras.push(("emissiveFactor".into(), Json::List(factor.into_iter().map(Json::Float).collect())));
			if !spec.emission.is_empty() {
				let index = self.texture(&spec.emission);
				let mut t = Json::dict();
				t.set("index", index);
				extras.push(("emissiveTexture".into(), t));
			}
			if let Some(strength) = strength {
				let mut inner = Json::dict();
				inner.set("emissiveStrength", strength);
				let mut ext = Json::dict();
				ext.set("KHR_materials_emissive_strength", inner);
				extras.push(("extensions".into(), ext));
				self.json.set("extensionsUsed", vec!["KHR_materials_emissive_strength"]);
			}
		}
		if spec.alpha_clip {
			extras.push(("alphaMode".into(), "MASK".into()));
		} else if spec.alpha < 1.0 {
			extras.push(("alphaMode".into(), "BLEND".into()));
			pbr.set("baseColorFactor", vec![1.0, 1.0, 1.0, spec.alpha]);
		}
		if spec.double_sided {
			extras.push(("doubleSided".into(), true.into()));
		}
		entry.set("pbrMetallicRoughness", pbr);
		for (k, v) in extras {
			entry.set(&k, v);
		}
		let index = list_push(&mut self.json, "materials", entry);
		self.material_index.insert(key.to_string(), index);
		index
	}

	fn mesh(&mut self, baked: &Baked, steps: bool) -> usize {
		let smooth = if baked.smooth { smooth_normals(&baked.polygons) } else { HashMap::new() };
		let mut primitives = Vec::new();
		for material in &baked.materials {
			let (mut positions, mut normals, mut uvs, mut colours, mut tags, mut indices) =
				(Vec::new(), Vec::new(), Vec::new(), Vec::<u16>::new(), Vec::new(), Vec::<u32>::new());
			for polygon in &baked.polygons {
				if &*polygon.face.material != material.as_str() {
					continue;
				}
				let base = (positions.len() / 3) as u32;
				let mut flat = polygon.normal;
				if !baked.smooth && polygon.folded {
					let t = polygon.triangles[0];
					let (a, b, c) = (polygon.coords[t[0]], polygon.coords[t[1]], polygon.coords[t[2]]);
					flat = normalized(cross(sub(b, a), sub(c, a)));
				}
				for (k, co) in polygon.coords.iter().enumerate() {
					let normal = if baked.smooth { smooth[&polygon.ids[k]] } else { flat };
					positions.extend([co[0], co[2], -co[1]]);
					normals.extend([normal[0], normal[2], -normal[1]]);
					uvs.extend([polygon.uvs[k][0], 1.0 - polygon.uvs[k][1]]);
					let word = colour_word(polygon.colours[k]);
					colours.extend([word, word, word, 65535]);
					tags.extend([polygon.step as f64 + 0.5, polygon.origin as f64 + 0.5]);
				}
				for triangle in &polygon.triangles {
					indices.extend(triangle.iter().map(|c| base + *c as u32));
				}
			}
			if positions.is_empty() {
				continue;
			}
			let count = positions.len() / 3;
			let mut attributes = Json::dict();
			attributes.set("POSITION", self.accessor(Values::F32(positions), 3, "VEC3", ARRAY_BUFFER, false, true));
			attributes.set("NORMAL", self.accessor(Values::F32(normals), 3, "VEC3", ARRAY_BUFFER, false, false));
			attributes.set("TEXCOORD_0", self.accessor(Values::F32(uvs), 2, "VEC2", ARRAY_BUFFER, false, false));
			attributes.set("COLOR_0", self.accessor(Values::U16(colours), 4, "VEC4", ARRAY_BUFFER, true, false));
			if steps {
				attributes.set("TEXCOORD_1", self.accessor(Values::F32(tags), 2, "VEC2", ARRAY_BUFFER, false, false));
			}
			let material_index = self.material(material);
			let wide = count > 65535;
			let index_values = if wide { Values::U32(indices) } else { Values::U16(indices.into_iter().map(|i| i as u16).collect()) };
			let indices_index = self.accessor(index_values, 1, "SCALAR", ELEMENT_ARRAY_BUFFER, false, false);
			let mut primitive = Json::dict();
			primitive.set("attributes", attributes).set("material", material_index).set("indices", indices_index);
			primitives.push(primitive);
		}
		let mut mesh = Json::dict();
		mesh.set("name", baked.name.as_str()).set("primitives", Json::List(primitives));
		list_push(&mut self.json, "meshes", mesh)
	}

	fn to_bytes(mut self) -> Vec<u8> {
		while self.blob.len() % 4 != 0 {
			self.blob.push(0);
		}
		let mut buffer = Json::dict();
		buffer.set("byteLength", self.blob.len());
		self.json.set("buffers", Json::List(vec![buffer]));
		let document = Json::Dict(self.json.as_dict().iter().filter(|(_, v)| !v.is_empty_container()).cloned().collect());
		let mut text = document.dumps(true).into_bytes();
		while text.len() % 4 != 0 {
			text.push(b' ');
		}
		let mut out = Vec::with_capacity(28 + text.len() + self.blob.len());
		out.extend_from_slice(b"glTF");
		out.extend_from_slice(&2u32.to_le_bytes());
		out.extend_from_slice(&((12 + 8 + text.len() + 8 + self.blob.len()) as u32).to_le_bytes());
		out.extend_from_slice(&(text.len() as u32).to_le_bytes());
		out.extend_from_slice(b"JSON");
		out.extend_from_slice(&text);
		out.extend_from_slice(&(self.blob.len() as u32).to_le_bytes());
		out.extend_from_slice(b"BIN\0");
		out.extend_from_slice(&self.blob);
		out
	}
}

/// PNG bytes of an image (uint8 RGB or RGBA rows), zlib-compressed.
pub fn encode_png(pixels: &[u8], width: usize, height: usize, channels: usize, level: u8) -> Vec<u8> {
	let colour_type = if channels == 4 { 6u8 } else { 2u8 };
	let mut rows = Vec::with_capacity(height * (width * channels + 1));
	for y in 0..height {
		rows.push(0);
		rows.extend_from_slice(&pixels[y * width * channels..(y + 1) * width * channels]);
	}
	let chunk = |kind: &[u8], data: &[u8]| -> Vec<u8> {
		let mut out = (data.len() as u32).to_be_bytes().to_vec();
		let mut body = kind.to_vec();
		body.extend_from_slice(data);
		out.extend_from_slice(&body);
		out.extend_from_slice(&crc32(&body).to_be_bytes());
		out
	};
	let mut header = Vec::new();
	header.extend_from_slice(&(width as u32).to_be_bytes());
	header.extend_from_slice(&(height as u32).to_be_bytes());
	header.extend_from_slice(&[8, colour_type, 0, 0, 0]);
	let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
	out.extend(chunk(b"IHDR", &header));
	out.extend(chunk(b"IDAT", &miniz_oxide::deflate::compress_to_vec_zlib(&rows, level)));
	out.extend(chunk(b"IEND", b""));
	out
}

/// A 2x2 mid-grey PNG standing in for a texture that could not be made.
fn grey_png() -> Vec<u8> {
	encode_png(&[128; 12], 2, 2, 3, 6)
}

/// (the .glb, textures it could not find): one mesh node, or a node named after the asset over one node
/// per part. textures(name) gives PNG bytes. steps adds TEXCOORD_1 = (step, origin) of each face.
pub fn glb_bytes(asset_id: &str, baked_parts: &[Baked], materials: &HashMap<String, Mat>, textures: &dyn Fn(&str) -> Option<Vec<u8>>,
	steps: bool, material_prefix: &str) -> (Vec<u8>, Vec<String>) {
	let mut json = Json::dict();
	let mut asset = Json::dict();
	asset.set("generator", "partscript").set("version", "2.0");
	let mut scene = Json::dict();
	scene.set("name", "Scene").set("nodes", Json::List(vec![]));
	let mut sampler = Json::dict();
	sampler.set("magFilter", NEAREST).set("minFilter", NEAREST_MIPMAP_NEAREST);
	json.set("asset", asset).set("scene", 0i64).set("scenes", Json::List(vec![scene])).set("nodes", Json::List(vec![]))
		.set("materials", Json::List(vec![])).set("meshes", Json::List(vec![])).set("textures", Json::List(vec![]))
		.set("images", Json::List(vec![])).set("samplers", Json::List(vec![sampler])).set("accessors", Json::List(vec![]))
		.set("bufferViews", Json::List(vec![])).set("buffers", Json::List(vec![]));
	let mut glb = Glb { materials, textures, material_prefix, json, blob: Vec::new(), material_index: HashMap::new(), images: HashMap::new(),
		missing: Vec::new() };
	let mut nodes = Vec::new();
	for baked in baked_parts {
		let mesh = glb.mesh(baked, steps);
		let mut node = Json::dict();
		node.set("mesh", mesh).set("name", baked.name.as_str());
		nodes.push(node);
	}
	let root = if nodes.len() == 1 {
		nodes[0].set("name", asset_id);
		0
	} else {
		let children: Vec<i64> = (0..nodes.len() as i64).collect();
		let mut node = Json::dict();
		node.set("name", asset_id).set("children", children);
		nodes.push(node);
		nodes.len() - 1
	};
	glb.json.set("nodes", Json::List(nodes));
	if let Some(Json::List(scenes)) = glb.json.get_mut("scenes") {
		scenes[0].set("nodes", vec![root]);
	}
	let missing = std::mem::take(&mut glb.missing);
	(glb.to_bytes(), missing)
}

/// The JSON chunk of a .glb.
pub fn glb_json(glb: &[u8]) -> String {
	let length = u32::from_le_bytes([glb[12], glb[13], glb[14], glb[15]]) as usize;
	String::from_utf8_lossy(&glb[20..20 + length]).trim_end_matches(' ').to_string()
}
