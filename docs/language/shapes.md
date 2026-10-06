# Shapes

Every shape takes `at=` (where it goes; the origin when left off), `mat=` (what it is made of: see
[materials](materials.md)) and its own measurements. Every shape also takes `turn=rx,ry,rz` (degrees
about x, then y, then z), the [copy phrases](copies.md), and the [variety](variety.md) and
[bending](shaping.md) options.

For shapes that have a middle (boxes, cylinders, spheres...), `at=` is that middle, and `on` in a
position sits the shape on that height instead: `at=0,0,on` stands it on the floor, `at=0,0,on(.8)`
on .8. A shape with no `at=` stands at the origin, on the floor.

## box

```text
box at=C size=W,D,H mat=M [taper=TX,TY] [lean=DX,DY] [skip=+z,-y] [from=A to=B] [break=K ...]
```

The workhorse. `taper=` scales the top face (`.6` makes a trapezoid, `0` a ridge or a point), `lean=`
slides the top face over (a sloping front), `skip=` leaves faces off (`+z` the top, `-y` the front...).
`from=`/`to=` runs it as a beam between two points ([placing](placing.md#beams-and-rods-fromto)) and
`break=` knocks it apart ([generators](generators.md#broken-boxes-break)).

```parts
prop shape_box_demo "Boxes"
  box at=-1,0,on size=.6,.6,.6 mat=crate_wood
  box at=0,0,on size=.6,.6,.9 mat=stone taper=.4,.4
  box at=1,0,on size=.6,.5,.8 mat=steel_grey lean=0,-.15
```

## bevel_box

```text
bevel_box at=C size=W,D,H bevel=B mat=M [taper=] [lean=]
```

A box with its edges chamfered by B: anything manufactured, cushions, rounded stones. 44 triangles
against a box's 12, so use it where it shows.

## box_between

```text
box_between from=LO to=HI mat=M
```

A box by two opposite corners, square to the axes.

## cylinder, cone

```text
cylinder at=C radius=R height=H mat=M [sides=10] [top_radius=RT] [axis=x|y|z] [arc=A0,A1] [caps=0] [cap_mat=M]
cone at=C radius=R height=H mat=M [sides=8] [axis=]
```

`axis=` lays it along x or y. `top_radius=` narrows (or widens) the top: buckets, lampshades. `arc=`
draws only part of the round (degrees), `caps=0` leaves the ends open, `cap_mat=` gives the ends
their own material.

```parts
prop shape_cylinder_demo "Cylinders"
  cylinder at=-.8,0,on radius=.3 height=.9 mat=steel_blue sides=10
  cylinder at=0,0,on radius=.25 height=.4 mat=#e8e0d0/plastic top_radius=.32 sides=8 caps=0
  cylinder at=.8,0,0 radius=.3 height=.5 mat=wood axis=x arc=90,270
  cone at=1.6,0,on radius=.2 height=.6 mat=#e25a1a/plastic sides=6
```

## sphere

```text
sphere at=C radius=R mat=M [sides=10] [rings=6]
```

Low sides and rings are the house style: `sides=6 rings=4` is a fine sphere at PSX sizes.

## tube

```text
tube at=C radius=R inner=RI height=H mat=M [sides=12] [axis=]
```

A pipe with a wall: rings, collars, cups, the drum of a well.

## wedge

```text
wedge at=C size=W,D,H mat=M
```

A ramp: full height at the back (+Y), nothing at the front (-Y). Turn it for roofs, steps, chocks.

## lathe

```text
lathe [at=C] mat=M r:z r:z ... [sides=16] [cap=1] [arc=A0,A1]
```

A profile turned about the vertical: each point is a radius and a height. Vases, bottles, posts,
lamp bases, chess pieces. `cap=1` closes the top.

```parts
prop shape_lathe_demo "Vase"
  lathe mat=#3a6a9a/plastic .1:0 .16:.05 .18:.2 .09:.42 .07:.5 .1:.56 sides=10 cap=1
```

## extrude

```text
extrude [at=C] width=W mat=M u:v u:v u:v ... [axis=x|y|z] [taper=T]
```

An outline pushed W through, centred on C. With `axis=x` (the default) the outline is a side view
(points are y:z); `axis=y` a front view (x:z); `axis=z` a plan (x:y). The outline may be any shape,
concave too: a car's side, an armchair, a headstone, a gable. `taper=` narrows W toward the top.

```parts
prop shape_extrude_demo "Gable"
  extrude width=.3 mat=brick -2:0 2:0 2:2 0:3.2 -2:2 axis=y
```

## pipe and sweep

```text
pipe mat=M radius=R P P P ... [sides=6] [closed=1] [smooth=N] [taper=K] [arc=CX,CZ,R,A0,A1[,N]]
sweep mat=M prof=x:y,x:y,... P P P ... [open=1] [closed=1] [smooth=N] [taper=K]
```

A round bar along a line of points (rails, pipes, cables, frames), or any profile along one
(mouldings, gutters, kerbs). `smooth=N` makes a curve through the points, `taper=K` narrows it to K
at the end (branches, horns), `closed=1` joins the last point to the first. `arc=` draws an arc in
the XZ plane instead of a line of points. The points can come from a variable: `set path="0,0,0 1,0,1"`
then `pipe mat=wood radius=.02 path`.

```parts
prop shape_pipe_demo "Pipes"
  pipe mat=steel_dark radius=.03 -1,0,0 -1,0,1 0,0,1.2 1,0,1 1,0,0 smooth=4 sides=6
  pipe mat=#4a3a2a/wood radius=.08 0,.6,0 .2,.7,.6 -.1,.6,1.2 smooth=3 taper=.2 sides=5
  sweep mat=stone prof=-.1:0,.1:0,.06:.08,-.06:.08 -1,-.6,0 1,-.6,0
```

## torus

```text
torus at=C radius=R thick=T mat=M [sides=16] [rings=6] [axis=x|y|z]
```

A ring lying flat: tyres, rims, handles, wreaths, a life ring. `axis=x` or `y` stands it up.

## frame

```text
frame at=C width=W height=H depth=D mat=M [hole=w,h] [hole_at=x,z]
```

A panel with a hole in it, facing the front: picture frames, window and door frames, hatches. The hole
is 70% of the frame unless given.

## arch_wall and vault

```text
arch_wall at=C width=W height=H thick=T mat=M [open=W] [spring=H] [sides=12]
vault at=C width=W depth=D rise=R mat=M [sides=12]
```

`arch_wall` is a wall with a round-arched opening (arcades, gateways, viaduct fronts); C is the bottom
middle of its front face and it is T thick toward +Y. `vault` is a barrel vault seen from below: span W
along x, D deep along y, springing at C's height and rising R.

```parts
prop shape_arch_demo "Gateway"
  arch_wall width=3 height=3.2 thick=.4 mat=stone open=1.4 spring=1.8
  vault at=0,.2,2.5 width=1.4 depth=.4 rise=.7 mat=stone
```

## face

```text
face mat=M P P P ... [double=1]
```

One flat polygon through the points, seen from the side its points run anticlockwise (`double=1`: both
sides).

## panel

```text
panel at=C width=W height=H mat=M [double=1]
```

A flat textured quad facing the front (-Y): posters, screens, glass. Turn it with `turn=0,0,90` to
face right, `turn=-90,0,0` to lie face up. Sit it 2 mm proud of what it is on.

## label and decal

`label` prints text on a quad (number plates, signs, packaging, gravestones) and is covered under
[materials and text](materials.md#text-labels-and-signs). `decal` draws a cell of the host's decal sheets on
a quad (detail a texture carries instead of triangles); the default host has no decal sheets.

## terrain

Ground from an expression; see [generators](generators.md#terrain).
