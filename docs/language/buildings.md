# Buildings

A building is laid out room by room on a grid, from a kit of pieces: wall pieces of different kinds,
floors, roofs, parapets, partitions, corners and stairs. Rooms say where floors and walls go; `open`
swaps a wall for a door, a window or a shop front; `stair` climbs a storey; and a room's `fill=` names a
part that furnishes it, given the room's size and where its doors are.

## A kit

```text
kit NAME [grid=4] [storey=4] [wall=.3] [parapet_lift=.3] [stair_cells=2] [stair_exit=ahead|back] [walls=SET]
  wall solid=PIECE window=PIECE door=PIECE ...
  piece floor=PIECE roof=PIECE parapet=PIECE partition=PIECE corner=PIECE stair=PIECE
  snap PIECE NAME C DIR [up=] [kind=]
```

Pieces are props (or a host's assets). A kit names them by role:

- **wall kinds**: `solid` is needed; any other word is a kind of wall (`window`, `door`, `shopfront`,
  `inner_door`, `sash`...). A wall piece stands on the floor line, centred, along x, its inside facing
  the front (-Y), one grid cell long.
- **floor** and **roof**: origin on the top surface, centred on the cell. **parapet**: along x, on a roof's
  edge. **partition**: a wall between two rooms. **corner**: the post at a building's outside corners.
- **stair**: origin at the lower nosing, rising toward +Y, a storey high and `stair_cells` long (or one
  cell turning back on itself, a dog-leg, with `stair_exit=back`).

`grid=` and `storey=` are the cell size and storey height in metres, `wall=` the wall thickness.
`walls=SET` starts from one of the host's own wall sets (see [embedding](../engine/hosts.md)).

The `flat` kit in `buildings.parts` is a complete one: brick walls with windows and doors, board
and tile floors, a flat roof with parapets, and a dog-leg stair with rails that join up.

## A building

```text
building NAME "Title" kit=KIT [cutaway=open]
  room X,Y W,D [storeys=1] [storey=0] [walls=KIND] [floor=PIECE|none] [roof=none] [theme=STYLE] [as=NAME] [fill=DEF ...]
  walls KIND [storey=N,M|all] [side=s,n,w,e]
  open X,Y SIDE KIND [storey=0]
  stair X,Y [DIR] [storey=0]
  roof [PIECE|none] [parapet=PIECE|none]
  attach PIECE to=TARGET [at=SNAP] [via=SNAP] [spin=DEG] [slide=A,B] [as=NAME]
  place PIECE C [turn=] [as=NAME]
```

Cells count from the front-left corner, x to the right and y to the back; storeys from 0. A side is
`s` (front), `n` (back), `w` or `e`.

- **room** covers W x D cells from X,Y for `storeys=` storeys from `storey=`: floors on each, walls
  round it, a roof and parapets on top. Rooms that touch share a partition; a room on a room is a
  higher storey. `as=` names it, `floor=` swaps its floor piece.
- **walls** sets the outside wall kind for some storeys and sides (`walls window storey=1,2`).
- **open** sets one wall: `open 1,0 s door`. X (or Y) may be a range: `open 0..2,0 s window`.
- **stair** puts the kit's stair in cell X,Y rising toward DIR (`n` unless given), and opens the floor
  above it.
- **roof** swaps the roof or parapet piece, or leaves them off.
- **attach** snaps a piece onto another by their [snaps](paths.md#snap): a sign on a wall's outside, a
  chimney on a roof. TARGET is an `as=` name, a wall `X,Y,SIDE[,STOREY]`, a floor `X,Y,floor[,STOREY]`
  or a roof `X,Y,roof`. `slide=A,B` moves it along the face, A to the right and B up.
- **place** puts a piece at a position in metres.
- `cutaway=open` leaves off the front walls and the roof: a dolls' house view.

## Furnishing rooms: fill=

`fill=DEF` runs a part in every storey of the room, in the room's own space: x from 0 to `w` across
it, y from 0 to `d` back, with these variables:

| | |
|---|---|
| `w`, `d`, `h` | the room's inside width, depth and height |
| `level` | which storey this is |
| `door_s`, `door_n`, `door_w`, `door_e` | where a door is on that wall, in metres from the room's front-left inside corner (along x for `s` and `n`, along y for `w` and `e`), or -1 for none |

Any other options on the `room` line are passed to the part as parameters. Because it is a part, it
draws its own variety: each room, and each storey of it, comes out different.

```parts
def parlour_demo
  rug at=w/2,d/2,0 rw=w-1.4 rd=d-1.6
  sofa at=w/2,d-.5,0 turn=0,0,180
  armchair at=.6,d/2,0 turn=0,0,-90 when=door_w<0
  floor_lamp at=w-.4,d-.4,0
  ceiling_lamp at=w/2,d/2,h-.2

building lodge_demo "Lodge" kit=flat cutaway=open
  room 0,0 2,1 fill=parlour_demo
  room 2,0 1,1 floor=floor_tiles fill=cottage_kitchen
  open 0,0 s door
  open 1,0 e inner_door
```

(`rug`, `sofa`, `armchair` and the other furniture, and the `flat` kit, are in `buildings.parts`.)

## What the build checks

Building a building also checks it makes sense, and warns where it does not:

- a room no door or stair reaches from outside;
- a stair with no floor at its foot, or none where it arrives;
- a room's furnishing that reaches outside the room (through a wall, into the floor above);
- furniture standing in a doorway (the metre just inside every door is kept clear; rugs may lie there).

## Buildings as data

A building builds into one model, like a prop. It is also data a game can place piece by piece:
`Project::building(name)` gives every piece's asset and position (Y-up), the rooms and the openings. See
the [Rust API](../engine/api.md#buildings-as-data).
