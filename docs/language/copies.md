# Copies

Any shape, `use` or group line can make copies of itself with a phrase at its end. The copies move
or turn the whole line: its `at=` is where the first one goes.

| phrase | copies |
|---|---|
| `repeat N every X,Y,Z` | N in a line, each one X,Y,Z on from the last |
| `grid A x B every X,Y` | A along x by B along y (`grid AxBxC every X,Y,Z` adds layers) |
| `ring N` | N turned evenly round the vertical (`ring N step D`: D degrees apart) |
| `scatter N over W,D` | N spread over a W x D area round the origin (`apart G`: kept G apart where they can be) |
| `scatter N within R` | N spread over a disc of radius R |
| `scatter N on NAME` | N spread over the faces of a named shape ([generators](generators.md#growing-over-shapes-scatter-on)) |
| `mirror x` | the line and its mirror image across x = 0 (`mirror xy`: four) |
| `along="P P P" every=D` | copies down a line of points ([paths](paths.md#along-a-line-of-points)) |

```parts
prop copies_demo "Copies"
  box at=-3,0,on size=.2 mat=crate_wood repeat 4 every 0,0,.25
  box at=-1.6,-.4,on size=.15 mat=steel_blue grid 3x3 every .3,.3
  box at=.4,0,on size=.08,.3,.4 mat=wood ring 8
  sphere at=2.4,0,.08 radius=.08 mat=#e8c040/plastic sides=6 rings=3 scatter 20 within .6 apart .1
```

Counts can be expressions: `repeat n`, `repeat (floor(len/.6))`. A line may make at most 400 copies.

## Which copy is this?

On a line that makes copies, `i` is the copy's number, from 0. Use it in anything: positions, sizes,
`when=`. Name it instead with `as`, and a grid gives its column and row:

```parts
prop stairs_demo "Stairs"
  box at=0,i*.28,on size=1,.28,.18+i*.18 mat=concrete repeat 8 every 0,0,0
  box at=-.8+col*.4,-1,on size=.3,.3,.1+row*.3 mat=stone grid 5x3 every 0,-.5 as col,row
```

`repeat N every 0,0,0` makes N copies in the same place, for lines that place each copy themselves
with `i`.

## Rings turn about the origin

`ring` turns copies about the vertical through the **origin of the line's space**, not about the
shape's own middle. Put the shape where the first one goes, at its radius:

```parts
prop clock_demo "Clock Face"
  cylinder at=0,0,1 radius=.3 height=.03 mat=#f0ece0/plastic axis=y sides=16
  group at=0,-.02,1 turn=90,0,0 {
    box at=.25,0,0 size=.05,.012,.012 mat=steel_dark ring 12
  }
```

Here the group stands the ring up (turning it 90 degrees about x), so the ticks go round the face.

## Copies of copies

Phrases apply in order, each to all the copies so far: `repeat 3 every 1,0,0 mirror x` is three in a
row, then their mirror images.

## Nudges and sags

- `jitter=P[,D]` moves each copy up to P metres and turns it up to D degrees, at random: piles,
  mess, a row of things set down by hand.
- `hang=S` makes a `repeat` row sag S at its middle, like things strung on a wire: bunting, bulbs.

```parts
prop bunting_demo "Bunting"
  box at=-2,0,on size=.06,.06,2.4 mat=wood repeat 2 every 4,0,0
  cone at=-1.8,0,2.28 radius=.12 height=.24 mat=#c83a3a/fabric|#e8c040/fabric|#3a6ac8/fabric sides=3 turn=180,0,0 \
      repeat 19 every .2,0,0 hang=.35
```
