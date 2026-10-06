# Tools: check, build, fmt, preview, playground

## The command line

```sh
partscript ref                        # the language, in one page
partscript check props/               # parse and check every .parts file under props/ (no building)
partscript build props/ -o out/       # every prop as out/<id>.glb
partscript build props/ -o out/ --only crate_stack,street_lamp
partscript build props/ -o out/ --only weathered_fence --seed 4817   # another deal of its draws
partscript list props/                # props, std parts and materials
partscript fmt props/a.parts          # print it in words (the readable form)
partscript fmt props/a.parts -w       # ...and write it back
partscript fmt props/a.parts --terse  # print it in shorthand
```

### check

`check` reads every file, follows every `use`, and reports mistakes with the file and line, without
building anything. It also estimates each prop's triangles and warns when a prop goes over its budget
(2,500 for a prop, 6,000 with `hero=1`, or `budget=N`).

```text
  crate_stack                              ~    36 tris  props/crates.parts:1
error: props/crates.parts:4: unknown material 'woood' (partscript materials lists them; or use #rrggbb/finish)
1 props, 1 errors, 0 warnings
```

`--json` prints the same as data. Exit status is 1 when there are errors.

### build

`build` writes one `.glb` per prop (buildings excepted: see [buildings](../language/buildings.md)), for the
files in the folders it is given; the props of files they only `import` are used, not built.
Textures are made once and cached in `~/.cache/partscript/textures` (`--cache DIR` moves it,
`--no-cache` makes them afresh).

`--seed S` deals every prop it builds again with that seed, as if each said `seed=S`, without touching the
files: one more variant of a weathered fence or a graveyard, the same every time for the same seed. Each
build writes the same file names, so give each seed its own `-o` folder.

```text
  crate_stack                                  36 tris      4.1 ms
1 props -> out in 0.21 s (3 textures made)
```

### fmt

Every shape line can be written in words or in shorthand, and `fmt` turns a file from one into the
other, keeping its comments and layout:

```text
b 0,0,~ .4,.3,.2 wood r=0,0,45 *5@.3,0,0 mx
box at=0,0,on size=.4,.3,.2 mat=wood turn=0,0,45 repeat 5 every .3,0,0 mirror x
```

The readable form is the one these docs use. `fmt` also wraps lines longer than 120 characters,
carrying them on with a `\` at the end of the line. See [basics](../language/basics.md#two-forms).

## The preview

The repository's `site/` folder is a small static site for looking at models:

```sh
uv run python site/build.py                  # builds every example into site/models/
python3 -m http.server -d site 8765          # then open http://localhost:8765 (index.html is the home page)
```

- **viewer.html** lists every model. Click one to orbit it. Hover a source line to light up the faces
  it made; hover an entry under *Built from* to light up everything that came through that part.
  Toggles: the PSX look (unlit, so only the baked shading shows, with snapped corners and fog), low resolution, wireframe,
  snap points.
- **sheet.html** is a contact sheet: two views of every model at once. Its address takes
  `?only=a,b`, `cols=`, `cell=`, `top=1`, `view=x,y,z`, `zoom=` and `psx=1`.
- **play.html** is the playground.

## The playground

The [playground](../../play.html) runs PartScript in the browser (Python in WebAssembly, through
Pyodide). Pick an example file or your own scratch file, type, and the model rebuilds half a second
after you stop. Mistakes show under the editor and in the margin; click one to go to its line. Your
edits stay in the browser. Every example in these docs opens there with **try it**.
