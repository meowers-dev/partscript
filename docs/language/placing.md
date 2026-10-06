# Placing by name

Working out where things go by adding up coordinates is slow to write and hard to read later.
Name a line and the lines after it can place things against what it made.

## Naming a line

```text
NAME = any shape, use, group, row or stack line
```

The name stands for the box that the line's shapes fill (all its copies together). Read numbers off it:

| | numbers |
|---|---|
| `desk.left` `desk.right` | its smallest and largest x |
| `desk.front` `desk.back` | its smallest and largest y (the front is -Y) |
| `desk.bottom` `desk.top` | its lowest and highest z |
| `desk.x` `desk.y` `desk.z` | its middle |
| `desk.w` `desk.d` `desk.h` | its width, depth and height |

and points on it, where a whole position goes:

| | point |
|---|---|
| `desk` | its middle |
| `desk.top` | the middle of its top (likewise `.bottom`, `.left`, `.right`, `.front`, `.back`) |
| `desk.top_left_front` | a corner; any sides joined by `_` (`desk.top_front` is the middle of its top front edge) |

```parts
prop name_numbers_demo "Shelf and Clock"
  wall = box at=0,0,on size=1.6,.06,1.9 mat=#d8d0c0/plaster
  shelf = box at=0,wall.front-.125,on(1.4) size=1.2,.25,.03 mat=wood
  box at=shelf.left+.05,shelf.y,shelf.bottom-.1 size=.03,.2,.2 mat=steel_dark taper=1,.2 mirror x
  clock = cylinder at=shelf.x,shelf.y,on(shelf) radius=.12 height=.06 mat=brass axis=y sides=12
  cylinder at=clock.x,clock.front-.002,clock.z radius=.1 height=.004 mat=#f0ece0/paint axis=y sides=12
```

Names hold for the rest of their block and anything inside it, and a part sees the names of the line
that uses it. Each line reads a name in its own space: inside a turned group, `desk.top` is still the
top of the desk.

## on=

`on=NAME` stands a thing on top of a named shape. Its `at=` is then measured from the middle of that
top and needs only x,y (a shape sits on the surface; give z to lift it):

```parts
prop on_demo "Things on a Table"
  table = box at=0,0,on(.72) size=1.2,.7,.04 mat=wood
  box at=table.left+.04,table.front+.04,on size=.05,.05,.72 mat=wood mirror xy
  mug on=table at=.3,-.1
  box on=table at=-.3,.1 size=.3,.21,.03 mat=#c83a3a/paint turn=0,0,12
  bottle = cylinder on=table at=0,.15 radius=.04 height=.28 mat=glass_opaque sides=6
  cylinder on=bottle radius=.015 height=.06 mat=#c8a040/plastic sides=6
```

`on(NAME)` does the same in one part of a position: `at=0,.2,on(table)` sits it on the table's top.

### Against a side: on=NAME.side

`on=drawers.front` stands a thing against the front of `drawers` instead of on its top, touching it from
outside; likewise `.back`, `.left`, `.right` and `.bottom` (hanging under it). Its `at=` is two numbers along
that side from its middle: across, then up (for `.bottom`, x then y).

```parts
prop cabinet_demo "Cabinet"
  body = box size=.6,.45,.9 mat=#4a6a7a/paint
  stack on=body.front pack=centre gap=.03 {
    box size=.52,.012,.26 mat=#5a7a8a/paint repeat 3
  }
  box on=body.right at=0,.2 size=.02,.1,.1 mat=brass
  box on=body.bottom size=.5,.35,.06 mat=#2a2a2a/plastic
```

## row and stack

```text
row x|y|z|-x|-y|-z [gap=G | over=L] [pack=start|centre|end] [align=back,bottom...] [at=] [on=] [turn=] {
  lines
}
stack [gap=G] ... {  lines  }
```

A row takes every copy of every line inside it and sets them end to end along its axis, by their
actual size, so things of drawn sizes still pack tight. `gap=` spaces them; `over=L` spreads them to fill
L exactly. The row is centred on its `at=` (`pack=start` puts its first at `at=`, `pack=end` its last).
`align=` lines their sides up across the row: `align=back` puts all their backs on one line.
`repeat N` needs no `every` inside a row.

`stack` is a row going up, from the floor: each one on the last.

```parts
prop shelf_demo "Bookshelf"
  set oak=#6a4a2a/wood
  box at=-.62,0,on size=.04,.3,1.6 mat=oak mirror x
  low = box at=0,0,on(.3) size=1.2,.3,.03 mat=oak
  high = box at=0,0,on(1) size=1.2,.3,.03 mat=oak
  row x on=low align=back {
    box size=rand(.025,.06),rand(.18,.22),rand(.22,.3) mat=pick(#8a2a2a,#2a4a6a,#3a6a3a,#c8a040,#e8e0d0)/paint repeat 16
  }
  row x on=high over=1.05 align=back {
    jar repeat 6
  }
  stack at=0,0,on {
    box size=1,.28,.12 mat=#5a4a3a/fabric turn=0,0,rand(-3,3) repeat 2
  }
```

(`jar` is from the project's `layout.parts`.) Name the row itself to place things after it:
`books = row x ...`, then `box at=books.right+.03,...`.

## Beams and rods: from=/to=

`box`, `bevel_box`, `cylinder`, `cone` and `use` can run from one point to another instead of standing
at one. Points are `x,y,z` or a named shape's point.

- `box from=A to=B size=S` is a beam; `size=` is its cross-section (one number, or width,height), and its
  top stays up.
- `cylinder from=A to=B radius=R` is a rod; a `cone` points at B.
- `use NAME from=A to=B` lays the part with its +X from A toward B, and gives a `def` the
  variable `length`, how far it is.

```parts
def rope_demo
  pipe mat=#8a7a5a/fabric radius=.015 0,0,0 length*.5,0,-length*.12 length,0,0 smooth=4 sides=4

prop rope_posts_demo "Rope Between Posts"
  post = box at=-1.5,0,on size=.15,.15,1.4 mat=wood mirror x
  box from=-2,0,0 to=-1.575,0,.9 size=.08 mat=wood mirror x
  cylinder from=-1.45,-.5,0 to=-1.5,0,1.2 radius=.03 mat=steel_dark sides=5 mirror x
  rope_demo from=-1.5,0,1.3 to=1.5,0,1.3
```
