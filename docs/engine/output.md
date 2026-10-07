# What the .glb holds

PartScript writes glTF 2.0 binary (`.glb`) itself, with no exporter in between. This page is what is in
it, for anyone importing the models into an engine or writing a viewer.

## Axes and units

Files are written Z up with +Y back (Blender's axes). The `.glb` is glTF's Y up with -Z forward: a point
`(x, y, z)` is written `(x, z, -y)`. A prop's front, -Y in a file, is +Z in the `.glb`. Units are metres.

## Nodes

- A prop with one mesh is one node, named after its asset id.
- A prop with several meshes (`part` lines, snap markers) is a node named after the asset id with a
  child node per mesh.
- With snaps on (`build(..., snaps=True)`), the snap markers are a child mesh named `snaps`.

A prop with [moving parts](../language/moving.md) (any `pivot=`, `parent=` or `mark`) is a node tree
instead, under a root node named after the asset id:

- each part is a node at its pivot (its `translation`, relative to its parent's pivot), hung under its
  parent's node or the root; its mesh's corners are written relative to the pivot;
- a part with a pivot but no shapes is a node with no mesh;
- each mark is an empty node (no mesh) at its point, with its turn as the node's `rotation`, under the
  part it is `on=` or the root.

Godot imports this as `Node3D`s and `MeshInstance3D`s named after the parts and marks, ready to move.

## Meshes and materials

Each mesh has a primitive per material. Corners are not shared between faces (flat shading), except in a
`part ... smooth=1`, whose normals are shared. Every material has:

- a `baseColorTexture`, sampled nearest (`NEAREST` / `NEAREST_MIPMAP_NEAREST`): hard pixels;
- `metallicFactor` and `roughnessFactor` from the material;
- for lit materials, `emissiveFactor` (with `KHR_materials_emissive_strength` above 1) and an emissive
  texture for signs;
- `alphaMode: BLEND` for see-through materials (glass), `MASK` for cut-outs, and `doubleSided` where asked.

Material names are the host's prefix and the material key (`zb_steel_dark`, `x8a6a42_wood` for a colour).

## Vertex attributes

| | |
|---|---|
| `POSITION`, `NORMAL` | as above, flat or smooth |
| `TEXCOORD_0` | texture coordinates (box-projected in metres over each material's tile size, or the shape's own) |
| `COLOR_0` | the baked tone, RGBA (grey), normalized 16-bit, linear: grounded shading, facing, per-face variation, `fade=` |
| `TEXCOORD_1` | only with `steps=True`: x is the source line's step index + .5, y the face's use-chain index + .5 (`Built.origins`) |

**Showing the baked tone.** The vertex colour multiplies the texture. For the PSX look, draw unlit:
texture times vertex colour (three.js: `MeshBasicMaterial({ map, vertexColors: true })`). In a lit
engine, use vertex colour as albedo (Godot's `vertex_color_use_as_albedo`) so the baked shading shows
under the engine's own lights.

Godot's runtime glTF loader leaves vertex-colour-as-albedo off on a mesh's first primitive, so a mesh can
name a `lead_material` (kitlib `Part.lead_material`) to put its least visible material first.

## Buildings

A building builds into one model like a prop (its pieces merged), and is also placement data (see the
[Rust API](api.md#buildings-as-data)), for a game that places its own copies of the pieces.
