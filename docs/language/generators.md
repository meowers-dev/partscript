# Terrain, ruins, drop and grow

These make places rather than props: ground, broken walls, things scattered over it all, moss and ivy
growing on it.

## terrain

```text
terrain [at=C] size=W,D mat=M height=EXPR [cells=16[,N]] [steep=M slope=35] [skirt=0]
```

Ground W x D whose height at each corner of a grid is EXPR, where `x` and `y` are that corner's place
measured from the middle. It is drawn in flat triangles, the low-poly look, and faces steeper than
`slope=` degrees take the `steep=` material (rock on the slopes, grass on the flat). Its sides go down
to C's height, so it reads as a block of ground (`skirt=0` leaves them off). `cells=` is how many squares
a side: 96 at most.

```parts
prop hills_demo "Hills" hero=1
  terrain size=12,12 cells=30 mat=#4e6a2c/fabric steep=#7a7266/stone slope=30 fade=.75 height=rough(x*.2,y*.2)*3
```

A variable can hold part of the expression, worked out at each corner:

```parts
prop crater_demo "Crater" hero=1
  set r="hypot(x,y)"
  terrain size=10,10 cells=28 mat=#6a5a42/concrete steep=#4a4036/stone height=1.2-1.2/(1+(r-2.5)*(r-2.5))+noise(x,y)*.3
```

## Noise

`noise(x,y,z)` is smooth noise, between 0 and 1 and about .5 on average: it wanders, without jumps.
`rough(x,y,z)` lays finer and finer noise on top, lumpy at every size, for ground and stone. Both take
one to three numbers, the same for the whole prop (`seed=` deals them again). Multiply the point to
change the size of the bumps: `noise(x*.2,y*.2)` has bumps about 5 m across.

Noise is for anything that should vary smoothly over space: heights, sizes and `when=` with
[here](variety.md#where-things-are-here).

## Dropping things: drop=

`drop=1` lets each copy fall onto whatever is under it and rest on the highest point it touches. A copy
that starts inside something (set in a hill) comes up out of it instead. Later copies land on earlier
ones, so a line of drops makes a pile; nothing under it at all lands on the floor.

- `drop=lean` also tilts it with the ground it lands on.
- `sink=D` settles it D into the ground (`sink=-1` floats it a metre up).

```parts
prop rubble_heap_demo "Rubble Heap"
  terrain size=4,4 cells=10 mat=#6a6458/concrete height=noise(x,y)*.6
  box at=0,0,4 size=rand(.2,.4),rand(.15,.3),rand(.1,.2) mat=pick(#8a8478,#9a6a58,#6a6458)/stone \
      turn=rand(0,360),rand(-30,30),rand(0,360) wobble=.03 drop=lean scatter 40 within 1.2
```

## Broken boxes: break=

```text
box ... break=K [chunk=S] [core=M] [rubble=R]
```

Knocks K (0 to 1) of a box out, in chunks about S across, most from the top and the ends, in noisy
bites. Chunks left hanging by nothing fall too. Corners near the damage are nudged, so it crumbles
rather than steps. `core=` is the material where it broke (brick behind plaster, darker stone).
`rubble=R` lays R of the fallen chunks round its foot as rubble, dropped onto whatever is there.

```parts
prop wall_damage_demo "Broken Wall"
  box at=0,0,-.03 size=6,3,.06 mat=#4e6a2c/fabric
  box size=4,.4,2.8 mat=#b4ac9a/stone core=#6e675c/stone break=.35 chunk=.3 rubble=.5
```

Every copy breaks its own way, and a different `seed=` breaks it again.

## Growing over shapes: scatter on

```text
LINE ... scatter N on NAME [facing up|side|down|any] [apart G]
```

Spreads N copies over the faces of a named shape, each standing out of its face: its +Z along the
face's outward direction. `facing up` keeps to faces that look up (moss, snow, dust, birds), `side`
to walls (ivy, posters, rivets), `down` to undersides (stalactites, cobwebs).

```parts
prop mossy_wall_demo "Mossy Wall"
  wall = box size=3,.4,2 mat=#b4ac9a/stone core=#6e675c/stone break=.25 chunk=.3
  box size=rand(.07,.15),rand(.07,.15),.03 mat=pick(#3a6a2a,#4a7a32,#2f5f26)/fabric turn=0,0,rand(0,360) \
      scatter 60 on wall facing up apart .08
  box size=.12,.15,.02 mat=pick(#3a6a2a,#4a7a32)/fabric turn=0,0,rand(0,360) scatter 50 on wall facing side
```

## Putting it together

The ruined chapel in `ruins.parts` is these and little else: a hill of `terrain`, walls with `break=`,
moss and ivy with `scatter on`, graves dropped with `drop=lean sink=.12` and `when=` keeping them off
the chapel floor, a path of flags dropped along a line, and a dead tree that grows itself from a part
that uses itself. See the [gallery](../guides/gallery.md).
