//! Textures: where the pixels come from, and a cache so each is made once.
//!
//! PartScript names textures; a TextureProvider makes them from recipes:
//!
//!   Surface { finish, hex }   a colour material's texture (#rrggbb/finish in a .parts file)
//!   Sign(spec)                a sign or label: text, sub line, colours, texture size
//!   Other(json)               anything else a provider knows (its library textures, decal sheets)
//!
//! The TextureStore asks the provider once per recipe and keeps the PNG, in memory and, given a cache
//! directory, on disk keyed by (provider id, provider version, recipe). BasicProvider is the open default:
//! flat colours with a little texture per finish, ordered dither and a palette quantize, and a pixel font.

use std::collections::HashMap;
use std::path::PathBuf;

use kitlib::geom::{Atlas, Mat};
use kitlib::gltf::encode_png;
use kitlib::hash::sha1_hex;
use kitlib::json::Json;
use kitlib::py;

use crate::npy::Generator;

/// What to make a texture from.
#[derive(Clone, Debug, PartialEq)]
pub enum Recipe {
	Surface { finish: String, hex: String },
	Sign(Json),
	Other(Json),
}

impl Recipe {
	pub fn to_json(&self) -> Json {
		match self {
			Recipe::Surface { finish, hex } => Json::List(vec!["surface".into(), finish.as_str().into(), hex.as_str().into()]),
			Recipe::Sign(spec) => Json::List(vec!["sign".into(), spec.clone()]),
			Recipe::Other(json) => json.clone(),
		}
	}
}

/// An image: uint8 rows, height x width x channels (3 or 4).
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
	pub width: usize,
	pub height: usize,
	pub channels: usize,
	pub pixels: Vec<u8>,
}

/// Makes textures. A game supplies its own to use its own materials and pixels.
pub trait TextureProvider {
	fn id(&self) -> &str;
	fn version(&self) -> &str;
	/// Materials every .parts file can name: key -> (Mat, recipe of its texture).
	fn library(&self) -> Vec<(String, Mat, Recipe)>;
	/// Decal sheets for the trim statement.
	fn atlases(&self) -> Vec<Atlas> {
		Vec::new()
	}
	/// The texture for a recipe.
	fn make(&self, recipe: &Recipe) -> Result<Image, String>;
}

/// Texture name -> recipe, and the PNG of each, made once.
pub struct TextureStore {
	pub cache_dir: Option<PathBuf>,
	pub recipes: HashMap<String, Recipe>,
	png: HashMap<String, Vec<u8>>,
	/// textures made this session (not served from a cache)
	pub made: usize,
}

impl TextureStore {
	pub fn new(cache_dir: Option<PathBuf>) -> TextureStore {
		TextureStore { cache_dir, recipes: HashMap::new(), png: HashMap::new(), made: 0 }
	}

	pub fn add(&mut self, name: &str, recipe: Recipe) {
		if self.recipes.get(name) != Some(&recipe) {
			self.recipes.insert(name.to_string(), recipe);
			self.png.remove(name);
		}
	}

	pub fn key(provider: &dyn TextureProvider, recipe: &Recipe) -> String {
		let text = Json::List(vec![provider.id().into(), provider.version().into(), recipe.to_json()]).dumps_sorted();
		sha1_hex(text.as_bytes())
	}

	/// The PNG of a named texture, or None when nothing knows how to make it.
	pub fn png(&mut self, provider: &dyn TextureProvider, name: &str) -> Option<Vec<u8>> {
		if let Some(data) = self.png.get(name) {
			return Some(data.clone());
		}
		let recipe = self.recipes.get(name)?.clone();
		let path = self.cache_dir.as_ref().map(|dir| dir.join(format!("{}.png", Self::key(provider, &recipe))));
		let data = match path.as_ref().and_then(|p| std::fs::read(p).ok()) {
			Some(data) => data,
			None => {
				let image = provider.make(&recipe).ok()?;
				let data = encode_png(&image.pixels, image.width, image.height, image.channels, 6);
				self.made += 1;
				if let Some(path) = &path {
					if let Some(parent) = path.parent() {
						let _ = std::fs::create_dir_all(parent);
					}
					let temp = path.with_extension("tmp");
					if std::fs::write(&temp, &data).is_ok() {
						let _ = std::fs::rename(&temp, path);
					}
				}
				data
			}
		};
		self.png.insert(name.to_string(), data.clone());
		Some(data)
	}
}

