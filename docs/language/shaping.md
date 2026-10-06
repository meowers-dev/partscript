# Bending and curving

These options reshape what a line makes, after it is made. On a `use` or a `group` they reshape the
whole part.

## twist=, shrink=, bend=

They work by height, from the lowest point of what the line made to the highest:

- `twist=DEG` turns it about the vertical, nothing at the bottom and DEG at the top.
- `shrink=K` narrows it toward the top: K times as wide there (`.5` half, `1.5` wider).
- `bend=DEG` curves it over toward +Y, DEG at the top; `bend=DEG,HEADING` curves it toward HEADING
  degrees round from +Y.

Use enough pieces for a smooth bend: a box is one piece, so bend a stack of them, or a cylinder with
a few rings.

```parts
prop twist_demo "Twisted, Shrunk, Bent"
  group at=-1,0,0 twist=90 {
    box at=0,0,on(i*.2) size=.4,.4,.2 mat=stone repeat 10 every 0,0,0
  }
  group shrink=.3 {
    box at=0,0,on(i*.2) size=.4,.4,.2 mat=stone repeat 10 every 0,0,0
  }
  group at=1,0,0 bend=70 {
    box at=0,0,on(i*.2) size=.2,.2,.2 mat=#3a6a2a/plastic repeat 10 every 0,0,0
  }
```

The gramophone in `showpieces.parts` bends a flaring cylinder into a horn; the twisted column twists
a stack of slices a quarter turn.

## smooth=

On `pipe`, `sweep` and `along=`, `smooth=N` draws a curve through the points instead of straight
lines between them, N pieces between each pair. A few points make a cable, a vine, a rail that bends.

```parts
prop smooth_demo "Hose"
  pipe mat=#2a5a2a/rubber radius=.03 0,0,.1 .6,.3,.1 .9,-.2,.5 .4,-.4,.9 .1,0,1.1 smooth=6 sides=6
```

## taper=

On `pipe` and `sweep`, `taper=K` narrows the line toward its end: K times its size there. Branches,
horns, tusks, tentacles, roots.

```parts
prop tentacle_demo "Tentacle"
  pipe mat=#8a4a6a/plastic radius=.12 0,0,0 .1,.1,.6 -.15,.2,1.1 0,.5,1.5 .3,.6,1.6 smooth=5 taper=.1 sides=7
```

(On `box` and `bevel_box`, `taper=TX,TY` is something else: it scales the top face.)

## Rounder shapes from fewer triangles

- `sides=` on round shapes: 6 to 12 is the house style; 3 to 5 for twigs and wires.
- `smooth=1` on a [part](reuse.md#more-than-one-mesh-part) shades it smooth instead of faceted.
- `wobble=` (see [variety](variety.md#fade-and-wobble)) takes the machine-made look off anything.
