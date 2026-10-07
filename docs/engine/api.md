# Rust API

Everything the command line does is a few calls on `partscript::Project`. Add the crate from the
repository (`cargo add partscript --git https://github.com/meowers-dev/partscript`).

```rust,no_run
use partscript::{BuildOptions, Host, Project};

// Every .parts file under props/, plus the std parts.
let mut project = Project::from_paths(&["props/"], Host::default()).unwrap();
// The static check: no building.
for error in &project.check().errors {
    println!("{error}");
}
// One prop: parts, steps, triangles, .glb bytes.
let built = project.build("street_lamp", &BuildOptions::default()).unwrap();
std::fs::write("street_lamp.glb", &built.glb).unwrap();
```

## Project

| | |
|---|---|
| `Project::new(sources, host, std, reader)` | from `(file name, text)` pairs; `std: false` leaves out the std parts; `reader` reads `import` lines (`None`: from disk, beside the file) |
| `Project::from_paths(&paths, host)` | every `.parts` file in these files and folders (imports read from disk) |
| `Project::from_text(text, name, host)` | one piece of text |
| `.errors()` | parse errors (each `file:line: message`) |
| `.props()` | a `PropInfo` each: `id`, `title`, `subcategory`, `kind` (`prop` or `building`), `file`, `line`, `imported` |
| `.prop(name)` | the parsed prop |
| `.check()` | the static check (below) |
| `.build(name, &options)` | a `Built` (below); `BuildOptions { glb, steps, snaps, seed }`, where `seed: Some(..)` deals its draws again: another variant, the same every time |
| `.write(name, out_dir, &options)` | build and write `out_dir/<id>.glb` |
| `.write_all(out_dir, only, seed)` | every prop: a `Written` with `built` (id, triangles, seconds), `errors`, `warnings` |
| `.building(name)` | a building as placement data (below) |
| `.uses(name)` | what a prop or part is built from, all the way down: a JSON tree of `name`, `kind`, `file`, `line`, `count`, `at`, `children` |

A project compiles once and keeps what it built: building a second prop reuses the parts, textures and
used props of the first. Make a new `Project` after the files change.
`BuildOptions::default()` writes the `.glb`; `glb: false` is quicker when you only want the geometry.

## check()

`check()` returns a `Report`; `report.to_json()` is what `partscript check --json` prints:

```json
{
 "props": [{"id": "street_lamp", "title": "Street Lamp", "subcategory": "street", "kind": "prop",
            "file": "props/street.parts", "line": 3, "triangles": 204, "warnings": []}],
 "errors": ["props/street.parts:9: unknown material 'woood' ..."],
 "warnings": ["street_lamp: about 2600 triangles, over the 2500 prop budget ..."],
 "materials_checked": true, "styles": [], "dressing": [], "prefix": ""
}
```

Triangle counts are estimates (made without building): close, not exact.

## build()

`build(name, &options)` returns a `Built`:

| field | |
|---|---|
| `asset_id` | the prop's id (with the host's prefix) |
| `glb` | the `.glb` bytes (empty with `glb: false`) |
| `triangles()` | how many, exactly |
| `warnings` | the build's warnings (bounds off their `size`, rooms nobody can reach, over budget...) |
| `parts` | the kitlib `Part`s (one per mesh): their `faces` in prop space |
| `baked` | the parts baked for writing: corners, UVs, vertex colours, per material |
| `steps` | with `steps: true`: per top-level line, the faces it made |
| `origins` | the `use` chain each face came through (`[(file, line), ...]`) |
| `snaps`, `joints`, `links` | its snap points, where chained pieces met, its loose ends |
| `nodes`, `rigged` | the node tree: each part (`name`, `parent`, its pivot `at`, the `baked` part it draws) and each mark (`mark: true`, its `rotation`); `rigged` when there are pivots, parents or marks ([moving parts](../language/moving.md)) |
| `seconds` | how long it took |

`steps: true` also tags every face in the `.glb` (`TEXCOORD_1`) with its line and its use chain, which is
how the preview lights up what a line made. `snaps: true` adds the snap markers as a part named `snaps`.

## Buildings as data

`building(name)` returns JSON (`kitlib::json::Json`; `.dumps(false)` writes it out):

```json
"placements": [{"asset": "floor_concrete", "role": "floor", "position": [1.5, 0.0, -1.5], "yaw": 0.0, "nav": "walkable"}, ...]
"rooms": [{"name": "hall", "rect": [0.0, -9.0, 3.0, 0.0], "storey": 0, "storeys": 3, "theme": ""}, ...]
"openings": [{"kind": "door", "cell": [0, 0], "side": "s", "storey": 0, "position": [1.5, 0.0, 0.0], "yaw": 180.0}, ...]
```

Positions are Y-up (glTF, Godot), round the building's front-left corner. A placement turned other than
about the vertical has `"rotation"` (radians, YXZ) instead of `"yaw"` (degrees). Placements carry `"nav"` for a game's
navigation: `walkable` (floors, walls, corners and stairs: bake navigation from them) or `ignore` (roofs and
parapets); placed and attached pieces carry none. The data also has `title`, `set` (the kit), `grid`
and `storey`.

## Lower level

- `partscript::parse(text, file, &mut program)` parses into a `Program` (props, macros, kits, file
  variables, errors) without a host; `partscript::load_program(sources, std, reader)` does a set of files.
- `partscript::check(&program, &host)` is the static check on a program.
- `partscript::evaluate(text, &env)` works out one expression with the variables in `env`.
- `partscript::REFERENCE` is the text `partscript ref` prints.
- `PartScriptError` is what goes wrong, with `file`, `line` and `message` (it displays as `file:line: message`).
- `partscript::fmt::readable(text)` and `partscript::fmt::terse(text)` are `partscript fmt`.