// ------------------------------------------------------------------ float32 image maths, as numpy does it
/// A size x size grid of 32-bit floats.
#[derive(Clone)]
struct Grid {
	size: usize,
	v: Vec<f32>,
}

impl Grid {
	fn filled(size: usize, value: f32) -> Grid {
		Grid { size, v: vec![value; size * size] }
	}
}

/// Smooth tileable noise, 0..1, size x size, cells lattice cells across (float64, as numpy promotes it).
fn value_noise(size: usize, cells: usize, seed: u64) -> Vec<f64> {
	let mut rng = Generator::new(seed);
	let lattice: Vec<f32> = (0..cells * cells).map(|_| rng.random_f32()).collect();
	let coords: Vec<f32> = (0..size).map(|k| (k as f32 * cells as f32) / size as f32).collect();
	let i0: Vec<usize> = coords.iter().map(|c| *c as i32 as usize).collect();
	let t: Vec<f64> = coords.iter().zip(&i0).map(|(c, i)| *c as f64 - *i as f64).collect();
	let t: Vec<f64> = t.iter().map(|t| t * t * (3.0 - 2.0 * t)).collect();
	let i1: Vec<usize> = i0.iter().map(|i| (i + 1) % cells).collect();
	let lat = |r: usize, c: usize| lattice[r * cells + c] as f64;
	let mut out = vec![0.0f64; size * size];
	for r in 0..size {
		for c in 0..size {
			let top = lat(i0[r], i0[c]) * (1.0 - t[c]) + lat(i0[r], i1[c]) * t[c];
			let bottom = lat(i1[r], i0[c]) * (1.0 - t[c]) + lat(i1[r], i1[c]) * t[c];
			out[r * size + c] = top * (1.0 - t[r]) + bottom * t[r];
		}
	}
	out
}

fn fbm(size: usize, cells: usize, octaves: usize, seed: u64) -> Grid {
	let mut total = Grid::filled(size, 0.0);
	let (mut weight, mut norm) = (1.0f64, 0.0f64);
	for octave in 0..octaves {
		let noise = value_noise(size, size.min(cells << octave), seed + octave as u64 * 7919);
		for (t, n) in total.v.iter_mut().zip(&noise) {
			*t = (*t as f64 + n * weight) as f32;
		}
		norm += weight;
		weight *= 0.5;
	}
	let norm = norm as f32;
	for t in total.v.iter_mut() {
		*t /= norm;
	}
	total
}

const BAYER4: [[f32; 4]; 4] = [[0.0, 8.0, 2.0, 10.0], [12.0, 4.0, 14.0, 6.0], [3.0, 11.0, 1.0, 9.0], [15.0, 7.0, 13.0, 5.0]];

/// Float RGB 0..255 -> uint8 on levels steps per channel, ordered (Bayer) dither between them.
fn quantize(rgb: &[f32], width: usize, height: usize, levels: usize, dither: f64) -> Vec<u8> {
	let step = (255.0 / (levels as f64 - 1.0)) as f32;
	let dither = dither as f32;
	let mut out = Vec::with_capacity(rgb.len());
	for r in 0..height {
		for c in 0..width {
			let threshold = (BAYER4[r % 4][c % 4] / 16.0 - 0.5) * dither;
			for k in 0..3 {
				let v = rgb[(r * width + c) * 3 + k];
				let q = (v / step + 0.5 + threshold).floor() * step;
				out.push(q.clamp(0.0, 255.0) as u8);
			}
		}
	}
	out
}

fn hex_rgb(hex: &str) -> [f32; 3] {
	[0, 1, 2].map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap_or(0) as f32)
}

