//! PartScript: low-poly props in a few words, compiled to .glb.
//!
//! A small language for building PSX-style 3D models. A [`Project`] holds `.parts` files (and the
//! standard parts), checks them, and builds each prop into baked kitlib parts and `.glb` bytes. A game
//! embeds it with its own [`Host`]: its materials and textures, its asset ids, its existing assets.

pub mod building;
pub mod check;
pub mod compiler;
pub mod expr;
pub mod fmt;
pub mod host;
pub mod lang;
mod npy;
pub mod ordered;
pub mod project;
pub mod reference;
pub mod textures;
pub mod value;

pub use check::{check, Report};
pub use expr::evaluate;
pub use host::{Embed, Host};
pub use lang::{parse, tokenize, PartScriptError, Program};
pub use project::{load_program, BuildOptions, Built, Project, PropInfo};
pub use reference::REFERENCE;
pub use textures::{BasicProvider, Image, Recipe, TextureProvider, TextureStore};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub use npy::Generator as NumpyGenerator;

/// The Rust examples in docs/engine/ compile (cargo test --doc).
#[cfg(doctest)]
mod engine_docs {
	#[doc = include_str!("../../../docs/engine/api.md")]
	struct Api;
	#[doc = include_str!("../../../docs/engine/hosts.md")]
	struct Hosts;
	#[doc = include_str!("../../../docs/engine/kitlib.md")]
	struct Kitlib;
}
