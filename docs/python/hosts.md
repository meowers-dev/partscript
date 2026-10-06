# Embedding: hosts and textures

PartScript never reads a game's files or textures itself. It asks a **host**: what materials there
are, how to make their textures, which outside assets `use` may name, what a building kit can start
from. The default `Host` is self-contained, with a material library and the `BasicProvider`'s textures. A
game or tool embeds PartScript by giving it a host of its own.

## Host

```python
from partscript import Host, Project

class GameHost(Host):
    def __init__(self):
        super().__init__(prefix="zb", cache_dir="build/texture-cache")

project = Project.from_paths(["content/props/"], GameHost())
```

| | |
|---|---|
| `Host(provider=None, prefix="", cache_dir=None)` | textures from `provider` (default `BasicProvider()`); asset ids `<prefix>_<name>`; a disk cache for textures |
| `asset_id(name)` · `bare(asset_id)` | a prop name to its asset id, and back |
| `add_material(key, mat, recipe=None)` | add a material (a kitlib `Mat`) and the recipe of its texture |
| `find_material(word)` | the material key a word in a file names (`<prefix>_<word>`, then the word) |
| `known_materials()` | the words `check` accepts (`None`: accept anything, fail at build) |
| `atlases()` | decal sheets for `decal` |
| `known_assets()` | `{asset id: {"triangles": n}}` of built assets that `use` may name (`check` counts them) |
| `foreign_parts(name)` | kitlib `Part`s for an asset outside the `.parts` files (`use pack__id`), or `None` |
| `native_props()` | prop names the game defines elsewhere (a `.parts` prop of that name needs `replace=1`) |
| `base_kit()` | wall sets and pieces a `kit ... walls=SET` can start from |
| `placed_id(asset_id, own)` | the id a building's placement data names for a piece |

Override what you need. A host that knows the game's own models answers `foreign_parts` with their
faces, so `use crates__pallet_stack` draws the game's own pallet into a prop.

## Texture providers

A provider makes textures from recipes and names the materials every file can use:

```python
import numpy as np
from kitlib.geom import Mat

class FlatProvider:
    id = "flat"           # with version, keys the texture cache: change version when its output changes
    version = "1"

    def library(self):
        """key -> (Mat, recipe): the materials every .parts file can name."""
        return {"wood": (Mat("tex/wood", tile=1.0), ("surface", "wood", "8a6a42"))}

    def atlases(self):
        return []

    def make(self, recipe):
        """A uint8 image, height x width x 3 (or 4), for a recipe."""
        if recipe[0] == "surface":
            colour = [int(recipe[2][k:k + 2], 16) for k in (0, 2, 4)]
            return np.full((16, 16, 3), colour, dtype=np.uint8)
        if recipe[0] == "sign":
            return np.zeros((32, 128, 3), dtype=np.uint8)
        raise ValueError(recipe)
```

Then `Host(provider=FlatProvider())`. Recipes are tuples:

| recipe | made for |
|---|---|
| `("surface", finish, "rrggbb")` | a colour material `#rrggbb/finish` (and the library's own) |
| `("sign", spec)` | a label or sign: `spec` has `text`, `sub`, `bg`, `fg` (RGB), `lit`, `tex` (w, h), and for sleeves `wrapped`, `mark`, `accent` |
| anything else | the provider's own: library textures, decal sheets |

The `TextureStore` asks the provider once per recipe and keeps the PNG, in memory and, with a
`cache_dir`, on disk keyed by `(provider id, version, recipe)`, so a rebuild only makes textures whose
recipe changed. `partscript.textures.encode_png(image)` writes a PNG without dependencies.

`BasicProvider(levels=24, size=64)` is the open default: every finish as a small texture with a little
grain, ordered dithering and a palette of `levels` steps, and signs in a pixel font.

## Materials: kitlib.geom.Mat

| field | |
|---|---|
| `texture` | the texture's name (what `TextureStore` makes and the `.glb` embeds) |
| `tile=2.0` · `tile_v` | metres per texture repeat (across, and up when different) |
| `emission` · `emission_strength` · `emission_color` | lit: an emission texture, or a flat colour and strength |
| `roughness=1` · `metallic=0` | PBR factors written to the `.glb` |
| `ao=True` | take the baked grounded shading |
| `alpha=1` · `alpha_clip` · `double_sided` | see-through (BLEND), cut-out (MASK), both sides |

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