// ------------------------------------------------------------------ pixel font (5 x 7)
const FONT: [(char, &str); 63] = [
	('A', "01110 10001 10001 11111 10001 10001 10001"), ('B', "11110 10001 10001 11110 10001 10001 11110"),
	('C', "01110 10001 10000 10000 10000 10001 01110"), ('D', "11110 10001 10001 10001 10001 10001 11110"),
	('E', "11111 10000 10000 11110 10000 10000 11111"), ('F', "11111 10000 10000 11110 10000 10000 10000"),
	('G', "01110 10001 10000 10111 10001 10001 01111"), ('H', "10001 10001 10001 11111 10001 10001 10001"),
	('I', "01110 00100 00100 00100 00100 00100 01110"), ('J', "00111 00010 00010 00010 00010 10010 01100"),
	('K', "10001 10010 10100 11000 10100 10010 10001"), ('L', "10000 10000 10000 10000 10000 10000 11111"),
	('M', "10001 11011 10101 10101 10001 10001 10001"), ('N', "10001 10001 11001 10101 10011 10001 10001"),
	('O', "01110 10001 10001 10001 10001 10001 01110"), ('P', "11110 10001 10001 11110 10000 10000 10000"),
	('Q', "01110 10001 10001 10001 10101 10010 01101"), ('R', "11110 10001 10001 11110 10100 10010 10001"),
	('S', "01111 10000 10000 01110 00001 00001 11110"), ('T', "11111 00100 00100 00100 00100 00100 00100"),
	('U', "10001 10001 10001 10001 10001 10001 01110"), ('V', "10001 10001 10001 10001 10001 01010 00100"),
	('W', "10001 10001 10001 10101 10101 10101 01010"), ('X', "10001 10001 01010 00100 01010 10001 10001"),
	('Y', "10001 10001 01010 00100 00100 00100 00100"), ('Z', "11111 00001 00010 00100 01000 10000 11111"),
	('0', "01110 10001 10011 10101 11001 10001 01110"), ('1', "00100 01100 00100 00100 00100 00100 01110"),
	('2', "01110 10001 00001 00010 00100 01000 11111"), ('3', "11111 00010 00100 00010 00001 10001 01110"),
	('4', "00010 00110 01010 10010 11111 00010 00010"), ('5', "11111 10000 11110 00001 00001 10001 01110"),
	('6', "00110 01000 10000 11110 10001 10001 01110"), ('7', "11111 00001 00010 00100 01000 01000 01000"),
	('8', "01110 10001 10001 01110 10001 10001 01110"), ('9', "01110 10001 10001 01111 00001 00010 01100"),
	(' ', "00000 00000 00000 00000 00000 00000 00000"), ('.', "00000 00000 00000 00000 00000 01100 01100"),
	(',', "00000 00000 00000 00000 01100 00100 01000"), ('!', "00100 00100 00100 00100 00100 00000 00100"),
	('?', "01110 10001 00001 00010 00100 00000 00100"), ('\'', "01100 00100 01000 00000 00000 00000 00000"),
	('-', "00000 00000 00000 11111 00000 00000 00000"), ('+', "00000 00100 00100 11111 00100 00100 00000"),
	('&', "01100 10010 10100 01000 10101 10010 01101"), ('/', "00001 00010 00010 00100 01000 01000 10000"),
	(':', "00000 01100 01100 00000 01100 01100 00000"), ('#', "01010 01010 11111 01010 11111 01010 01010"),
	('(', "00010 00100 01000 01000 01000 00100 00010"), (')', "01000 00100 00010 00010 00010 00100 01000"),
	('%', "11000 11001 00010 00100 01000 10011 00011"), ('$', "00100 01111 10100 01110 00101 11110 00100"),
	('*', "00000 00100 10101 01110 10101 00100 00000"), ('=', "00000 00000 11111 00000 11111 00000 00000"),
	('@', "01110 10001 10111 10101 10111 10000 01111"), ('"', "01010 01010 00000 00000 00000 00000 00000"),
	('\u{0}', ""), ('\u{0}', ""), ('\u{0}', ""), ('\u{0}', ""), ('\u{0}', ""), ('\u{0}', ""), ('\u{0}', ""),
];

/// A mask: rows of booleans.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
	pub width: usize,
	pub height: usize,
	pub bits: Vec<bool>,
}

fn glyph(ch: char) -> Option<Mask> {
	let rows = FONT.iter().find(|(c, rows)| *c == ch && !rows.is_empty())?.1;
	let bits: Vec<bool> = rows.split(' ').flat_map(|row| row.chars().map(|c| c == '1')).collect();
	Some(Mask { width: 5, height: 7, bits })
}

