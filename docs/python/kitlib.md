# kitlib

kitlib is the geometry under PartScript, and usable without it: build parts in Python, bake them, write
`.glb`. Pure Python with no dependencies (PartScript's textures use numpy; kitlib does not).

```python
from kitlib.bake import bake
from kitlib.geom import Mat, Part
from kitlib.gltf import glb_bytes

materials = {"red": Mat("red"), "steel": Mat("steel", roughness=.9)}
part = Part("crate", materials)
part.box((0, 0, .25), (.5, .5, .5), "red")
part.push((0, 0, .5), (0, 0, .4))              # move and turn (radians) what comes next
part.cylinder((0, 0, .1), .1, .2, "steel", sides=8)
part.pop()

def textures(name):
    return None                                 # PNG bytes for a texture name; None draws it grey

data, missing = glb_bytes("crate", [bake(part)], materials, textures)
open("crate.glb", "wb").write(data)
```

## kitlib.geom

`Part(name, materials)` collects faces. Its drawing calls work in a local frame that `push(location,
rotation, scale)` (rotation in radians, XYZ) and `push_matrix(m)` nest and `pop()` undoes.

| | |
|---|---|
| `face(points, material, uv=None, shade=1, corner_shade=None)` | one polygon |
| `box(center, size, material, rotation=, skip=, taper=, lean=)` · `box_minmax(lo, hi, material)` | boxes |
| `bevel_box(center, size, bevel, material, ...)` | a chamfered box |
| `cylinder(center, radius, height, material, sides=10, radius_top=, caps=, arc=)` · `cone(...)` | round things |
| `lathe(profile, material, sides=16, center=, arc=, cap=)` | a profile `[(r, z)]` turned about z |
| `extrude(center, outline, width, material, axis="x", taper=1)` | an outline pushed through |
| `sweep(profile, path, material, closed_path=, closed_profile=, up=, twist=, scales=)` | a profile along a path |
| `panel(center, width, height, material, rotation=, uv=, double=)` · `trim(...)` | quads facing -Y |
| `triangle_count()` · `bounds()` | counting and measuring |

`Mat(texture, tile=2, emission="", emission_strength=0, emission_color=, roughness=1, metallic=0, ao=True,
alpha=1, alpha_clip=False, double_sided=False, tile_v=0)` describes a material
([fields](hosts.md#materials-kitlibgeommat)). `Face` is one polygon: `points`, `material`, `uv`, `shade`,
`corner_shade`, `group`, `origin`.

## kitlib.bake and kitlib.gltf

- `bake(part, ao_height=1.4, face_steps=None)` drops duplicate and degenerate faces, orders materials,
  works out UVs and the baked vertex tone (grounded shading, facing, per-face variation), and returns a
  dict ready to write.
- `glb_bytes(asset_id, baked_parts, materials, textures, steps=False)` returns `(bytes, missing textures)`;
  `write_glb(path, ...)` writes it.

## kitlib.maths

`Vector`, `Matrix` (3x3 and 4x4: `Identity`, `Translation`, `Rotation(angle, size, axis)`, `Diagonal`, `@`,
`inverted()`, `determinant()`) and `Euler`: the subset of Blender's mathutils the geometry needs, with its
conventions.

## kitlib.paths

| | |
|---|---|
| `path_frames(points, every=0, fit=False, corners=False, closed=False, joints=False)` | where pieces go down a polyline: `Frame(pos, yaw, stretch, segment)` |
| `smooth_points(points, steps, closed=False)` | a Catmull-Rom curve through points |
| `snap_markers(snaps, material_of, size=.06)` | a part with a marker per snap |

## kitlib.noise

`noise(x, y=0, z=0, seed="")` and `rough(x, y=0, z=0, seed="", octaves=4)` (0 to 1), and the raw
`perlin(...)` (about -1 to 1).

## kitlib.surface

| | |
|---|---|
| `SurfaceIndex(polygons, cell=.5)` | the up-facing triangles of some polygons, bucketed in plan |
| `.below(x, y, z)` | the surface at or below z over (x, y), else the lowest above; `(z, normal)` or `None` |
| `.heights(x, y)` · `.add(points)` | every surface over a point; add a polygon |
| `spread(polygons, count, rng, facing="any", apart=0)` | points over polygons by area, with normals |

## kitlib.ruin

`broken_box(part, size, material, amount, chunk, seed, core=None)` draws a box centred on the part's
origin with `amount` knocked out in chunks, and returns the fallen chunks as `[(centre, size)]`.
