# Third-party notices

PartScript's original code is under the [MIT license](LICENSE). The following components retain
their upstream terms. Include this file and `licenses/` when distributing native binaries or
WebAssembly built from this repository. `site/build.sh` includes them in the website output.

## Compatibility code and data

PartScript began in Python. These Rust implementations preserve its random streams, expression
semantics and texture output. The upstream implementations were adapted to Rust and to PartScript's
data structures and error handling; Python and NumPy themselves are not runtime dependencies.

| Component | PartScript source | Upstream and retained notice |
|---|---|---|
| CPython behaviour, including random seeding, float arithmetic/formatting and set iteration | `crates/kitlib/src/py.rs`, `crates/partscript/src/pyexpr.rs`, `crates/partscript/src/expr.rs` | [CPython 3.13](https://github.com/python/cpython/tree/3.13), [Python license](licenses/CPython.txt) |
| MT19937 Mersenne Twister | `crates/kitlib/src/py.rs` | [CPython's random module](https://github.com/python/cpython/blob/v3.13.13/Modules/_randommodule.c), [Matsumoto and Nishimura notice](licenses/MT19937.txt) |
| NumPy-compatible random generation | `crates/partscript/src/npy.rs` | [NumPy](https://github.com/numpy/numpy/tree/v2.2.0/numpy/random), [NumPy license](licenses/NumPy.txt) |
| SeedSequence | `crates/partscript/src/npy.rs` | [NumPy's SeedSequence](https://github.com/numpy/numpy/blob/v2.2.0/numpy/random/bit_generator.pyx), [MIT notice](licenses/SeedSequence.txt) |
| PCG64 | `crates/partscript/src/npy.rs` | [NumPy's PCG64](https://github.com/numpy/numpy/tree/v2.2.0/numpy/random/src/pcg64), [MIT notice](licenses/PCG64.txt) |
| Unicode 15.1 data, obtained through CPython 3.13's `unicodedata` and character operations | `crates/kitlib/src/unicode.rs` | [Unicode Character Database](https://www.unicode.org/versions/Unicode15.1.0/), [Unicode license](licenses/Unicode.txt) and [Python license](licenses/CPython.txt) |

The reference fixtures contain outputs from the original Python implementation and are retained
for compatibility testing.

## Rust dependencies

`Cargo.lock` records the exact dependency versions. The retained MIT notices for the current lockfile
are in [Rust-dependencies.txt](licenses/Rust-dependencies.txt): adler2, aho-corasick, bitflags, memchr,
miniz_oxide, pulldown-cmark, pulldown-cmark-escape, regex, regex-automata, regex-syntax and unicase.
For crates offering several licenses, this notice bundle uses their MIT option. Update the notices
when changing dependencies. Some of these crates are used only by the website build tool.

## Website assets

- [three.js r170](https://github.com/mrdoob/three.js/tree/r170), loaded from jsDelivr at version
  `0.170.0`: [MIT notice](licenses/three.js.txt).
- [Home Video by GGBotNet](https://ggbot.itch.io/home-video-font), the website's pixel font:
  CC0 1.0 Universal. The full notice accompanies the font in `site/fonts/LICENSE.txt` and is copied to
  `licenses/home-video/LICENSE.txt` in a built website.

The example models, procedural textures and gallery images are project content covered by the
root license. The website theme uses the project's ZombieBox styling; the font has the separate
terms above.