/// A 7-pixel-high mask of text in the pixel font (unknown characters are a ?, spaces blank).
pub fn text_mask(text: &str) -> Mask {
	let glyphs: Vec<Mask> =
		text.to_uppercase().chars().map(|ch| glyph(ch).unwrap_or_else(|| if crate::expr::py_isspace(ch) { glyph(' ').unwrap() } else { glyph('?').unwrap() })).collect();
	if glyphs.is_empty() {
		return Mask { width: 1, height: 7, bits: vec![false; 7] };
	}
	let width = glyphs.len() * 6 - 1;
	let mut bits = vec![false; width * 7];
	for (k, g) in glyphs.iter().enumerate() {
		for r in 0..7 {
			for c in 0..5 {
				bits[r * width + k * 6 + c] = g.bits[r * 5 + c];
			}
		}
	}
	Mask { width, height: 7, bits }
}

fn stamp(canvas: &mut [f32], cw: usize, ch: usize, mask: &Mask, colour: [f32; 3], centre_x: f64, top: i64, scale: usize) {
	let (width, height) = (mask.width * scale, mask.height * scale);
	let x0 = py::round(centre_x - width as f64 / 2.0) as i64;
	let y0 = top;
	let (xs, ys) = ((-x0).max(0), (-y0).max(0));
	let (xe, ye) = ((width as i64).min(cw as i64 - x0), (height as i64).min(ch as i64 - y0));
	if xe <= xs || ye <= ys {
		return;
	}
	for y in ys..ye {
		for x in xs..xe {
			if mask.bits[(y as usize / scale) * mask.width + x as usize / scale] {
				let (cx, cy) = ((x0 + x) as usize, (y0 + y) as usize);
				canvas[(cy * cw + cx) * 3..(cy * cw + cx) * 3 + 3].copy_from_slice(&colour);
			}
		}
	}
}

/// text in lines no wider than room pixels at scale (None when a single word is wider).
fn wrap(text: &str, scale: usize, room: i64) -> Option<Vec<String>> {
	let mut lines: Vec<String> = Vec::new();
	for word in crate::lang::words(text) {
		let n = word.chars().count() as i64;
		if (6 * n - 1) * scale as i64 > room {
			return None;
		}
		let joined = lines.last().map(|l| (6 * (l.chars().count() as i64 + 1 + n) - 1) * scale as i64 <= room).unwrap_or(false);
		if joined {
			let last = lines.last_mut().unwrap();
			last.push(' ');
			last.push_str(word);
		} else {
			lines.push(word.to_string());
		}
	}
	Some(lines)
}

/// [(mask, top, scale, tone)] placing text and its sub line on a width x height label; None when nothing
/// fits (force: scale 1, lines cut at the edges).
pub fn sign_layout(text: &str, sub: &str, width: usize, height: usize, force: bool) -> Option<Vec<(Mask, i64, usize, f64)>> {
	for scale in (1..=8).rev() {
		let sub_scale = 1.max((scale * 3 + 2) / 5);
		let pad = 2 + (scale / 2).max(width.min(height) / 16);
		let (room_w, room_h) = (width as i64 - 2 * pad as i64, height as i64 - 2 * pad as i64);
		let mut main = wrap(text, scale, room_w);
		let mut subs = if sub.is_empty() { Some(vec![]) } else { wrap(sub, sub_scale, room_w) };
		if force && (main.is_none() || subs.is_none()) {
			main = Some(vec![text.to_string()]);
			subs = Some(if sub.is_empty() { vec![] } else { vec![sub.to_string()] });
		}
		let (Some(main), Some(subs)) = (main, subs) else { continue };
		let rows: Vec<(String, usize, f64)> =
			main.iter().map(|l| (l.clone(), scale, 1.0)).chain(subs.iter().map(|l| (l.clone(), sub_scale, 0.85))).collect();
		let heights: Vec<i64> = rows.iter().map(|(_, s, _)| 7 * *s as i64).collect();
		let mut gaps: Vec<i64> = rows.iter().skip(1).map(|(_, s, _)| *s as i64 + 1).collect();
		if !main.is_empty() && !subs.is_empty() {
			let k = main.len() - 1;
			gaps[k] = 2 * sub_scale as i64;
		}
		let block: i64 = heights.iter().sum::<i64>() + gaps.iter().sum::<i64>();
		if block > room_h && !force {
			continue;
		}
		let mut top = (height as i64 - block).div_euclid(2);
		let mut out = Vec::new();
		for (k, (line, s, tone)) in rows.iter().enumerate() {
			out.push((text_mask(line), top, *s, *tone));
			top += heights[k] + gaps.get(k).copied().unwrap_or(0);
		}
		return Some(out);
	}
	None
}

