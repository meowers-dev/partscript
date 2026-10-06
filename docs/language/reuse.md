# Parts made of parts

## def and use

`def` names a group of lines with parameters; `use` draws it. A part is drawn in its own space: its
origin goes where the `use` puts it, turned and scaled with it.

```parts
def bollard_demo h=.9 band=#e8c040/paint
  cylinder radius=.09 height=h mat=#2a2a2a/paint sides=8
  cylinder at=0,0,h*.8 radius=.095 height=.06 mat=band sides=8
  sphere at=0,0,h radius=.09 mat=#2a2a2a/paint sides=8 rings=3

prop bollards_demo "Bollards"
  use bollard_demo at=-1,0,0
  use bollard_demo at=0,0,0 h=1.2
  use bollard_demo at=1,0,0 band=#c83a3a/paint turn=0,15,0
```

Parameters always have defaults. A `use` line gives any it wants to change, by name, and any number
or expression works: `use table w=rand(.8,1.2)`. A parameter can hold a material (`band=`), a word, or
`none` to leave the shapes that use it out.

A parameter's default can be a draw: `def headstone lean=rand(-8,8)` leans every stone its own way unless
a `use` line says otherwise.

`use` options that place the part (`at=`, `turn=`, `scale=`, `on=`, `from=`/`to=`, `drop=`, the copy
phrases...) are not passed to it, so a part cannot have a parameter called `turn`, `scale`, `when`, `fade`,
`wobble`, `jitter`, `along` and so on. If a part does have a parameter called `drop`, `on`, `sink`,
`facing`, `from` or `to`, the part gets it.

## Calling a part by name

A part (or a prop) can be called like a shape, without `use`: its name, then where and how.

```parts
def stool_call_demo h=.6 seat=#8a2a2a/paint
  cylinder at=0,0,on(h) radius=.17 height=.04 mat=seat sides=10
  box at=.1,0,on size=.03,.03,h mat=steel_dark ring 3

prop bar_demo "Bar Stools"
  stool_call_demo at=-.5,0,0
  stool_call_demo at=0,0,0 h=.7 seat=#2a4a6a/paint
  tall = stool_call_demo at=.5,0,0 h=.8
  mug on=tall
  crate at=1.2,0,0 fill=#d8a030/plastic
```

It is the same as `use`, word for word (`stool_call_demo at=0,0,0` is `use stool_call_demo at=0,0,0`),
so a library of parts reads like more shapes. A word that is not a shape, a part or a prop is reported as
an unknown statement.

## What use can draw

- a `def` in any file of the project;
- a [standard part](../reference/std-parts.md): `table`, `chair`, `shelf`, `crate`, `barrel`...;
- another prop, built whole and placed as one piece, with its loose ends;
- an asset the host provides, as `pack__id` (see [embedding](../engine/hosts.md)).

When a prop and a def share a name, `use` draws the def; `check` warns. A `def` with the name of a std part
replaces it (`check` warns about that too).

## Importing files

```text
import "path/to/file.parts"
import "path/to/folder"
import "path/to/file.parts" as NAME
```

`import` brings another file into the project, or every `.parts` file in a folder; the path is from the
importing file's folder, and `.parts` may be left off. A file is self-contained when it imports what it
uses: it builds from anywhere, whichever folder is built. Each file comes in once, however many files
import it. The props in an imported file can be used but are not built on their own by `partscript build`.

`import ... as NAME` keeps a library's names apart from yours: its parts, props and kits are
`NAME.table`, `NAME.chair`, `NAME.flat`, so two libraries can both have a `table`. Inside the library its
own names still work as they are (its `table` uses its own `leg`, not yours).

```text
import "../lib/furniture.parts" as furniture

prop dining_room "Dining Room"
  furniture.table at=0,0,0
  furniture.chair at=0,-.6,0 repeat 4 every .5,0,0
  building ... kit=furniture.flat
```

A file's variables (its unindented `set` lines) stay with it: a library's colours are its own, and a part
draws with the variables of the file it was written in.

The project has a worked example: `examples/tavern.parts` builds a tavern room from two libraries in
`library/`. It imports `furniture.parts` with a name and calls its parts like shapes
(`furniture.table`, `furniture.chair`, `furniture.keg`), and imports `tableware.parts` plainly
(`tankard`, `candlestick`). It also has a `settle` and an `oak` of its own, which leave the library's
`furniture.settle` and its oak untouched.

## group

`group` moves, turns or scales the lines inside it as one, and `if` keeps them only while something
holds. Groups nest.

```parts
prop sign_post_demo "Sign Post"
  box at=0,0,on size=.08,.08,2.2 mat=wood
  group at=0,0,1.9 turn=0,0,30 {
    box at=.4,0,0 size=.8,.04,.18 mat=#f0ece0/paint
    label at=.4,-.021,0 width=.74 height=.14 text="CHAPEL" bg=#f0ece0 fg=#2a2a2a
    label at=.4,.021,0 width=.74 height=.14 text="CHAPEL" bg=#f0ece0 fg=#2a2a2a turn=0,0,180
  }
  group at=0,0,1.6 turn=0,0,195 scale=.9 {
    box at=.4,0,0 size=.8,.04,.18 mat=#f0ece0/paint
    label at=.4,-.021,0 width=.74 height=.14 text="FERRY" bg=#f0ece0 fg=#2a2a2a
    label at=.4,.021,0 width=.74 height=.14 text="FERRY" bg=#f0ece0 fg=#2a2a2a turn=0,0,180
  }
```

## Parts that use themselves

A part may use itself, with a `when=` to stop it: a branch carries smaller branches down to depth 0.
Parts nest at most 16 deep.

```parts
def twig_demo len=1 rad=.06 depth=3
  cylinder radius=rad height=len mat=#5a3a22/wood top_radius=rad*.7 sides=5
  twig_demo at=0,0,len*.9 turn=rand(25,40),0,i*120+rand(-20,20) scale=.7 len=len rad=rad depth=depth-1 \
      when=depth>0 ring 3 step 0
  sphere at=0,0,len radius=len*.4 mat=#3a6a2a/fabric sides=6 rings=4 when=depth==0

prop little_tree_demo "Little Tree"
  twig_demo len=1.2 rad=.1
```

## More than one mesh: part

Normally a prop is one mesh. `part NAME` starts another; the lines after it draw into it. `part NAME
smooth=1` shades it smooth (organic shapes) instead of faceted. Engines see each part as its own node.

## size and card

- `size W,D,H` says how big the prop should be; the build warns when its bounds differ by more than
  15%. A guard against a slip of a decimal point.
- `card where="..." pairs="a, b" look="..." avoid="..."` is the prop's library card: notes a host's
  tools can show (where it goes, what it pairs with). It draws nothing.

## Budgets

`check` counts triangles. A prop's budget is 2,500, `hero=1` gives 6,000, `budget=N` sets it, and a
building's is 200,000. Over budget is a warning, not an error. In the preview, *Built from* lists every
part a prop uses, all the way down, with how many copies.
