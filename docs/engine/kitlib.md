# kitlib

kitlib is the geometry under PartScript, and usable without it: build parts in Rust, bake them, write
`.glb`. A Rust crate whose only dependency is miniz_oxide (for PNG compression).

```rust,no_run
use std::cell::RefCell;
use std::collections::HashMap;
use std::f64::consts::TAU;
use std::rc::Rc;

use kitlib::bake::bake;
use kitlib::geom::{Mat, Part};
use kitlib::gltf::glb_bytes;

let mut steel = Mat::new("steel", 2.0);
steel.roughness = 0.9;
let materials = Rc::new(RefCell::new(HashMap::from([
    ("red".to_string(), Mat::new("red", 2.0)),
    ("steel".to_string(), steel),
])));
let mut part = Part::new("crate", materials.clone());
part.simple_box([0.0, 0.0, 0.25], [0.5, 0.5, 0.5], "red");
// Move and turn (radians) what comes next.
part.push_at([0.0, 0.0, 0.5], [0.0, 0.0, 0.4]);
// centre, radius, height, material, sides, rotation, top radius, caps, cap material, arc
part.cylinder([0.0, 0.0, 0.1], 0.1, 0.2, "steel", 8, [0.0; 3], None, true, None, [0.0, TAU]);
part.pop();

// PNG bytes for a texture name; None draws it grey.
let textures = |_name: &str| None;
let baked = [bake(&part, 1.4, None)];
let materials = materials.borrow();
let (data, _missing) = glb_bytes("crate", &baked, &materials, &textures, false, "");
std::fs::write("crate.glb", data).unwrap();
```

Points and vectors are `[f64; 3]` (`kitlib::maths::V3`).

## kitlib::geom

`Part::new(name, materials)` collects faces. Its drawing calls work in a local frame that
`push(location, rotation, scale)` and `push_at(location, rotation)` (rotation in radians, XYZ) and
`push_matrix(&m)` nest and `pop()` undoes. Every argument is positional; pass the defaults shown.

| | |
|---|---|
| `face(points, material, FaceSpec { uv, shade, corner_shade, .. })` · `plain(points, material)` | one polygon |
| `box_(center, size, material, rotation, skip, shade, taper, lean)` · `simple_box(center, size, material)` · `box_minmax(lo, hi, material)` | boxes (`skip`: sides to leave off, `taper`/`lean` `[1, 1]`/`[0, 0]` for none) |
| `bevel_box(center, size, bevel, material, rotation, taper, lean)` | a chamfered box |
| `cylinder(center, radius, height, material, sides, rotation, radius_top, caps, cap_material, arc)` · `cone(center, radius, height, material, sides, rotation)` | round things (`arc` in radians, `[0, TAU]` for whole) |
| `lathe(profile, material, sides, center, arc, cap, rotation)` | a profile `[[r, z]]` turned about z |
| `extrude(center, outline, width, material, axis, taper, rotation)` | an outline pushed through along `'x'`, `'y'` or `'z'` |
| `sweep(profile, path, material, closed_path, closed_profile, up, twist, scales)` | a profile along a path |
| `panel(center, width, height, material, rotation, uv, double)` · `trim(...)` | quads facing -Y |
| `triangle_count()` · `bounds()` | counting and measuring |

`Mat::new(texture, tile)` describes a material; its other fields are public with defaults
([fields](hosts.md#materials-kitlibgeommat)). `Face` is one polygon: `points`, `material`, `uv`, `shade`,
`corner_shade`, `group`, `origin`. A `Part` also has `smooth`, `uv_scale`, `ao_height` and
`lead_material`.

## kitlib::bake and kitlib::gltf

- `bake(&part, ao_height, face_steps)` (`1.4, None` as a rule) drops duplicate and degenerate faces,
  orders materials, works out UVs and the baked vertex tone (grounded shading, facing, per-face
  variation), and returns a `Baked` ready to write.
- `glb_bytes(asset_id, &baked_parts, &materials, &textures, steps, material_prefix)` returns
  `(bytes, missing textures)`.
- `glb_scene(&baked_parts, &scene, &materials, &textures, steps, material_prefix)` writes a `Scene`: its
  `nodes` in order (`SceneNode`: name, parent, translation, rotation, scale, and a `SceneMesh`: one
  baked part written from an origin, several skinned whole to joints, or a `WeightedMesh` of its own
  corners, each weighted to up to four joints, as an imported character's smooth skin), its `skins` (joints and the
  skeleton root; the inverse bind matrices come from the nodes' rest transforms) and its `clips`. Nodes
  with no parent are the scene's roots.

## kitlib::anim

What a `.glb` animates, already sampled: a `Clip` (name, length, `tracks`, `events`, more `extras`) holds
`Track`s, each the `Key`s (time and value) of one `Channel` (translation, rotation or scale) of one node,
filled between keys by an `Interp` (`Linear` or `Step`). Values are in the authoring axes, rotations as
quaternions (x, y, z, w); the writer turns them into glTF's. Events (`Event`: a time, a name and any data)
are written to the animation's `extras` as `{"events": [{"t": .4, "name": "eject_casing", ...}]}`.
Quaternion helpers: `quat_euler` (PartScript's `turn=` order), `quat_axis_angle`, `quat_mul`,
`quat_slerp`, `quat_rotate`, `quat_matrix` and `quat_from_matrix`.
- `encode_png(pixels, width, height, channels, level)` and `decode_png(bytes)` read and write the PNGs.

## kitlib::maths

`V3` (`[f64; 3]`) with `add`, `sub`, `scale`, `dot`, `cross`, `length`, `normalized`; `M4` (`identity`,
`translation`, `rotation_named(angle, 'x')`, `rotation_axis`, `diagonal`, `mul`, `point`, `inverted`,
`determinant`), `M3` and `euler(angles)`: Blender's mathutils conventions, the subset the geometry needs.

## kitlib::paths

| | |
|---|---|
| `path_frames(points, every, fit, corners, closed, joints)` | where pieces go down a polyline: `Frame { pos, yaw, stretch, segment }` |
| `smooth_points(points, steps, closed)` | a Catmull-Rom curve through points |
| `snap_markers(snaps, material_of, size)` | a part with a marker per snap (`Marker { pos, dir, kind }`) |

## kitlib::noise

`noise(x, y, z, seed)` and `rough(x, y, z, seed, octaves)` (0 to 1), and the raw `perlin(x, y, z, seed)`
(about -1 to 1).

## kitlib::surface

| | |
|---|---|
| `SurfaceIndex::new(polygons, cell)` | the up-facing triangles of some polygons, bucketed in plan |
| `.below(x, y, z)` | the surface at or below z over (x, y), else the lowest above; `Some((z, normal))` or `None` |
| `.heights(x, y)` · `.add(points)` | every surface over a point; add a polygon |
| `spread(polygons, count, &mut rng, facing, apart)` | points over polygons by area, with normals (`facing` `"any"`, `"up"` or `"side"`) |

## kitlib::ruin

`broken_box(&mut part, size, material, amount, chunk, seed, core)` draws a box centred on the part's
origin with `amount` knocked out in chunks, and returns the fallen chunks as `[(centre, size)]`.

## And the rest

`kitlib::json` (`Json`: what the APIs return as data), `kitlib::py` (`PyRandom`, the Mersenne Twister
every seeded draw uses), `kitlib::hash` (SHA-1, CRC-32).
