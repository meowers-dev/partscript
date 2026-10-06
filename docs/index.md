# PartScript

PartScript is a small language for building low-poly, PSX-style 3D models: props, rooms, ruins,
whole buildings and the hills they stand on. A model is a few words a line, a few dozen lines a
scene, and it compiles to `.glb` (glTF 2.0) by one small Rust program, with no Blender or other 3D package.

```parts
prop street_lamp_demo "Street Lamp"
  lathe mat=steel_dark .19:0 .19:.07 .15:.11 .11:.42 .085:.5 .06:.58 sides=10 cap=1
  cylinder at=0,0,on(.58) radius=.05 height=2.5 mat=steel_dark sides=8
  lamp = cylinder at=0,0,on(3.13) radius=.12 height=.34 mat=lamp_warm sides=6 top_radius=.16
  box at=.14,0,3.3 size=.025,.025,.36 mat=steel_dark ring 6 step 60
  cone on=lamp radius=.22 height=.16 mat=steel_dark sides=6
```

That is a whole prop. Each line is a shape (a lathe, two cylinders, six bars round the lamp, a cap),
where it goes, how big it is and what it is made of. The model it builds is 200 triangles with
small, crunchy, dithered textures, the way games looked in 1997.

## What it can do

- **Shapes and parts.** Boxes, bevelled boxes, cylinders, cones, spheres, lathes, extrusions,
  pipes, sweeps, rings, frames, arches and vaults, flat panels and printed labels. Any group of
  lines can become a part with parameters (`def`), used anywhere, as often as you like.
- **Variety.** `rand()`, `pick()` and `when=` make every copy different: a bookshelf where no two books
  match, a graveyard where every stone has its own name and dates. `odds()` deals each copy one of
  several fates as often as you say: a fence whose every bay is whole, sagging or flat in the grass.
- **Placing by name.** Name a shape and place others against it: `on=desk`, `desk.top`, `row` and
  `stack` for things side by side and piled up, `from=`/`to=` for beams and cables.
- **Places.** Ground from noise (`terrain`), walls with chunks knocked out (`break=`), things that
  fall onto whatever is under them (`drop=`), moss and ivy that grow over named shapes (`scatter on`).
- **Buildings.** Kits of wall, floor and stair pieces, and buildings laid out room by room on a grid,
  each room furnished by a part of your own.

## Where to go next

- New here: [Install and first prop](start/install.md), then [the tour](start/tour.md).
- Looking something up: [every statement and option](reference/statements.md).
- Want to see what it can do: the [gallery](guides/gallery.md), or open the
  [playground](../play.html) and change something.
- Building it into a game or a tool: the [Rust API](engine/api.md) and [embedding](engine/hosts.md).

## Conventions in these docs

Every example with a `prop` in it builds as written (the test suite checks), and has a **try it**
link that opens it in the playground, where you can change it and watch the model change. Examples
may use the parts defined in the project's example files, as the playground does.
