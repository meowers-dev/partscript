# Reference fixtures

What the Python implementation of PartScript (the last Python commit on `main`, with the broken-box
lattice limit) built, which the Rust tests compare against: every example and docs model
(`examples.json.gz`, `docs.json.gz`), the golden shapes (`golden.json.gz`), textures, the formatter,
Python's random streams and number formatting (`basics.json.gz`) and the order CPython 3.13 iterates
sets in (`sets.json.gz`).

The Rust rewrite was also checked against that Python build by differential fuzzing: tens of thousands
of mutated `.parts` files and random expressions run through both, comparing CLI output, error
messages, exit codes and every `.glb` byte (images as pixels). Ordinary files match exactly. These are
the known differences, all on input no real file has:

- **Complex numbers in a model.** `(-1)**.5` is complex in Python. Expressions work them out exactly as
  Python did, and `when=` and use arguments accept them, but a complex size or position is refused
  where it is read ("is a complex number, not a size or a position"); Python carried it further and
  failed later with a TypeError worded by wherever it landed, and its `check` sometimes let it pass.
- **Counts past 64 bits in `check`.** `sides=1e300` makes Python's triangle estimate a 300-digit integer;
  Rust's estimate saturates at 2**63-1, so the numbers in those warnings differ.
- **Crashes.** Where Python raised an exception nothing caught (a ZeroDivisionError, an OverflowError),
  Rust fails too, with an error rather than a traceback: mostly by stopping the build at the same prop
  with the exception's name and message, in a few cases (a broken box too big to cut, a colour with no
  such finish) as an error on the line or for every prop.
- **NaN bits.** A NaN coordinate is written as NaN, but its sign and payload bits may differ.
- **Limits.** Counts past 100,000 (copies, sides, rings, cells, a path's pieces, a broken box's chunks)
  are an error on the line: Python tried, and ran for hours or out of memory. Expressions nested about
  950 deep, where Python ran out of recursion, fail near (not exactly at) the same depth. Where Python
  ran out of memory below those limits, Rust may build.
- **`\N{name}` escapes** in a string literal inside an expression are a syntax error (Rust has no
  Unicode name table; such an expression was an error in Python too, worded differently).
