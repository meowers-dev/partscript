# Cookbook

Short recipes for things that come up. Each is a whole prop: open it with **try it** and adapt it.

## A table with things on it

Name the top; stand things on it with `on=`.

```parts
prop recipe_table "Laid Table"
  top = box at=0,0,on(.74) size=1.4,.8,.04 mat=#6a4a2a/wood
  box at=top.left+.06,top.front+.06,on size=.05,.05,.74 mat=#5a3a22/wood mirror xy
  cylinder on=top at=-.4+i*.4,-.22 radius=.11 height=.012 mat=#e8e4dc/plastic sides=12 repeat 3 every 0,0,0
  mug on=top at=.3,.15
  bottle = cylinder on=top at=0,.2 radius=.04 height=.28 mat=#2a5a2a/glass sides=6
```

## A shelf of books

A `row` packs things of any size tight; `align=back` lines their backs up.

```parts
prop recipe_books "Shelf of Books"
  shelf = box at=0,0,on(1) size=1.2,.25,.03 mat=wood
  row x on=shelf align=back {
    box size=rand(.025,.06),rand(.15,.21),rand(.18,.27) mat=pick(#8a2a2a,#2a4a6a,#3a6a3a,#c8a040,#4a2a4a)/paint repeat 18
  }
```

## A pile of things

Drop copies from above: they land on each other. Spread them a little wider than the pile you want, or
they stack up into a tower.

```parts
prop recipe_pile "Pile of Sacks"
  sack at=0,0,3 turn=rand(-15,15),rand(-15,15),rand(0,360) drop=lean scatter 10 within 1.1 apart .3
```

## A fence round a yard

One line of points carries the panels and the posts.

```parts
prop recipe_fence "Paddock"
  set edge="0,0 8,0 8,5 0,5"
  box at=4,2.5,-.02 size=8.4,5.4,.04 mat=#4e6a2c/fabric
  box at=0,0,.6 size=2,.06,.1 mat=wood repeat 2 every 0,0,-.35 along=edge every=2 fit=1 closed=1
  box at=0,0,on size=.12,.12,.9 mat=wood along=edge every=2 fit=1 closed=1 joints=1
```

## A hanging sign

A group puts the sign's origin at its hook, so turning it (`turn=`) swings it about the hook.

```parts
prop recipe_sign "Inn Sign"
  box at=0,0,on size=.12,.12,3 mat=#3a2a1e/wood
  box at=.5,0,2.8 size=1.1,.08,.08 mat=#3a2a1e/wood
  group at=.75,0,2.76 turn=6,0,0 {
    cylinder from=-.25,0,0 to=-.25,0,-.2 radius=.01 mat=steel_dark sides=3 mirror x
    box at=0,0,-.55 size=.7,.05,.7 mat=#2a4a3a/paint
    label at=0,-.026,-.55 width=.62 height=.4 text="THE CROW" sub="FREE HOUSE" bg=#2a4a3a fg=#e8d8a0
  }
```

## Wear and tear

`wobble=` for hand-made, `fade=` for grounded, `break=` for broken, `scatter on` for growth.

```parts
prop recipe_wear "Old Gate Pier"
  pier = box size=.6,.6,2 mat=#b4ac9a/stone core=#6e675c/stone break=.15 chunk=.2 wobble=.01 fade=.7
  sphere on=pier radius=.25 mat=#b4ac9a/stone sides=8 rings=4 wobble=.02
  box size=.1,.1,.02 mat=#3a6a2a/fabric turn=0,0,rand(0,360) scatter 30 on pier facing up apart .06
  box size=.1,.12,.02 mat=#4a7a32/fabric turn=0,0,rand(0,360) scatter 25 on pier facing side
```

## A hill with a path up it

Terrain from noise; flags dropped along a line onto it.

```parts
prop recipe_hill_path "Hill Path" hero=1
  terrain size=12,12 cells=24 mat=#4e6a2c/fabric height=max(0,3-hypot(x,y)*.5)+rough(x*.3,y*.3)*.5
  box size=.5,.4,.08 mat=#9a9488/stone turn=0,0,rand(-20,20) wobble=.02 drop=lean sink=.04 \
      along="0,-6 .5,-3.5 -.4,-1.5 0,0" every=.6
```
