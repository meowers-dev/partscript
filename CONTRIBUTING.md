# Contributing to PartScript

Bug reports, examples, documentation fixes and focused pull requests are welcome. For a bug, include
the smallest `.parts` file that reproduces it, the command you ran, the expected and actual result,
your OS, and the PartScript commit. Discuss substantial language or API changes in an issue first.
For security issues, follow [SECURITY.md](SECURITY.md).

## Working from source

Install stable Rust with [rustup](https://rustup.rs), then clone the repository. The CLI and tests
need only the Rust toolchain; the website also needs the WebAssembly target.

```sh
cargo run --release --locked -p partscript-cli -- check examples/
cargo test --workspace --release --locked
cargo run --release --locked -p site -- docs --check
rustup target add wasm32-unknown-unknown
sh site/build.sh
python3 -m http.server --bind 127.0.0.1 -d site 8765
```

Open <http://127.0.0.1:8765>. `site/build.sh` builds models, docs and WebAssembly locally. Python is
only used here to serve the site. Regenerating gallery images with `cargo run -p site -- shots`
additionally needs Chromium and ImageMagick; see `crates/site/src/shots.rs`.

To install the CLI from your checkout:

```sh
cargo install --locked --path crates/partscript-cli
```

## Changes and checks

- Use tabs in Rust and JavaScript, and follow the surrounding style. Avoid unrelated reformatting.
- Keep model output deterministic. Changing geometry, textures or `.glb` bytes for an existing file
  is a compatibility change; explain it before changing that behaviour. Do not regenerate the Python
  reference fixtures to make a failing test pass.
- Update `docs/` for language and API changes. Regenerate the material and standard-part references
  with `cargo run --locked -p site -- docs` when their source changes.
- Run the relevant test during development and the commands above before submitting a pull request.
  CI checks all workspace tests, documentation, examples, the website build and Git history for secrets.
- Edit the home page in `site/docs-assets/home.html`; `site/index.html` is generated. Generated models,
  docs, WebAssembly and license copies belong in the site build, not in new source commits.
- Keep credentials, local caches and build output out of commits. Preserve third-party notices when
  changing dependencies or adapted code; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
- Bound time and memory when fuzzing or testing extreme inputs. Individual count limits do not bound
  a complete build's resource use.

Include what changed, why, and how you verified it in your pull request. Submit only material you have
the right to contribute under the project's [MIT license](LICENSE), retaining any applicable
third-party notices.
