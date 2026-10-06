# Materials and text

`mat=` says what a shape is made of. It can be:

- a key from the [material library](../reference/materials.md): `wood`, `steel_dark`, `brick`, `glass`,
  `lamp_warm`... (`partscript list` shows every key your host knows);
- a colour and a finish: `#c84a3a/paint`, `#3a6a2a/fabric`, `#ffcc70/glow`;
- a variable holding either: `set oak=#7a5a32/wood`, then `mat=oak`;
- several, cycled per copy: `mat=#c83a3a/fabric|#ece0c0/fabric` makes stripes;
- one drawn per copy: `mat=pick(#8a2a2a,#2a4a6a,#3a6a3a)/paint`;
- `none`, which leaves the shape out (useful for optional pieces of a part: `use crate fill=none`).

## Finishes

A colour material is the colour with a little texture for its finish, dithered and quantized so it
reads at PSX resolutions:

| finish | for |
|---|---|
| `paint` | painted anything (the default when the finish is left off) |
| `metal` | bare metal |
| `plastic` | mouldings, toys, signs, food |
| `rubber` | tyres, seals, shoes |
| `fabric` | cloth, cushions, leaves, grass, moss |
| `wood` | timber with grain |
| `plaster` | walls and ceilings |
| `concrete` | slabs, kerbs, roads |
| `stone` | blocks of stone (coursed) |
| `glow` | lit and full bright: bulbs, flames, screens, lit windows |
| `glass` | see-through |

```parts
prop finishes_demo "Finishes"
  box at=-2.25+i*.5,0,on size=.4 mat=#8a6a4a/paint|#8a6a4a/metal|#8a6a4a/plastic|#8a6a4a/rubber|#8a6a4a/fabric|#8a6a4a/wood|#8a6a4a/plaster|#8a6a4a/concrete|#8a6a4a/stone|#8a6a4a/glow \
      repeat 10 every 0,0,0
```

## Cycles and picks

`a|b|c` takes the next material for each copy of a line, round and round; it is how the fairground's
canopy gets its stripes. `pick(a,b,c)` draws one at random for each copy, and the same file always
draws the same. Both work in a `def` parameter too, so whatever the part draws takes it:

```parts
def stool_demo col=pick(#c84a3a,#3a6a9a,#e8c040)/paint
  cylinder at=0,0,on(.6) radius=.17 height=.04 mat=col sides=10
  box at=.1,0,on size=.03,.03,.6 mat=steel_dark ring 3

prop stools_demo "Stools"
  stool_demo repeat 5 every .5,0,0
```

## Text: labels and signs

```text
label at=C width=W height=H text="TEXT" [sub="small line"] [bg=#hex] [fg=#hex]
sign  at=C width=W height=H text="TEXT" [sub=] [bg=] [fg=] [lit=1.1] [tex=256x32]
```

Both draw text on a quad facing the front, in a pixel font. A `label` is print: number plates, name
plates, packaging, gravestones, dials; it is shaded like the rest of the prop. A `sign` is a lit
shop sign, full bright (`lit=` is how bright). The texture is sized to the quad's shape unless
`tex=WxH` says (each 16 to 256).

Text can be different on every copy: `pick()` chooses words, and `{expression}` puts a number in.
A variable holding text drops in with `{name}`.

```parts
set first="pick(ADA,NELL,SILAS,RUTH,EZRA)"
prop name_plates_demo "Name Plates"
  box at=0,.03,on size=.6,.05,.25 mat=brass repeat 3 every 0,0,.3
  label at=0,0,.125 width=.56 height=.21 text="{first} ASHBY" sub="{floor(rand(1820,1899))}" bg=#d8c070 fg=#2a2a2a \
      repeat 3 every 0,0,.3
```

A label round a cylinder (a can, a tin, a barrel) is a sleeve: `wrap=RADIUS` with `sides=`, and a
printed `mark=bolt|orbit|star|wave` in an `accent=#hex` colour. Put it a millimetre outside a closed
body.

```parts
prop tin_demo "Tin"
  cylinder radius=.04 height=.12 mat=steel_grey sides=12
  label at=0,0,.06 width=.25 height=.08 text="BEANS" wrap=.041 sides=12 mark=star accent=#e8c040 bg=#c83a2a fg=#ffffff
```

## See-through and lit

`glass` (the library key, or `#rrggbb/glass`) is see-through and draws both sides when it is a
`panel ... double=1`. `glow` materials and the library's `lamp_*` keys are lit: they skip the baked
shading and stay full bright.
