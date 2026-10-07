# Every statement and option

The whole language in tables. Shorthand forms are in brackets. `C` is a position (`x,y,z`, or a
named shape's point), `S` a size, `M` a material, `P` a point; `[...]` is optional. `partscript ref` prints a
one-page version of this.

## Top level

| statement | |
|---|---|
| `prop NAME ["Title"] [subcategory] [option=...]` | a model. Options below |
| `def NAME [param=default ...]` | a part with parameters, drawn with `use NAME` or called as `NAME ...` |
| `import "PATH" [as NAME]` | bring in a file or a folder's files, from this file's folder; `as` keeps their names as `NAME.x` |
| `set NAME=VALUE ...` | variables: for its own file when unindented, else for the rest of the block |
| `kit NAME [option=...]` | a set of building pieces ([buildings](../language/buildings.md)) |
| `building NAME "Title" kit=KIT [cutaway=open]` | a building laid out room by room |
| `style NAME [exterior=1]` | a room style for a host's dressing tools ([embedding](../engine/hosts.md#styles-and-dressing)) |
| `dressing BUCKET piece[:radius] ...` / `dressing signs KIND=piece` | pieces for a host's dressing buckets |
| `end` | closes a block early (never needed) |

### prop options

| option | |
|---|---|
| `hero=1` · `budget=N` | triangle budget 6,000 · N (default 2,500) |
| `seed=WORD` | deals every draw again (from outside the file: `Project.build(name, seed=)`, `partscript build --seed`) |
| `px=auto\|N` | texel density: sized to the prop, or N texels a metre |
| `ao=auto\|H` | grounded shading over the prop's height, or H metres (default 1.4) |
| `for v=a,b,c` | one prop per value, `{v}` replaced by it |
| `desc=` `dd=` `mount=` `col=` `replace=1` | metadata for a host |

## Shapes

| statement | arguments | its own options |
|---|---|---|
| `box` (`b`) | `at= size= mat=` | `taper=TX,TY` `lean=DX,DY` `skip=+z,-y,...` `from= to=` `break=` `chunk=` `core=` `rubble=` `shade=` |
| `bevel_box` (`bb`) | `at= size= bevel= mat=` | `taper=` `lean=` `from= to=` |
| `box_between` (`bx`) | `from=LO to=HI mat=` | |
| `cylinder` (`c`) | `at= radius= height= mat=` | `sides=` `top_radius=` `axis=x\|y\|z` `arc=A0,A1` `caps=0` `cap_mat=` `from= to=` |
| `cone` | `at= radius= height= mat=` | `sides=` `axis=` `from= to=` |
| `sphere` (`sph`) | `at= radius= mat=` | `sides=` `rings=` |
| `tube` | `at= radius= inner= height= mat=` | `sides=` `axis=` |
| `wedge` | `at= size= mat=` | |
| `lathe` | `at= mat=` then `r:z r:z ...` | `sides=` `cap=1` `arc=A0,A1` |
| `extrude` (`ext`) | `at= width= mat=` then `u:v u:v ...` | `axis=x\|y\|z` `taper=` |
| `pipe` | `mat= radius=` then `P P ...` (or a variable) | `sides=` `closed=1` `smooth=` `taper=` `arc=CX,CZ,R,A0,A1[,N]` |
| `sweep` | `mat= prof=x:y,x:y,...` then `P P ...` | `open=1` `closed=1` `smooth=` `taper=` `arc=` |
| `face` | `mat=` then `P P P ...` | `double=1` |
| `panel` (`pan`) | `at= width= height= mat=` | `double=1` |
| `decal` (`trim`) | `at= width= height= cell=` | `m=` (its material) |
| `label` | `at= width= height= text=` | `sub=` `bg=` `fg=` `wrap=` `sides=` `mark=` `accent=` `tex=` `double=1` |
| `sign` | `at= width= height= text=` | `sub=` `bg=` `fg=` `lit=` `tex=` `double=1` |
| `torus` | `at= radius= thick= mat=` | `sides=` `rings=` `axis=` |
| `frame` | `at= width= height= depth= mat=` | `hole=w,h` `hole_at=x,z` |
| `arch_wall` (`archwall`) | `at= width= height= thick= mat=` | `open=` `spring=` `sides=` |
| `vault` | `at= width= depth= rise= mat=` | `sides=` |
| `terrain` | `at= size= mat=` | `height=` `cells=` `steep=` `slope=` `skirt=0` |

A shape without `at=` stands at the origin. Pages: [shapes](../language/shapes.md),
[materials and text](../language/materials.md), [generators](../language/generators.md).

## Structure

| statement | |
|---|---|
| `use NAME [at=] [param=value ...]` | draw a part, std part, prop or host asset; `from= to=` lays it between two points; `scale=` |
| `NAME [at=] [param=value ...]` | the same as `use NAME ...`: a part called like a shape |
| `group [at=] [turn=] [scale=] {` ... `}` (`at`) | move, turn or scale lines together |
| `if EXPR {` ... `}` | lines kept only while EXPR holds |
| `row x\|y\|z\|-x... [gap=] [over=] [pack=start\|centre\|end] [align=...] [at=] [on=] {` ... `}` | copies end to end ([placing](../language/placing.md#row-and-stack)) |
| `stack [gap=] ... {` ... `}` | a row going up |
| `NAME = LINE` | name what a line makes |
| `part NAME [smooth=1] [pivot=C] [parent=PART]` | start another mesh; a pivot it turns about and a part it hangs from ([moving parts](../language/moving.md)) |
| `mark NAME at=C [on=PART] [turn=X,Y,Z]` | a named point for a game (a muzzle, a grip): an empty node ([moving parts](../language/moving.md)) |
| `size W,D,H` | expected bounds (warns when off by 15%) |
| `card where= pairs= look= avoid= [what=] [notes=]` | the prop's library card |

## Snaps, paths and links

| statement | |
|---|---|
| `snap NAME C DIR [up=DIR] [kind=WORD]` | a connection point ([paths](../language/paths.md)) |
| `chain A B*3 C [at=] [turn=]` | pieces end to end, start snap on end snap |
| `link KIND at=C toward=DIR` | a loose end |
| `join KIND [reach=.6] [radius=.025] [sides=6] [mat=] [bulge=] [with=DEF]` | bridge loose ends that nearly meet |

## Buildings

Inside a `building`:

| statement | |
|---|---|
| `room X,Y W,D [storeys=] [storey=] [walls=] [floor=] [roof=none] [theme=] [as=] [fill=DEF ...]` | a room on the grid |
| `walls KIND [storey=N,M\|all] [side=s,n,w,e]` | outside wall kind for storeys and sides |
| `open X,Y SIDE KIND [storey=]` | one wall (X or Y may be a range `0..2`) |
| `stair X,Y [DIR] [storey=]` | the kit's stair |
| `roof [PIECE\|none] [parapet=PIECE\|none]` | roof and parapet pieces |
| `attach PIECE to=TARGET [at=] [via=] [spin=] [slide=A,B] [as=]` | snap a piece onto another |
| `place PIECE C [turn=] [as=]` | a piece at a position |

Inside a `kit`: `wall KIND=PIECE ...`, `piece ROLE=PIECE ...` (floor roof parapet partition corner stair),
`snap PIECE NAME C DIR [up=] [kind=]`. Kit options: `grid=` `storey=` `wall=` `parapet_lift=` `stair_cells=`
`stair_exit=ahead|back` `walls=SET`.

## Options any line takes

| option | |
|---|---|
| `turn=RX,RY,RZ` (`r=`) | degrees about x, then y, then z |
| `when=EXPR` | keep the line, or each copy, only while EXPR holds |
| `on=NAME` · `on=NAME.side` | stand it on a named shape (`at=` from the middle of its top), or against its front, back, left, right or bottom |
| `fade=F` | darker toward its base (F at the bottom) |
| `wobble=A[,B,C]` | corners moved up to A, differently each copy |
| `twist=DEG` `shrink=K` `bend=DEG[,HEADING]` | reshape it by height ([bending](../language/shaping.md)) |
| `smooth=N` | a curve through the points (pipe, sweep, along) |
| `drop=1\|lean` `sink=D` | fall onto what is under it ([generators](../language/generators.md#dropping-things-drop)) |
| `jitter=P[,D]` (`jit=`) | copies nudged and turned at random |
| `hang=S` | a `repeat` row sags S at its middle |
| `along=` `every=` `fit=1` `joints=1` `corners=1` `closed=1` | copies down a line of points |
| `facing=up\|side\|down\|any` | with `scatter on`: which faces |
| `shade=K` | a box's faces K times as bright |

## Copy phrases

| readable | shorthand | |
|---|---|---|
| `repeat N [every X,Y,Z]` | `*N@X,Y,Z` | N in a line |
| `grid AxB[xC] every X,Y[,Z]` | `*AxB@X,Y` | a grid |
| `ring N [step D]` | `*N%D` | turned round the vertical |
| `scatter N over W,D [apart G]` | `*N~W,D,G` | over an area |
| `scatter N within R [apart G]` | `*N~R,0,G` | over a disc |
| `scatter N on NAME [facing F] [apart G]` | `*N^NAME,G` | over a named shape's faces |
| `mirror x\|y\|z\|xy...` | `mx my mz` | and its mirror image |
| `as k` · `as col,row[,layer]` | `index=` | name the copy's number |
