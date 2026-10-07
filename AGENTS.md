# Agent instructions

PartScript is a small language for low-poly, PSX-style 3D models that compiles to `.glb`. It is all Rust
(it began in Python; the Python implementation is gone, see `tests/reference/README.md`).

## Layout

- `crates/partscript`: the language. `lang` (parser), `pyexpr` + `expr` (expressions: Python 3.13's
  grammar, evaluated as the Python original did), `check` (static check), `compiler` (statements to
  geometry), `building` (kits, snaps, buildings), `textures`, `host` (the embedding API), `project`, `fmt`.
- `crates/kitlib`: geometry, baking, the `.glb` writer, noise, paths, surfaces, broken boxes, and `py`
  (Python's random, float formatting, set order, min/max and math errors, kept so files build the same).
- `crates/partscript-cli`: the `partscript` command. `crates/partscript-wasm`: the playground's compiler.
- `crates/site`: builds partscript.dev (`models`, `docs [--check]`, `shots`). `site/`: static pages.
- `docs/`: the documentation (Markdown, built into the site). `examples/`, `library/`: example files.
- `tests/reference/`: fixtures from the Python original; the Rust tests compare against them.

## Commands

```sh
cargo test --workspace --release                   # every test (what CI runs, with the two below)
cargo run --release -p site -- docs --check        # generated reference pages are current
cargo build --profile wasm --target wasm32-unknown-unknown -p partscript-wasm
cargo run --release -p partscript-cli -- check examples/
sh site/build.sh && python3 -m http.server -d site 8765   # the whole site, previewed locally
cargo run --release -p partscript --example bench  # build speed
```

## Rules

- Tabs for indentation; match the surrounding code's naming, comments and idiom.
- Same file, same model: anything that changes geometry, draws or output bytes for an existing file is a
  breaking change. The reference tests catch it; do not regenerate the fixtures to make them pass.
- Behaviour on odd input (NaN, infinity, huge counts, bad syntax) follows the Python original, with the
  differences listed in `tests/reference/README.md`. Counts past 100,000 are refused (`MAX_COUNT`).
- A change to the language or the API updates `docs/` in the same change. The std-parts and material
  pages are generated from the code: run `cargo run -p site -- docs` after changing either.
- When fuzzing or stress-testing, cap memory and time per process (`prlimit --as=1536M`, a timeout): the
  machine is shared, and one unbounded run has taken it down.
- Do not push, publish, make the repo public or deploy without asking. Commits end with the
  Co-Authored-By line the harness gives.
