# Writing it so it reads well

A `.parts` file is read far more often than it is written: by you next month, by whoever borrows your
parts. These habits keep a file easy to look at. Take this desk, which works but is hard to read:

```text
prop writers_desk "Writer's Desk" layout hero=1
  top = bevel_box at=0,0,on(.72) size=1.4,.7,.04 bevel=.01 mat=#6a4a2a/wood
  box at=top.left+.05,top.front+.05,on size=.05,.05,.72 mat=#5a3a22/wood mirror xy
  drawers = box at=top.right-.27,0,on(.42) size=.44,.62,.3 mat=#5a3a22/wood
  stack at=drawers.x,drawers.front-.006,drawers.bottom+.02 gap=.02 {
    box size=.4,.012,.12 mat=#7a5a32/wood repeat 2
  }
  books = row x on=top at=-.42,.22 align=back {
    use book repeat 10
  }
  box on=top at=books.right+.03,.24 size=.025,.14,.18 mat=brass turn=0,-14,0
  use anglepoise on=top at=.5,.18
  papers = stack on=top at=-.1,-.12 {
    box size=.3,.21,.004 mat=#f0ece0/paint turn=0,0,rand(-14,14) repeat 7
  }
  box on=papers at=.06,0 size=.15,.012,.012 mat=#2a2a2a/plastic turn=0,0,35
  use cup on=top at=.22,-.18
```

and the same desk, as it is in `layout.parts`:

```text
set oak=#6a4a2a/wood dark_oak=#5a3a22/wood pale_oak=#7a5a32/wood

prop writers_desk "Writer's Desk" layout hero=1
  # The desk: a top on four legs, and a bank of two drawers under its right-hand end.
  top = bevel_box size=1.4,.7,.04 bevel=.01 at=0,0,on(.72) mat=oak
  legs4 w=1.3 d=.6 h=.72 m=dark_oak
  drawers = box size=.44,.62,.3 at=top.right-.27,0,on(.42) mat=dark_oak
  stack on=drawers.front pack=centre gap=.02 {
    box size=.4,.012,.12 mat=pale_oak repeat 2
  }

  # On it: books along the back, a lamp, a pile of papers with a pen, a cup.
  books = row x on=top at=-.42,.22 align=back {
    book repeat 10
  }
  bookend on=top at=books.right+.03,.24
  anglepoise on=top at=.5,.18
  papers = stack on=top at=-.1,-.12 {
    paper repeat 7
  }
  pen on=papers at=.06,0
  cup on=top at=.22,-.18
```

## Name your colours

`#6a4a2a/wood` says nothing about what it is for, and a colour written out six times is six places to
change. Name them once at the top of the file (`set oak=... dark_oak=...`) and say `mat=oak`.

## Give small things a name of their own

A line whose meaning is in its numbers (`box size=.15,.012,.012 mat=#2a2a2a/plastic turn=0,0,35`) is a
thing waiting for a name. Make it a part (`def pen`) and the prop says `pen on=papers`. Call parts like
shapes (`book repeat 10`, `cup on=top`); `use` adds nothing there.

## Place against names, not numbers

`on=top`, `on=drawers.front`, `row`, `stack` and `from=`/`to=` say where things go in words. Arithmetic
like `drawers.front-.006` means the line is working out something the language can do for it. Std
parts do the same: `legs4` puts four legs under a top.

## Say what, then where, then what of

Write each line in the same order: the shape, its size, where it goes, then its material and finish
(`box size=... at=... mat=...`). The eye learns where to look.

## Group, and say what each group is

A blank line between groups and a one-line comment over each (`# On it: books, a lamp...`) turn a list of
shapes into a description of a thing. The comment is for what the lines are, not how they work.

## Keep lines short

Long lines wrap in the editor and hide their ends. Break a long line with `\` and indent the rest;
`partscript fmt` does this at 120 characters.
