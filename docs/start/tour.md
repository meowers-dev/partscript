# A tour: one lamp, eight ideas

This page builds a desk lamp and the desk it stands on, adding one idea at a time. Each step is a
whole prop; open any of them with **try it** and change the numbers.

## 1. Shapes

A shape is a word, then what it needs by name. Positions are `x,y,z` with no spaces.

```parts
prop tour_1 "Tour 1"
  box at=0,0,.72 size=1.2,.6,.04 mat=wood
  cylinder at=.4,.1,.75 radius=.08 height=.03 mat=steel_dark sides=8
```

`at=` is the middle of the shape. That puts half the desk top below .72 and half the lamp base
inside the desk.

## 2. Sitting things on things

`on` in a position sits the shape on that height instead of centring it there: `on` alone is the floor,
`on(.72)` is .72 m up. Even better, name a shape and put things **on** it:

```parts
prop tour_2 "Tour 2"
  top = box at=0,0,on(.72) size=1.2,.6,.04 mat=wood
  base = cylinder on=top at=.4,.1 radius=.08 height=.03 mat=steel_dark sides=8
```

`on=top` measures `at=` from the middle of `top`'s top surface, and sits the shape on it. Nothing
needs adding up.

## 3. Copies

Legs: one line, four copies. `mirror xy` copies a shape across x = 0 and across y = 0.

```parts
prop tour_3 "Tour 3"
  top = box at=0,0,on(.72) size=1.2,.6,.04 mat=wood
  box at=top.left+.05,top.front+.05,on size=.05,.05,.72 mat=wood mirror xy
```

`top.left` and `top.front` are numbers read off the named shape: its left edge and its front edge.

## 4. Lines between points

The lamp's arm runs from its base to an elbow, and its shade points back at the elbow. `from=` and
`to=` take points, and a named shape gives points too: `base.top` is the middle of its top.

```parts
prop tour_4 "Tour 4"
  top = box at=0,0,on(.72) size=1.2,.6,.04 mat=wood
  base = cylinder on=top at=.4,.1 radius=.08 height=.03 mat=steel_dark sides=8
  elbow = sphere at=.34,.15,1.18 radius=.02 mat=steel_dark sides=6 rings=3
  cylinder from=base.top to=elbow radius=.012 mat=steel_dark sides=5
  cone from=.55,.08,1.05 to=elbow radius=.08 mat=#c84a3a/paint sides=8
```

`#c84a3a/paint` is a material made from a colour and a finish.

## 5. Parts with parameters

Name a group of lines with `def`, then call it by its name like a shape, anywhere. Parameters have defaults.

```parts
def tour_lamp shade=#c84a3a/paint
  base = cylinder radius=.08 height=.03 mat=steel_dark sides=8
  elbow = sphere at=-.06,.05,.45 radius=.02 mat=steel_dark sides=6 rings=3
  cylinder from=base.top to=elbow radius=.012 mat=steel_dark sides=5
  cone from=.15,-.02,.32 to=elbow radius=.08 mat=shade sides=8

prop tour_5 "Tour 5"
  top = box at=0,0,on(.72) size=1.2,.6,.04 mat=wood
  tour_lamp on=top at=.4,.1
  tour_lamp on=top at=-.4,.1 shade=#3a6a9a/paint
```

## 6. Rows and variety

`row x` sets copies side by side along x. `rand()` and `pick()` draw something new for every copy.

```parts
prop tour_6 "Tour 6"
  top = box at=0,0,on(.72) size=1.2,.6,.04 mat=wood
  row x on=top at=-.3,.2 align=back {
    box size=rand(.025,.06),rand(.15,.21),rand(.2,.27) mat=pick(#8a2a2a,#2a4a6a,#3a6a3a,#c8a040)/paint repeat 9
  }
```

Every book has its own thickness, height and colour, and the row packs them against each other
whatever sizes they came out.

## 7. Stacks

`stack` piles things up, each on the last.

```parts
prop tour_7 "Tour 7"
  top = box at=0,0,on(.72) size=1.2,.6,.04 mat=wood
  stack on=top at=-.2,-.1 {
    box size=.3,.21,.004 mat=#f0ece0/paint turn=0,0,rand(-14,14) repeat 7
  }
```

## 8. Whole places

The same few ideas build scenes. Every line below is one you have seen, plus `terrain` and `drop=`:

```parts
prop tour_8 "Tour 8"
  terrain size=10,10 cells=20 mat=#4e6a2c/fabric height=rough(x*.2,y*.2)*1.6
  dead_tree at=1,1,5 drop=1
  grave kind=pick(0,1,3) drop=lean sink=.1 scatter 9 over 8,8 apart 1.6
```

From here: [the language](../language/basics.md) page by page, or the [cookbook](../guides/cookbook.md)
for recipes.
