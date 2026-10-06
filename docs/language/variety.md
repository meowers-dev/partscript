# Variety

Copies that are all alike look like copies. PartScript draws a little differently for every copy, and
always draws the same: the same file builds the same model every time, so nothing changes until you
change something.

## rand() and pick()

- `rand(lo,hi)` is a number drawn for this copy between lo and hi (`rand(hi)` from 0, `rand()` 0 to 1).
- `pick(a,b,c)` is one of the list, for numbers, materials, words or parameters.
- `odds(a,b,c)` is 0, 1 or 2, as often as the weights say ([below](#odds-one-fate-of-several)).

Each call is drawn on its own, for each copy, for each use of a part: two `rand(0,1)` on one line
draw two numbers.

```parts
prop flower_bed_demo "Flower Bed"
  box at=0,0,on size=2,1,.12 mat=#5a3a22/concrete
  flower at=0,0,.12 h=rand(.25,.45) col=pick(#e84a6a,#f0f0f0,#8a4ac8,#e8c040)/plastic turn=0,0,rand(0,360) \
      scatter 30 over 1.8,.8 apart .12
```

(`flower` is a part in the project's `garden.parts`: every one of the thirty draws its own height, its
colour and which way it faces.)

A part passes its draw on: everything inside it varies with the copy that placed it. That is how
`flower scatter 40 ...` gives forty different flowers from one `def flower`.

## when= and if

`when=EXPR` keeps a line, or a copy, only while EXPR holds. `if EXPR {` ... `}` keeps a whole group.
Expressions compare (`== != < <= > >=`) and combine (`and or not`, `a if c else b`).

```parts
prop fence_demo "Fence, One Missing"
  box at=-1.8+i*.4,0,on size=.08,.04,1 mat=#e8e4d8/paint taper=.4,1 when=i!=4 repeat 10 every 0,0,0
  box at=0,.03,.3 size=4,.03,.06 mat=#e8e4d8/paint repeat 2 every 0,0,.45
  if rand() < .5 {
    box at=-.2,-.4,.04 size=.08,1,.04 mat=#e8e4d8/paint turn=0,0,70
  }
```

## odds(): one fate of several

`odds(a,b,c)` draws 0, 1 or 2 for the copy, as often as its weights say: `odds(70,20,10)` is 0 seven
times in ten, 1 twice and 2 once. The weights are any numbers (they need not add up to 100), so they can
be parameters. Roll the fate once in a part with `set`, and give each way it can turn out its own lines
with `when=`:

```parts
def crate_fate_demo whole=60 open=25 smashed=15
  set fate=odds(whole,open,smashed)
  # 0 whole and 1 open: the crate; an open one has its lid propped up behind it.
  box at=0,0,on size=.5,.4,.4 mat=wood when=fate<=1
  box at=0,.2,.4 size=.5,.03,.4 mat=wood turn=-25,0,0 when=fate==1

  # 2 smashed: its boards in a heap.
  box at=0,0,.03 size=.5,.08,.03 mat=wood turn=0,0,rand(0,180) when=fate==2 scatter 6 within .3

prop crate_odds_demo "Crates, Dealt"
  crate_fate_demo at=-1.6,0,0 repeat 5 every .8,0,0
  crate_fate_demo at=-1.6,1,0 whole=0 open=50 smashed=50 repeat 5 every .8,0,0
```

The front row keeps the part's own odds; the back row never comes up whole. `examples/garden.parts`
deals a whole fence this way: each bay between two posts is whole, gappy, sagging, kicked over or flat
on the ground, then each of its pickets is whole, snapped or gone, and each post stands or leans, all
by odds the fence is given.

## seed=

Every draw comes from the prop's seed. Give the prop `seed=2` (any word or number) and everything
is drawn again: a new take on the same prop. The [variants](basics.md#props) of a prop share their
draws unless each gets a seed.

`Project.build(name, seed=...)` and `partscript build --seed S` deal a prop again without changing its
file: a game's tools can ask for variant 4817 of a broken fence and get the same one every time.

A line's draws follow its own words, not where it stands in the file: add a comment, a blank line or
another line and everything else stays as it was. Change a line and only that line is drawn again.

## fade= and wobble=

- `fade=F` darkens what a line makes toward its base: tone F at the lowest point, full at the top.
  Grass, trunks, walls, anything that should feel grounded.
- `wobble=A` (or `wobble=A,B,C` in x, y, z) moves every corner up to A, differently for each copy.
  Corners that faces share move together, so the shape stays closed: weathered stone, lumpy earth,
  hand-made things, no two bones alike.

```parts
prop rocks_demo "Rocks"
  sphere at=0,0,.15 radius=rand(.12,.3) mat=#8a8678/stone sides=6 rings=4 wobble=.06 fade=.6 scatter 8 within 1.2 apart .5
```

## Text that differs

Label text takes `pick()` and `{expressions}` too, drawn per copy. See
[materials and text](materials.md#text-labels-and-signs).

## Where things are: here

`here.x`, `here.y` and `here.z` are where this copy stands in the prop: where its copy phrase (and
any group or part it is in) puts it, before its own `at=`. With [noise](generators.md#noise) they
vary smoothly over space instead of copy by copy:

```parts
prop city_block_demo "City Block"
  box at=-2,-2,on size=.42,.42,.3+noise(here.x*.4,here.y*.4)*3 mat=#8a8a84/concrete grid 9x9 every .5,.5
```

Inside a part, `here` is where that use of the part stands, so a part can ask where it was put.
