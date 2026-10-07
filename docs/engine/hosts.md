# Embedding: hosts and textures

PartScript never reads a game's files or textures itself. It asks a **host**: what materials there
are, how to make their textures, which outside assets `use` may name, what a building kit can start
from. The default `Host` is self-contained, with a material library and the `BasicProvider`'s textures. A
game or tool embeds PartScript by giving it a host of its own.

Native project imports can read files outside the initial source directory, including when using
`Project::from_text`. When accepting untrusted source, pass an explicit `Reader` to `Project::new`
that only supplies permitted files, and run the build in a separate process with time and memory
limits. The per-operation count limits do not bound total build cost. `Embed` and `TextureProvider`
implementations run as trusted code in the host process.

## Host

```rust,no_run
use partscript::{BasicProvider, Embed, Host, Project};

struct Game;
impl Embed for Game {
    fn native_props(&self) -> std::collections::HashSet<String> {
        ["crate".to_string()].into()
    }
}

let cache = Some("build/texture-cache".into());
let host = Host::new(Box::new(BasicProvider::default()), "zb", cache)
    .with_embed(Box::new(Game));
let project = Project::from_paths(&["content/props/"], host).unwrap();
```

| | |
|---|---|
| `Host::new(provider, prefix, cache_dir)` | textures from `provider`; asset ids `<prefix>_<name>`; a disk cache for textures (`None`: none) |
| `Host::default()` · `Host::with_cache(cache_dir)` | the `BasicProvider`, no prefix |
| `.with_embed(embed)` | the game's own world (below) |
| `asset_id(name)` · `bare(asset_id)` | a prop name to its asset id, and back |
| `add_material(key, mat, recipe)` | add a material (a kitlib `Mat`) and the recipe of its texture |
| `find_material(word)` | the material key a word in a file names (`<prefix>_<word>`, then the word) |
| `known_materials()` | the words `check` accepts (`None`: accept anything, fail at build) |
| `atlases()` | decal sheets for `decal` |
| `texture_png(name)` | a texture's PNG, made (or read from the cache) once |

A game's world beyond materials is an `Embed`; every method has a default, so implement only what you need:

| `Embed` method | |
|---|---|
| `known_assets()` | `{asset id: triangles}` of built assets that `use` may name (`check` counts them) |
| `foreign_parts(name, materials)` | kitlib `Part`s for an asset outside the `.parts` files (`use pack__id`), or `None` |
| `native_props()` | prop names the game defines elsewhere (a `.parts` prop of that name needs `replace=1`) |
| `base_kit()` | wall sets and pieces a `kit ... walls=SET` can start from |
| `placed_id(asset_id, own)` | the id a building's placement data names for a piece |
| `accepts_any_material()` | let `check` pass any material word (they fail at build if unknown) |

An embed that knows the game's own models answers `foreign_parts` with their faces, so
`use crates__pallet_stack` draws the game's own pallet into a prop.

## Texture providers

A provider makes textures from recipes and names the materials every file can use:

```rust,no_run
use kitlib::geom::Mat;
use partscript::{Host, Image, Recipe, TextureProvider};

struct FlatProvider;

impl TextureProvider for FlatProvider {
    // With version, keys the texture cache: change version when its output changes.
    fn id(&self) -> &str { "flat" }
    fn version(&self) -> &str { "1" }

    /// The materials every .parts file can name: (key, Mat, recipe of its texture).
    fn library(&self) -> Vec<(String, Mat, Recipe)> {
        let recipe = Recipe::Surface { finish: "wood".into(), hex: "8a6a42".into() };
        vec![("wood".into(), Mat::new("tex/wood", 1.0), recipe)]
    }

    /// An image, height x width x 3 (or 4) bytes, for a recipe.
    fn make(&self, recipe: &Recipe) -> Result<Image, String> {
        match recipe {
            Recipe::Surface { hex, .. } => {
                let byte = |k: usize| u8::from_str_radix(&hex[k..k + 2], 16).unwrap();
                let pixels = [byte(0), byte(2), byte(4)].repeat(16 * 16);
                Ok(Image { width: 16, height: 16, channels: 3, pixels })
            }
            Recipe::Sign(_) => {
                Ok(Image { width: 128, height: 32, channels: 3, pixels: vec![0; 128 * 32 * 3] })
            }
            Recipe::Other(json) => Err(format!("no texture for {}", json.dumps(false))),
        }
    }
}

let host = Host::new(Box::new(FlatProvider), "", None);
```

`atlases()` (decal sheets) has a default of none. Recipes are a `Recipe`:

| recipe | made for |
|---|---|
| `Surface { finish, hex }` | a colour material `#rrggbb/finish` (and the library's own) |
| `Sign(spec)` | a label or sign: `spec` (JSON) has `text`, `sub`, `bg`, `fg` (RGB), `lit`, `tex` (w, h), and for sleeves `wrapped`, `mark`, `accent` |
| `Other(json)` | the provider's own: library textures, decal sheets |

The `TextureStore` asks the provider once per recipe and keeps the PNG, in memory and, with a
cache directory, on disk keyed by `(provider id, version, recipe)`, so a rebuild only makes textures whose
recipe changed. `kitlib::gltf::encode_png(pixels, width, height, channels, level)` writes a PNG.

`BasicProvider { levels: 24, size: 64 }` (its default) is the open provider: every finish as a small texture with a little
grain, ordered dithering and a palette of `levels` steps, and signs in a pixel font.

## Materials: kitlib::geom::Mat

`Mat::new(texture, tile)` with the rest at their defaults; every field is public:

| field | |
|---|---|
| `texture` | the texture's name (what `TextureStore` makes and the `.glb` embeds) |
| `tile` · `tile_v` | metres per texture repeat (across, and up when different; `tile_v` 0 is the same) |
| `emission` · `emission_strength` · `emission_color` | lit: an emission texture, or a flat colour and strength |
| `roughness` (1) · `metallic` (0) | PBR factors written to the `.glb` |
| `ao` (true) | take the baked grounded shading |
| `alpha` (1) · `alpha_clip` · `double_sided` | see-through (BLEND), cut-out (MASK), both sides |

## Styles and dressing

`style` and `dressing` blocks are data for a host's room-dressing tools; PartScript checks them (every
piece they name must be a prop or one of the host's assets) and stores them on the program (`project.program.styles`,
`project.program.dressing`) but draws nothing from them.

```text
style workshop
  wall shelf repeat=1,2
  center workbench hero=1 around=stool count=2
  surface toolbox
  debris crate cluster=2,3
dressing clutter crate:0.4 barrel:0.35 sack
dressing signs pub=pub_sign shop=shop_sign
```

A style has a line per group of pieces, by layer (`wall corner center surface decor small debris`, and
`decals`), with options a dresser reads (`repeat=` `hero=1` `front=` `around=` `count=` `beside=` `sides=`
`above=` `cluster=` `stack=1` `surface=1` `once=1` `weight=` `gap=` `turn=`). A building's `room ... theme=STYLE`
names the style for its rooms in the placement data.