// ------------------------------------------------------------------ the open provider
const LIBRARY: [(&str, &str, &str, f64); 34] = [
	("wood", "wood", "8a5a32", 1.6),
	("crate_wood", "planks", "a07a48", 1.0),
	("steel_dark", "metal", "3a3d40", 2.4),
	("steel_grey", "metal", "7c8084", 2.4),
	("steel_white", "metal", "d8d8d0", 2.4),
	("steel_blue", "metal", "2f5a8a", 2.4),
	("steel_red", "metal", "9a2a22", 2.4),
	("steel_teal", "metal", "2a7a72", 2.4),
	("steel_olive", "metal", "5c6236", 2.4),
	("steel_green", "metal", "3a7a3a", 2.4),
	("steel_yellow", "metal", "d0a020", 2.4),
	("steel_orange", "metal", "d06a1a", 2.4),
	("steel_purple", "metal", "5a3a8a", 2.4),
	("brass", "metal", "b08a3a", 2.4),
	("rust", "rust", "8a4a22", 1.5),
	("rubber", "rubber", "2a2a2a", 2.4),
	("canvas", "fabric", "b0a080", 1.0),
	("leather", "fabric", "5a3a22", 0.8),
	("paper", "plaster", "e8e4d8", 1.0),
	("concrete", "concrete", "8c8a84", 3.2),
	("plaster", "plaster", "c8c0b0", 4.0),
	("brick", "brick", "8a3a2a", 3.0),
	("stone", "stone", "8a8478", 3.0),
	("hazard", "hazard", "e0b020", 1.0),
	("glass", "glass", "a8c8d8", 1.0),
	("glass_opaque", "glass", "4a6a7a", 1.0),
	("lamp_warm", "glow", "5a4628", 1.0),
	("lamp_cold", "glow", "3a4450", 1.0),
	("lamp_sodium", "glow", "3a3d40", 1.0),
	("lamp_red", "glow", "5a1410", 1.0),
	("lamp_green", "glow", "1a4a3a", 1.0),
	("lamp_amber", "glow", "3a3d40", 1.0),
	("screen", "glass", "1a2a22", 1.0),
	("screen_amber", "glass", "2a2218", 1.0),
];

fn library_mat(key: &str, tile: f64) -> Mat {
	let mut mat = Mat::new(key, tile);
	let glow = |m: &mut Mat, colour: [f64; 3], strength: f64| {
		m.emission_color = colour;
		m.emission_strength = strength;
		m.ao = false;
	};
	match key {
		"steel_dark" => mat.roughness = 0.6,
		"brass" => mat.roughness = 0.5,
		"glass" => {
			mat.roughness = 0.4;
			mat.ao = false;
			mat.alpha = 0.3;
		}
		"glass_opaque" => {
			mat.roughness = 0.15;
			mat.metallic = 0.4;
			mat.ao = false;
		}
		"lamp_warm" => glow(&mut mat, [1.0, 0.78, 0.5], 1.1),
		"lamp_cold" => glow(&mut mat, [0.8, 0.9, 1.0], 1.1),
		"lamp_sodium" => glow(&mut mat, [1.0, 0.55, 0.18], 1.3),
		"lamp_red" => glow(&mut mat, [1.0, 0.06, 0.03], 1.2),
		"lamp_green" => glow(&mut mat, [0.3, 1.0, 0.45], 1.1),
		"lamp_amber" => glow(&mut mat, [1.0, 0.55, 0.1], 1.2),
		"screen" => glow(&mut mat, [0.35, 0.95, 0.55], 1.6),
		"screen_amber" => glow(&mut mat, [1.0, 0.62, 0.2], 1.6),
		_ => {}
	}
	mat
}

/// The open default: every finish as a small quantized texture, signs in a pixel font, no decal sheets.
pub struct BasicProvider {
	pub levels: usize,
	pub size: usize,
}

impl Default for BasicProvider {
	fn default() -> Self {
		BasicProvider { levels: 24, size: 64 }
	}
}

impl TextureProvider for BasicProvider {
	fn id(&self) -> &str {
		"partscript.basic"
	}

	fn version(&self) -> &str {
		"2"
	}

	fn library(&self) -> Vec<(String, Mat, Recipe)> {
		LIBRARY
			.iter()
			.map(|(key, finish, colour, tile)| (key.to_string(), library_mat(key, *tile), Recipe::Surface { finish: finish.to_string(), hex: colour.to_string() }))
			.collect()
	}

