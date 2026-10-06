# The PSX look

PartScript's defaults lean toward how 3D games looked in the late 1990s: few triangles, small
textures with hard pixels, light painted into the corners of faces. This page is about keeping that
look on purpose, and making it read well rather than just look old.

## Triangles

- A prop: a few hundred triangles. The budget `check` warns at is 2,500 (6,000 with `hero=1`).
- Round things: 6 to 12 sides, spheres with 3 or 4 rings. A bottle needs 6 sides, not 32.
- Detail goes where the eye goes: a bevel on the edge you see, a plain box behind.
- Many small copies of a cheap part beat one expensive part: 40 books of 12 triangles each read as a
  shelf of books.

## Textures

- Every colour material is a small texture with a little grain for its finish, dithered and quantized
  to a limited palette, filtered nearest (hard pixels).
- `px=auto` on a prop sizes its texel density to it, about 80 texels across its longest side, like a
  model with its own small texture page. Without it, every prop shares one density.
- Prefer a few colours that sit together. Pick from a short list (`pick(a,b,c)`) rather than any colour.

## Shading

- Every face gets a baked tone in its vertex colours: darker toward the ground (fading over 1.4 m, or
  the prop's height with `ao=auto`), a little darker facing away from the sky, and a small random
  difference per face so flat things are not dead flat.
- `fade=` darkens what a line makes toward its base: grass, trunks, walls.

## Silhouettes

At low resolution, outline is everything. Chunky, exaggerated proportions read; fine detail turns to
noise. Make what matters big: the lamp's shade, the door's frame, the gravestone's lean.

## The PSX look in the preview

The preview's PSX look shows a model as the PlayStation would have: no real-time lights (only the
baked shading), corners snapped to a coarse grid of the screen (the wobble), the picture drawn small and
blown up, and fog. Look at your scene that way before you call it done.

## Hard rules worth keeping

- Sit decals and labels 2 mm proud of the surface they are on, so they never fight it.
- Glass is see-through; give a window glass a frame so the eye knows it is there.
- Lit materials (`lamp_*`, `/glow`) are for things that give light: bulbs, flames, screens. Anything
  else that should be bright should be a bright colour.
