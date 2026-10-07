# Install and first prop

PartScript is one program, `partscript`, written in Rust with no other dependencies. Install it with
Cargo (from [rustup](https://rustup.rs)):

```sh
cargo install --locked --git https://github.com/meowers-dev/partscript partscript-cli
partscript ref | less               # the whole language on one page
```

Or, working from a clone of the repository:

```sh
git clone https://github.com/meowers-dev/partscript
cd partscript
cargo run --release --locked -p partscript-cli -- ref | less
```

You do not need to install anything to try it: the [playground](../../play.html) runs PartScript in
your browser.

## A first prop

Make a folder called `props` and a file in it called `crates.parts`:

```parts
prop crate_stack "Crate Stack"
  box size=.6,.45,.4 mat=crate_wood
  box at=0,0,on(.4) size=.6,.45,.4 mat=crate_wood turn=0,0,8
  bottle = cylinder at=.12,.05,on(.8) radius=.04 height=.28 mat=glass_opaque sides=6
```

Three lines, three shapes:

- A box `.6` metres wide (x), `.45` deep (y) and `.4` tall (z), in the library's `crate_wood`. A shape
  with no `at=` stands at the origin, on the floor.
- A second box sitting on top of the first: `on(.4)` means "with its bottom at .4", and it is
  turned 8 degrees about the vertical.
- A bottle on top of that, named `bottle` so later lines could place things against it.

Check it, then build it:

```sh
partscript check props/          # parses everything, counts triangles; no building, instant
partscript build props/ -o out/  # writes out/crate_stack.glb
```

`out/crate_stack.glb` opens in Blender, Godot, three.js, any glTF viewer. It is 44 triangles with
two small textures.

## Units and directions

- Lengths are **metres**, angles are **degrees**.
- **X** is right, **Y** is back, **Z** is up (Blender's axes). A prop's front faces **-Y**.
- A prop stands on the floor at z = 0, its origin at the middle of its footprint.
- In the `.glb`, positions are converted to glTF's Y-up axes (see [what the .glb holds](../engine/output.md)).

## Next

The [tour](tour.md) builds one prop ten ways, a new idea each time. [Tools](tools.md) covers the
command line, the formatter, the preview and the playground.