	fn make(&self, recipe: &Recipe) -> Result<Image, String> {
		match recipe {
			Recipe::Surface { finish, hex } => Ok(self.surface(finish, hex)),
			Recipe::Sign(spec) => Ok(self.sign(spec)),
			Recipe::Other(json) => Err(format!("BasicProvider cannot make {} textures", json.dumps(false))),
		}
	}
}

impl BasicProvider {
	pub fn surface(&self, finish: &str, hex: &str) -> Image {
		let size = if finish == "glow" || finish == "glass" { 16 } else { self.size };
		let seed = u64::from_str_radix(hex, 16).unwrap_or(0) % 997;
		let base = hex_rgb(hex);
		let n = size * size;
		let mut shade = Grid::filled(size, 1.0);
		let xy = |i: usize| ((i % size) as f32, (i / size) as f32);
		let add_f32 = |shade: &mut Grid, delta: &dyn Fn(usize) -> f32| {
			for i in 0..n {
				shade.v[i] += delta(i);
			}
		};
		match finish {
			"paint" => {
				let f = fbm(size, 4, 3, seed);
				add_f32(&mut shade, &|i| (f.v[i] - 0.5) * 0.10);
			}
			"metal" => {
				let mut rng = Generator::new(seed);
				let streak: Vec<f32> = (0..size).map(|_| rng.random_f32()).collect();
				let f = fbm(size, 8, 2, seed);
				add_f32(&mut shade, &|i| (streak[i / size] - 0.5) * 0.08 + (f.v[i] - 0.5) * 0.06);
			}
			"plastic" => {
				let f = fbm(size, 4, 2, seed);
				add_f32(&mut shade, &|i| (f.v[i] - 0.5) * 0.05);
			}
			"rubber" => {
				let mut rng = Generator::new(seed);
				let noise: Vec<f32> = (0..n).map(|_| rng.random_f32()).collect();
				add_f32(&mut shade, &|i| (noise[i] - 0.5) * 0.10);
			}
			"fabric" => {
				let f = fbm(size, 4, 2, seed);
				for i in 0..n {
					let (x, y) = xy(i);
					let check = if ((x as i64) / 2 + (y as i64) / 2) % 2 == 0 { 0.06 } else { -0.06 };
					let delta = check + ((f.v[i] - 0.5) * 0.08) as f64;
					shade.v[i] = (shade.v[i] as f64 + delta) as f32;
				}
			}
			"wood" | "planks" => {
				let warp = fbm(size, 4, 3, seed);
				let fine = fbm(size, 16, 2, seed + 1);
				for i in 0..n {
					let (_, y) = xy(i);
					let w = warp.v[i] * 6.0;
					let angle = (y + w) * 0.9;
					let s = (angle as f64).sin() as f32;
					shade.v[i] += s * 0.08 + (fine.v[i] - 0.5) * 0.08;
				}
				if finish == "planks" {
					for i in 0..n {
						let (_, y) = xy(i);
						let cut = if (y as i64) % (size as i64 / 4) == 0 { 0.35 } else { 0.0 };
						shade.v[i] = (shade.v[i] as f64 - cut) as f32;
					}
				}
			}
			"plaster" | "concrete" => {
				let f = fbm(size, 4, 4, seed);
				let k = if finish == "plaster" { 0.10f32 } else { 0.16f32 };
				add_f32(&mut shade, &|i| (f.v[i] - 0.5) * k);
				if finish == "concrete" {
					let mut rng = Generator::new(seed);
					for i in 0..n {
						let speck = rng.random_f64() < 0.03;
						shade.v[i] = (shade.v[i] as f64 - if speck { 0.18 } else { 0.0 }) as f32;
					}
				}
			}
			"stone" | "brick" => {
				let brick = finish == "brick";
				let rows = (size / if brick { 8 } else { 4 }) as i64;
				let width = rows * if brick { 2 } else { 1 };
				let mut rng = Generator::new(seed);
				let jitter: Vec<f32> = (0..n).map(|_| rng.random_f32()).collect();
				let f = fbm(size, 8, 2, seed);
				for i in 0..n {
					let (x, y) = xy(i);
					let (xi, yi) = (x as i64, y as i64);
					let course = yi / rows;
					let offset = if course % 2 == 1 { width / 2 } else { 0 };
					let column = (xi + offset) / width;
					let tint = jitter[(course as usize % size) * size + (column as usize % size)];
					shade.v[i] += (tint - 0.5) * 0.16 + (f.v[i] - 0.5) * 0.10;
					let mortar = yi % rows == 0 || (xi + offset) % width == 0;
					if mortar {
						shade.v[i] = if brick { 0.55 } else { 0.7 };
					}
				}
			}
			"rust" => {
				let f = fbm(size, 4, 4, seed);
				add_f32(&mut shade, &|i| (f.v[i] - 0.5) * 0.35);
			}
			"hazard" => {
				let f = fbm(size, 4, 2, seed);
				let mut rgb = vec![0f32; n * 3];
				for i in 0..n {
					let (x, y) = xy(i);
					let stripe = (((x + y) as i64) / (size as i64 / 4)) % 2 == 0;
					let colour = if stripe { base } else { [28.0, 28.0, 28.0] };
					let k = 1.0 + (f.v[i] - 0.5) * 0.08;
					for c in 0..3 {
						rgb[i * 3 + c] = colour[c] * k;
					}
				}
				return Image { width: size, height: size, channels: 3, pixels: quantize(&rgb, size, size, self.levels, 1.0) };
			}
			"glass" => {
				for i in 0..n {
					let (x, y) = xy(i);
					let w = if (x - y).abs() < 2.0 { 0.12 } else { 0.0 };
					shade.v[i] = (shade.v[i] as f64 + w) as f32;
				}
			}
			_ => {}
		}
		let mut rgb = vec![0f32; n * 3];
		for i in 0..n {
			for c in 0..3 {
				rgb[i * 3 + c] = base[c] * shade.v[i];
			}
		}
		Image { width: size, height: size, channels: 3, pixels: quantize(&rgb, size, size, self.levels, 1.0) }
	}

