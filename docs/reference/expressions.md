# Expressions

Anywhere a number goes, an expression can: `w/2`, `-h+.1`, `sin(30)*r`, `max(a,b)`. Inside a position
or size, each comma-separated number is its own expression, so commas in function calls are fine:
`at=max(a,b),0,on`. An expression with spaces in it goes in quotes.

## Arithmetic and logic

| | |
|---|---|
| `+ - * / ** % //` | add, subtract, multiply, divide, power, remainder, whole division |
| `== != < <= > >=` | compare (true is 1, false is 0); chains like `0 < x < 1` work |
| `and or not` | combine |
| `a if c else b` | a when c holds, else b |

## Functions

Angles are degrees, in and out.

| | |
|---|---|
| `sin(d)` `cos(d)` `tan(d)` | of an angle in degrees |
| `asin(v)` `acos(v)` `atan(v)` `atan2(y,x)` | an angle in degrees |
| `sqrt(v)` `abs(v)` `hypot(x,y)` | square root, size, distance |
| `min(a,b,...)` `max(a,b,...)` | least, greatest |
| `floor(v)` `ceil(v)` `round(v)` | to whole numbers |
| `rad(d)` `deg(r)` | degrees to radians and back |
| `pi` `tau` | 3.14159..., 6.28318... |

## Drawn numbers

| | |
|---|---|
| `rand()` `rand(hi)` `rand(lo,hi)` | a number drawn for this copy (0-1, 0-hi, lo-hi) |
| `pick(a,b,c)` | one of the list, for this copy (numbers, materials, words) |
| `odds(a,b,c)` | 0, 1 or 2 for this copy, as often as the weights say ([odds](../language/variety.md#odds-one-fate-of-several)) |
| `noise(x[,y[,z]])` | smooth noise over space, 0-1, the same for the whole prop |
| `rough(x[,y[,z]])` | layered noise, 0-1 |

Draws come from the prop's `seed=`, the chain of `use` lines and copy numbers that led to this line,
and where in the line the call is: the same file always draws the same.

## Names

| | |
|---|---|
| variables | set with `set`, a part's parameters, or a room's `fill=` (`w d h level door_s door_n door_w door_e`) |
| `i` | the copy's number on a line that makes copies (or the name given with `as`) |
| `here.x` `here.y` `here.z` | where this copy stands in the prop |
| `NAME.left` ... `NAME.h` | numbers off a [named shape](../language/placing.md#naming-a-line) |
| `x`, `y` | in a `terrain`'s `height=`: the grid corner's place |
| `length` | in a part drawn with `from=`/`to=`, or a `join ... with=` bridge: how long it is |
| `depth` | nothing special: a parameter name the recursive examples use |

## Text

Label text takes `{expression}` (a number, written whole when it is whole), `{variable}` (a variable
holding text) and `pick(a,b,c)` (one of the words).
