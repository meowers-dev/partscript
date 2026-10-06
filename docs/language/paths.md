# Paths, snaps and links

Some things are made of pieces that join end to end (paths, rails, pipes, fences) or have to meet
other things at known points (a sign on a wall, a lamp on a post). Snaps say where a piece joins;
`chain` and `along=` lay pieces out; links tie up loose ends.

## snap

```text
snap NAME C DIR [up=DIR] [kind=WORD]
```

A connection point on a prop: where it is, which way it faces out of the piece, and what kind of thing
joins there. DIR is `+x -x +y -y +z -z` or `front back left right up down`. Two snaps join when their kinds
match (or either is `any`, the default). Every piece also has snaps from its bounds: `base`, `top`,
`front`, `back`, `left`, `right`.

```text
snap start 0,-1,0 -y kind=path
snap end 0,1,0 +y kind=path
```

## chain

```text
chain A B*3 C [at=] [turn=]
```

Lays pieces end to end, each one's `start` snap on the last one's `end` snap (without them, its front
on the last one's back). It starts heading +Y. A bend piece turns the rest of the path; a steps piece
lifts it.

```parts
prop walk_demo "Garden Walk"
  chain path_straight path_bend path_straight path_steps path_straight*2 path_bend_left path_straight
```

(The path pieces are in the project's `paths.parts`.)

## Along a line of points

```text
LINE ... along="P P P ..." every=D [fit=1] [joints=1] [corners=1] [closed=1] [smooth=N]
```

Copies down a line of points (`x,y` or `x,y,z`), each turned so its +X follows the line:

- `every=D`: one each D metres, centred in its slot;
- `fit=1`: a whole number on each straight, stretched to fill it exactly (a fence panel always meets
  a post);
- `joints=1`: one where those pieces meet, instead (the posts);
- `corners=1`: one at each point, turned halfway between the straights that meet there;
- `closed=1`: the last point joins the first; `smooth=N`: along a curve through the points.

The line can be a variable, so one line of points can carry the panels, the posts and the hedge:

```parts
prop yard_demo "Fenced Yard"
  set yard="0,0 6,0 6,4 0,4"
  box at=3,2,-.02 size=6.4,4.4,.04 mat=#4a6a2a/plaster
  fence_panel along=yard every=2 fit=1 closed=1 when=i!=1
  fence_post along=yard every=2 fit=1 closed=1 joints=1
```

## link and join

```text
link KIND at=C toward=DIR
join KIND [reach=.6] [radius=.025] [sides=6] [mat=steel_dark] [bulge=] [with=DEF]
```

A piece can leave loose ends: `link` marks one, where it is and which way it leaves the piece. A
`join` line bridges the loose ends of its kind that nearly meet: within `reach=`, facing each other,
nearest first, each end once. Ends that already touch count as joined. The bridge is a pipe that leaves
one end and curves into the other, so two ends side by side loop round, like a stair rail's return at a
half landing. `with=DEF` draws each bridge with a part of your own instead, laid along +X for `length`
metres. Loose ends come up through `use`, `chain` and buildings, so a stair's rails join the next
storey's.

```parts
def rail_run_demo run=2
  pipe mat=steel_dark radius=.025 0,0,.9 run,0,.9 sides=6
  box at=.1+i*(run-.2)/2,0,on size=.04,.04,.9 mat=steel_dark repeat 3 every 0,0,0
  link rail at=0,0,.9 toward=-x
  link rail at=run,0,.9 toward=+x

prop railing_demo "Railing, Auto Joined"
  rail_run_demo at=0,0,0 run=1.6
  rail_run_demo at=1.9,0,0 run=1.3
  rail_run_demo at=3.4,.2,0 run=1.6 turn=0,0,90
  join rail reach=.6
```

The three runs come out as one rail, round the corner. With `with=`:

```parts
def swag_demo
  pipe mat=steel_grey radius=.014 0,0,0 length*.5,0,-length*.12 length,0,0 smooth=4 sides=4

prop chain_posts_demo "Posts and Chain"
  bollard at=0,0,0 along="0,0 1.6,.3 3.4,0 5,.8" corners=1
  join chain reach=2.2 with=swag_demo
```

(`bollard` is from `paths.parts`: a post with a `link chain` on each side.)

## Seeing snaps

Built with snaps (`project.build(name, snaps=True)`, which the preview does), a model carries small
markers for its snaps, the joints of its chains and its loose ends, each kind its own colour, as a part
called `snaps`. The preview's *snap points* toggle shows and hides them.
