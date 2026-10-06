# Basics

## Files

A `.parts` file holds any number of props, parts (`def`), variables (`set`), and, for buildings,
kits and buildings. A project is every `.parts` file in the folders you build, plus any file they
[import](reuse.md#importing-files), plus the [standard parts](../reference/std-parts.md): a prop or part
defined in one file can be used from any other. Each name is defined once; a second `def` of the same
name is a mistake.

```parts
# Comments start with a hash and a space.
set oak=#7a5a32/wood

def plank len=1
  box at=len/2,0,0 size=len,.12,.02 mat=oak

prop bench_demo "Bench" garden
  plank at=-.8,-.12,.45 len=1.6 repeat 3 every 0,.13,0
  box at=-.7,0,on size=.06,.4,.44 mat=oak mirror x
```

A block (`prop`, `def`, `kit`, `building`) runs until the next unindented line. Its lines are
indented, by any amount. `end` closes a block early, and is never needed.

## Lines

One statement per line. The first word says what it is (a shape like `box`, or `use`, `group`,
`set`, ...). Then come its arguments by name (`at=0,0,1`), its options (`turn=0,0,45`), and copy
phrases (`repeat 5 every .3,0,0`). Their order does not matter, except that copy phrases are written as
phrases.

- `;` separates two statements on one line.
- `# ` (a hash and a space) starts a comment. A hash with no space after it is a colour: `#c84a3a`.
- A line ending in `\` carries on onto the next line.
- Positions and sizes are comma lists with **no spaces**: `at=0,.5,1.2`. A single number stands for all
  three: `size=.4` is a .4 m cube.
- An option is one word. An expression with spaces in it goes in quotes: `when="i<3 or i>7"`.

## Props

```text
prop NAME ["Title"] [subcategory] [option=value ...]
```

NAME is lower_snake_case and is the model's asset id (the `.glb` is `NAME.glb`). The title and the
subcategory are for people and catalogues. Header options:

| option | what it does |
|---|---|
| `hero=1` | a larger triangle budget (6,000 instead of 2,500) |
| `budget=N` | this prop's triangle budget |
| `seed=N` | deals every `rand()`, `pick()`, `odds()` and `scatter` again: a new take on the same prop (`Project.build(name, seed=N)` and `partscript build --seed N` do it without editing the file) |
| `px=auto` | texel density sized to the prop (about 80 texels across its longest side) |
| `ao=auto` | grounded shading faded over the prop's own height |
| `for v=a,b,c` | variants: the prop is made once per value, with `{v}` replaced by it everywhere |
| `desc="..."`, `dd=`, `mount=`, `col=`, `replace=1` | metadata a [host](../engine/hosts.md) may read |

Variants make a family of props from one block:

```parts
prop traffic_cone_demo_{c} "Traffic Cone" street for c=e25a1a,d8c020,3a8ae2
  cylinder radius=.16 height=.64 mat=#{c}/plastic top_radius=.03 sides=12
  bevel_box size=.42,.42,.05 bevel=.015 mat=rubber
```

That is three props: `traffic_cone_demo_e25a1a` and two more.

## Variables

`set` names a value. Unindented, it holds for its own file (and the parts defined there, wherever they
are used); indented, for the rest of its block.

```parts
set paint=#2a4a6a/paint
prop locker_demo "Locker"
  set w=.4 h=1.8
  box size=w,.5,h mat=paint
  box at=0,-.255,h*.8 size=w*.8,.01,.12 mat=steel_dark repeat 4 every 0,0,-.05
```

A variable can hold a number, a material, a word, a line of points, or an expression that is worked
out where it is used.

## Expressions

Anywhere a number goes, an expression can: `w/2`, `-h+.1`, `sin(30)*r`, `max(a,b)`,
`atan(rise/run)`. Angles are degrees. The [expressions reference](../reference/expressions.md) lists
every function, and the special names (`i`, `here`, `rand()`, `noise()`).

## Two forms

Every shape line can name its arguments, the readable form these docs use, or give them in order,
a shorthand that is quicker to type once you know it:

```text
box at=0,0,on size=.4,.3,.2 mat=wood turn=0,0,45 repeat 5 every .3,0,0 mirror x
b 0,0,~ .4,.3,.2 wood r=0,0,45 *5@.3,0,0 mx
```

| readable | shorthand |
|---|---|
| `box`, `bevel_box`, `box_between`, `cylinder`, `sphere`, `panel`, `extrude`, `group`, `decal`, `arch_wall` | `b`, `bb`, `bx`, `c`, `sph`, `pan`, `ext`, `at`, `trim`, `archwall` |
| `turn=` `sides=` `scale=` `top_radius=` `axis=` `jitter=` | `r=` `s=` `s=` `rt=` `ax=` `jit=` |
| `on`, `on(z)` in a position | `~`, `~z` |
| `repeat N every V` · `grid AxB every V` · `ring N [step D]` | `*N@V` · `*AxB@V` · `*N%D` |
| `scatter N over W,D [apart G]` · `within R` · `on NAME` | `*N~W,D,G` · `*N~R` · `*N^NAME` |
| `mirror xy` | `mx my` |

Both parse to the same thing. `partscript fmt` rewrites a file into either form.
