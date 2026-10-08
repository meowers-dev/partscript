# Moving parts

A prop can say where its parts turn and slide from, which part each hangs from, and name the points a
game hangs things on. Nothing here changes what the model looks like: it changes how the `.glb` is laid
out, so an engine (or an animation tool such as PartMotion) can move the parts. A prop that uses none of
it is written exactly as before.

## Pivots and parents

```text
part NAME [pivot=X,Y,Z] [parent=PART] [smooth=1]
```

- `pivot=` is where the part's node sits, in prop space: the point it turns about. Its shapes are written
  relative to it, so turning the node turns the part about the pivot. Without one, the node sits at the
  prop's origin.
- `parent=` names the part it hangs from: moving the parent moves it too. Without one, it hangs from the
  prop's root.

```parts
prop moving_demo "Bolt Rifle (moving parts)" weapons
  part receiver
  box at=0,0,.03 size=.045,.32,.06 mat=steel_dark
  part bolt parent=receiver pivot=0,.02,.045        # the bore runs through the pivot
  cylinder at=0,.02,.045 radius=.011 height=.16 mat=steel_grey axis=y sides=8
  part bolt_handle parent=bolt pivot=0,.09,.045
  cylinder at=.035,.09,.045 radius=.005 height=.05 mat=steel_grey axis=x sides=6
  sphere at=.06,.09,.045 radius=.011 mat=steel_grey sides=6 rings=4
  mark muzzle at=0,-.62,.05
  mark eject at=.03,0,.06 on=receiver
```

A part with a pivot but no shapes of its own is kept as an empty node, for other parts to hang from (a
hinge, a hip).

## Marks

```text
mark NAME at=X,Y,Z [on=PART] [turn=X,Y,Z]
```

A mark is a named point and turn that draws nothing: an empty node in the `.glb`, which Godot imports as a
`Node3D`. Games use them for a muzzle, a hand grip, an aim anchor, where a casing leaves. `on=` makes it
ride on a part (it moves with it); without one it hangs from the root. Names may use capitals, to match
what a game looks for (`Muzzle`, `RightHandGrip`).

## What check looks for

- `parent=` and `on=` name a part of the same prop;
- parents do not go round in a circle (`a` hangs from `b`, which hangs from `a`);
- once a prop has pivots, parents or marks, no two of its parts and marks share a name.

A pivot well outside its part's shapes is a build warning: most often a sign or a decimal point is wrong.

## In the .glb

The prop is a root node named after its asset id, with each part's node at its pivot under its parent's,
and each mark under the part it is on. See [what the .glb holds](../engine/output.md#nodes).