	/// A sign or label: its text, then its sub line smaller, word-wrapped and centred with a margin at the
	/// biggest pixel size that fits; drawn at 2x or 4x (to 256 px) when even the smallest does not fit.
	pub fn sign(&self, spec: &Json) -> Image {
		let tex = spec.get("tex").map(|t| (t.idx(0).as_i64() as usize, t.idx(1).as_i64() as usize)).unwrap_or((256, 32));
		let (base_w, base_h) = tex;
		let text = spec.get("text").map(|t| t.as_str().to_string()).unwrap_or_default();
		let sub = spec.get("sub").map(|t| t.as_str().to_string()).unwrap_or_default();
		let mut layout = None;
		let (mut width, mut height) = (base_w, base_h);
		let mut broke = false;
		for mult in [1, 2, 4] {
			width = base_w * mult;
			height = base_h * mult;
			if mult > 1 && width.max(height) > 256 {
				broke = true;
				break;
			}
			layout = sign_layout(&text, &sub, width, height, false);
			if layout.is_some() {
				broke = true;
				break;
			}
		}
		if !broke {
			width = base_w * 4;
			height = base_h * 4;
		}
		let layout = match layout {
			Some(l) => l,
			None => {
				width = width.min(256);
				height = height.min(256);
				sign_layout(&text, &sub, width, height, true).unwrap_or_default()
			}
		};
		let colour = |key: &str| -> [f32; 3] {
			spec.get(key).map(|c| [c.idx(0).as_f64() as f32, c.idx(1).as_f64() as f32, c.idx(2).as_f64() as f32]).unwrap_or([0.0; 3])
		};
		let (bg, fg) = (colour("bg"), colour("fg"));
		let mut canvas = vec![0f32; width * height * 3];
		for p in canvas.chunks_mut(3) {
			p.copy_from_slice(&bg);
		}
		let edge = bg.map(|c| c * 0.6);
		for r in [0, height - 1] {
			for c in 0..width {
				canvas[(r * width + c) * 3..(r * width + c) * 3 + 3].copy_from_slice(&edge);
			}
		}
		for r in 0..height {
			for c in [0, width - 1] {
				canvas[(r * width + c) * 3..(r * width + c) * 3 + 3].copy_from_slice(&edge);
			}
		}
		for (mask, top, scale, dim) in &layout {
			let colour = fg.map(|c| c * *dim as f32);
			stamp(&mut canvas, width, height, mask, colour, width as f64 / 2.0, *top, *scale);
		}
		Image { width, height, channels: 3, pixels: quantize(&canvas, width, height, self.levels, 0.0) }
	}
}
