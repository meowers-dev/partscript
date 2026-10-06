# Python API

Everything the command line does is a few calls on `partscript.Project`.

```python
from partscript import Project

project = Project.from_paths(["props/"])        # every .parts file under props/, plus the std parts
report = project.check()                         # static check: no building
for error in report["errors"]:
    print(error)
built = project.build("street_lamp")             # one prop: parts, steps, triangles, .glb bytes
open("street_lamp.glb", "wb").write(built.glb)
```

## Project

| | |
|---|---|
| `Project(sources, host=None, std=True)` | from a list of `(file name, text)` pairs; `std=False` leaves out the std parts |
| `Project.from_paths(paths, host=None)` | every `.parts` file in these files and folders |
| `Project.from_text(text, name="<text>", host=None)` | one piece of text |
| `.errors` | parse errors (each `file:line: message`) |
| `.props()` | `[{"id", "title", "subcategory", "kind", "file", "line"}]`, kind `prop` or `building` |
| `.prop(name)` | the parsed prop |
| `.check()` | the static check (below) |
| `.build(name, glb=True, steps=False, snaps=False, seed=None)` | a `Built` (below); `seed=` deals its draws again: another variant, the same every time |
| `.write(name, out_dir)` | build and write `out_dir/<id>.glb` |
| `.write_all(out_dir, only=None)` | every prop: `{"built": [{"id", "triangles", "seconds"}], "errors", "warnings"}` |
| `.building(name)` | a building as placement data (below) |
| `.uses(name)` | what a prop or part is built from, all the way down: a tree of `{"name", "kind", "file", "line", "count", "at", "children"}` |

A project compiles once and keeps what it built: building a second prop reuses the parts, textures and
used props of the first. Make a new `Project` after the files change.

## check()

```python
{
    "props": [{"id": "street_lamp", "title": "Street Lamp", "subcategory": "street", "kind": "prop",
               "file": "props/street.parts", "line": 3, "triangles": 204, "warnings": []}],
    "errors": ["props/street.parts:9: unknown material 'woood' ..."],
    "warnings": ["street_lamp: about 2600 triangles, over the 2500 prop budget ..."],
    "materials_checked": True, "styles": [], "dressing": [], "prefix": "",
}
```

Triangle counts are estimates (made without building): close, not exact.

## build()

`build(name)` returns a `Built`:

| field | |
|---|---|
| `asset_id` | the prop's id (with the host's prefix) |
| `glb` | the `.glb` bytes (empty with `glb=False`, which is quicker when you only want the geometry) |
| `triangles` | how many, exactly |
| `warnings` | the build's warnings (bounds off their `size`, rooms nobody can reach, over budget...) |
| `parts` | the kitlib `Part`s (one per mesh): their `faces` in prop space |
| `baked` | the parts baked for writing: corners, UVs, vertex colours, per material |
| `steps` | with `steps=True`: per top-level line, the faces it made |
| `origins` | the `use` chain each face came through (`((file, line), ...)`) |
| `snaps`, `joints`, `links` | its snap points, where chained pieces met, its loose ends |
| `seconds` | how long it took |

`steps=True` also tags every face in the `.glb` (`TEXCOORD_1`) with its line and its use chain, which is
how the preview lights up what a line made. `snaps=True` adds the snap markers as a part named `snaps`.

## Buildings as data

```python
data = project.building("apartment_closed")
data["placements"][0]   # {"asset": "floor_concrete", "role": "floor", "position": [1.5, 0.0, -1.5], "yaw": 0.0, "nav": "walkable"}
data["rooms"][0]        # {"name": "hall", "rect": [0.0, -9.0, 3.0, 0.0], "storey": 0, "storeys": 3, "theme": ""}
data["openings"][0]     # {"kind": "door", "cell": [0, 0], "side": "s", "storey": 0, "position": [1.5, 0.0, 0.0], "yaw": 180.0}
```

Positions are Y-up (glTF, Godot), round the building's front-left corner. A placement turned other than
about the vertical has `"rotation"` (radians, YXZ) instead of `"yaw"` (degrees). Placements carry `"nav"` for a game's
navigation: `walkable` (floors, walls, corners and stairs: bake navigation from them) or `ignore` (roofs and
parapets); placed and attached pieces carry none. `data` also has `title`, `set` (the kit), `grid`
and `storey`.

## Lower level

- `partscript.parse(text, file)` gives a `Program` (props, macros, kits, file variables, errors) without a
  host.
- `partscript.check(program, host)` is the static check on a program.
- `partscript.evaluate(text, env)` works out one expression with the variables in `env`.
- `partscript.REFERENCE` is the text `partscript ref` prints.
- `PartScriptError` is what goes wrong, with `.file`, `.line` and `.message`.
