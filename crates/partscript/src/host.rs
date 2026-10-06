//! The host: what PartScript builds into. Materials, textures, and anything outside the .parts files.
//!
//! The language and compiler never look at the file system or a game's content directly; they ask the
//! host. The default Host is self-contained (a material library and textures from a texture provider).
//! A game embeds PartScript with its own provider (materials and pixels), its own asset-id prefix, and an
//! Embed for its existing assets (`use` can name them), its own props and its base kit for buildings.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use kitlib::geom::{Atlas, Mat, Materials, Part};

use crate::building::BaseKit;
use crate::textures::{BasicProvider, Recipe, TextureProvider, TextureStore};

/// What a game adds beyond materials: the world outside the .parts files.
pub trait Embed {
	/// asset id -> triangles, for built assets `use` may name (check counts them).
	fn known_assets(&self) -> HashMap<String, usize> {
		HashMap::new()
	}
	/// Prop names the game defines outside PartScript (a .parts prop of that name needs replace=1).
	fn native_props(&self) -> HashSet<String> {
		HashSet::new()
	}
	/// Parts of an asset outside the program that `use` and buildings may name.
	fn foreign_parts(&self, _name: &str, _materials: &Materials) -> Option<Vec<Part>> {
		None
	}
	/// Wall sets and pieces a kit can start from (kit NAME walls=SET).
	fn base_kit(&self) -> BaseKit {
		BaseKit::default()
	}
	/// The id a building's placement names for a piece (own: a prop of these files).
	fn placed_id(&self, asset_id: &str, _own: bool) -> String {
		asset_id.to_string()
	}
	/// Material words the check accepts beyond the host's own (None: accept any, they fail at build).
	fn accepts_any_material(&self) -> bool {
		false
	}
}

struct NoEmbed;
impl Embed for NoEmbed {}

pub struct Host {
	pub provider: Box<dyn TextureProvider>,
	/// asset ids are <prefix>_<name> when set
	pub prefix: String,
	pub textures: RefCell<TextureStore>,
	pub materials: Materials,
	pub embed: Box<dyn Embed>,
}

impl Default for Host {
	fn default() -> Self {
		Host::new(Box::new(BasicProvider::default()), "", None)
	}
}

impl Host {
	pub fn new(provider: Box<dyn TextureProvider>, prefix: &str, cache_dir: Option<PathBuf>) -> Host {
		let host = Host {
			provider,
			prefix: prefix.to_string(),
			textures: RefCell::new(TextureStore::new(cache_dir)),
			materials: Rc::new(RefCell::new(HashMap::new())),
			embed: Box::new(NoEmbed),
		};
		for (key, mat, recipe) in host.provider.library() {
			host.add_material(&key, mat, Some(recipe));
		}
		host
	}

	pub fn with_cache(cache_dir: Option<PathBuf>) -> Host {
		Host::new(Box::new(BasicProvider::default()), "", cache_dir)
	}

	pub fn with_embed(mut self, embed: Box<dyn Embed>) -> Host {
		self.embed = embed;
		self
	}

	/// The asset id of a prop name: <prefix>_<name>, or the name when there is no prefix.
	pub fn asset_id(&self, name: &str) -> String {
		if self.prefix.is_empty() || name.starts_with(&format!("{}_", self.prefix)) {
			name.to_string()
		} else {
			format!("{}_{name}", self.prefix)
		}
	}

	pub fn bare(&self, asset_id: &str) -> String {
		if self.prefix.is_empty() {
			asset_id.to_string()
		} else {
			asset_id.strip_prefix(&format!("{}_", self.prefix)).unwrap_or(asset_id).to_string()
		}
	}

	pub fn placed_id(&self, asset_id: &str, own: bool) -> String {
		self.embed.placed_id(asset_id, own)
	}

	/// Registers a material; recipe makes its texture when asked.
	pub fn add_material(&self, key: &str, mat: Mat, recipe: Option<Recipe>) {
		if let Some(recipe) = recipe {
			self.textures.borrow_mut().add(&mat.texture, recipe);
		}
		self.materials.borrow_mut().insert(key.to_string(), mat);
	}

	pub fn has_material(&self, key: &str) -> bool {
		self.materials.borrow().contains_key(key)
	}

	/// The material key a word in a .parts file names: <prefix>_<word>, then the word.
	pub fn find_material(&self, value: &str) -> Option<String> {
		let first = if self.prefix.is_empty() { value.to_string() } else { self.asset_id(value) };
		[first, value.to_string()].into_iter().find(|key| self.has_material(key))
	}

	/// Material words the check accepts (None: accept any; they fail at build).
	pub fn known_materials(&self) -> Option<HashSet<String>> {
		if self.embed.accepts_any_material() {
			return None;
		}
		Some(self.materials.borrow().keys().cloned().collect())
	}

	pub fn atlases(&self) -> Vec<Atlas> {
		self.provider.atlases()
	}

	pub fn known_assets(&self) -> HashMap<String, usize> {
		self.embed.known_assets()
	}

	pub fn native_props(&self) -> HashSet<String> {
		self.embed.native_props()
	}

	pub fn foreign_parts(&self, name: &str) -> Option<Vec<Part>> {
		self.embed.foreign_parts(name, &self.materials)
	}

	pub fn base_kit(&self) -> BaseKit {
		self.embed.base_kit()
	}

	/// The PNG of a texture by name (made once).
	pub fn texture_png(&self, name: &str) -> Option<Vec<u8>> {
		self.textures.borrow_mut().png(self.provider.as_ref(), name)
	}
}
