# Mistakes and what they mean

Every mistake comes with its file and line (`props/a.parts:12: ...`), from `partscript check`, from a
build, and in the playground's margin. The message says what to do; this page says a little more.

## "size= is missing (it takes at= size= mat=)"

A shape is missing something it needs. The message lists what that shape takes. Positions default to
the origin; sizes, radii and materials do not.

## "unknown material 'woood'"

The material is not a library key and not a colour. Check the spelling, `partscript list props/` for the
keys your host knows, or write a colour: `#8a6a4a/wood`.

## "colour finish 'shiny': one of paint, metal, ..."

A colour's finish (after the `/`) must be one of the [finishes](../language/materials.md#finishes).

## "unknown name w in 'w'"

An expression names a variable that is not set here. Variables are set with `set`, by a part's
parameters, or, in a room's `fill=` part, by the room (`w`, `d`, `h`...). A variable set inside one
prop is not seen by another.

## "'0,0': expected 3 numbers"

Positions and sizes are three numbers, or one for all three. Two are allowed only after `on=` (where
the height is the surface's) and in `scatter ... over W,D`.

## "'or' on its own: an expression with spaces goes in quotes"

An option is one word: `when=i<3 or i>7` reads as three words. Quote it: `when="i<3 or i>7"`.

## "turn= and r= are the same option; give one"

Every option has a long name and a short one (`turn=` and `r=`, `sides=` and `s=`); a line gave both.

## "no sides= (sides= is for round shapes)"

`sides=` is for cylinders, cones, spheres, tubes, lathes, pipes, sweeps, arches, vaults and tori (and a
wrapped label). `scale=` is for `use` and `group`.

## "def stool defined twice (also props/a.parts:4)"

Two `def`s have the same name, in one file or two. Rename one, or keep a library's names apart with
`import ... as NAME`.

## "import "lib/furniture.parts": no such file or folder"

The path is from the importing file's folder (`../lib/...` to go up one). `.parts` may be left off.

## "unknown statement 'bxo'"

A line starts with a word that is not a shape, a part, a std part or a prop. A part called by name is
`NAME ...` or `use NAME ...`; check the spelling.

## "'use tabel': no def, std part or prop of that name"

Check the spelling. `partscript list` shows the std parts; a prop or def in any file of the project
can be used.

## "'table' has no parameter 'colour' (it takes w, d, h, top, leg, t)"

The part was given a parameter it does not have. Its parameters are listed.

## "on=desk: no shape of that name above this line"

Names are read after the line that makes them, in the same block (or one inside it). Name the shape
first: `desk = box ...`.

## "desk.middle: a named shape gives left, right, front, ..."

The numbers a name gives are `left right front back bottom top x y z w d h`; points are `desk`,
`desk.top`, `desk.top_left_front`... (see [placing](../language/placing.md)).

## "900 copies; keep arrays under 400"

One line may make at most 400 copies. Split it, or use a part that makes copies of its own.

## "about 10800 triangles, over the 2500 prop budget"

A warning, not an error. Give the prop `hero=1` (6,000) or `budget=N`, or make it cheaper: fewer sides,
fewer copies, plain boxes where bevels do not show.

## "unclosed block (add a '}' line)"

A `group`, `if`, `row` or `stack` opened with `{` and was never closed. Every `{` needs a `}` on a line
of its own (or at the end of a one-line block).

## "'box' outside a prop, def, kit or building"

A shape line is not indented under a block, or the block above it ended (an unindented `set`, or a new
`prop`). Indent it under its prop.

## "prop name 'A': lower_snake_case"

Prop and part names are lower case, digits and underscores, starting with a letter.

## "prop wheel has the name of a std part; 'use wheel' draws the std part"

A warning: a prop shares its name with a part, so `use` of that name draws the part. Rename the prop.

## Things that are not errors but look wrong

- **Half a shape is under the floor.** `at=` is a shape's middle. Use `at=x,y,on` (or `on(z)`, or
  `on=NAME`) to sit it on something.
- **A ring of copies is in a clump.** `ring` turns copies about the line's origin: put the shape at its
  radius (`at=.5,0,0`). See [copies](../language/copies.md#rings-turn-about-the-origin).
- **Everything is the same.** `rand()` inside a line without copies draws once. Give the line copies, or
  put it in a part used many times.
- **Noise is the same everywhere.** `here` is where a copy's copy phrase puts it, before its own `at=`:
  copies placed only by `at=` with `i` all have the same `here`.
