# Open-source readiness review

Reviewed 2026-10-07, starting at `c8c1fc6` on `rust`, including the existing website changes and the
uncommitted fixes from this review. GitHub was private and its only published branch was `main`,
at `81609a5` (the Python implementation).

Following approval on 2026-10-07, the reviewed Rust and website changes were committed as `82d9249`
and fast-forwarded onto GitHub's default branch, `main`.
[GitHub CI passed](https://github.com/meowers-dev/partscript/actions/runs/37607632944), including
the workspace tests, generated references, examples, full site build and secret scan. Installing
with `cargo install --locked --git https://github.com/meowers-dev/partscript partscript-cli`
also succeeded; the installed CLI checked all 58 example props without errors or warnings.
The repository remains private. No manual deployment or visibility change was performed.

## Fixed

- Added [third-party notices](THIRD_PARTY_NOTICES.md) and upstream license texts for the CPython,
  MT19937, NumPy/SeedSequence/PCG64 and Unicode compatibility code/data, locked Rust dependencies
  and three.js. Identified the Home Video font's author and source alongside its existing CC0 terms.
  The site build now distributes these notices with its WebAssembly and links them from the home page.
- Added [contributor setup and checks](CONTRIBUTING.md), a [security policy](SECURITY.md), and native
  import/resource-boundary guidance in the embedding docs.
- Added local credential/cache ignore rules and weekly Cargo/GitHub Actions dependency updates.
- Pinned CI actions, limited the workflow token to read access, disabled persisted checkout
  credentials, used the lockfile, and added example/site builds plus a checksum-verified Gitleaks
  history scan. CI now runs on branch pushes as well as pull requests.
- Fixed `partscript --help` and `partscript -h` returning an error.
- Removed interpretation of model titles and part-tree labels as HTML in the model viewer.
- Corrected the first-prop guide's triangle/texture counts using the actual built output.

## Verification

| Check | Result |
|---|---|
| `cargo test --workspace --release --locked` | 159 tests passed, including Python output fixtures and Rust documentation examples |
| Generated reference check | Current |
| CLI check of `examples/` | 58 props, zero errors or warnings |
| Source-only CLI installation with `cargo install --locked --path crates/partscript-cli` | Passed without using the checkout's build artifacts |
| Installed CLI smoke tests | Top-level and subcommand help succeeded; documented first prop produced a valid 44-triangle GLB |
| `sh site/build.sh` | WebAssembly, 82 models, 13 playground examples, 28 documentation pages and license copies built |
| Generated site's local links and assets | No missing targets; copied license texts matched source files |
| Chromium browser checks | Home page rendered, install docs loaded, playground compiled a model, model viewer listed 82 models; no JavaScript errors |
| Model-viewer injection regression | HTML in a model title and part-tree name rendered literally; no injected elements or script execution |
| `actionlint` and shell syntax | Passed |
| `git diff --check` | Passed |
| Gitleaks 8.30.1 | No findings in reachable history or the candidate source tree, including decompressed reference fixtures |
| [OSV](https://osv.dev/) advisory lookup | No known advisories returned for the 11 locked registry crates or three.js 0.170.0 |

Local native verification used Linux x86_64 with Rust/Cargo 1.91.1; browser verification used Chromium.
Windows/macOS execution and a crates.io release were not tested. Dependency and secret scans are
point-in-time checks, not guarantees against all vulnerabilities.

## Before publication

- [x] Review and commit the local changes, including the pre-existing website/theme/font changes.
- [x] Land the Rust implementation and those changes on GitHub's default branch and verify passing
  CI there. The documented `cargo install --git ...` now installs the Rust CLI from `main`.
- [ ] Enable GitHub private vulnerability reporting when available. Its status could not be verified
  for the private repository (the API returned 404); the security policy includes a fallback.
- [ ] Change repository visibility and deploy only after the release commit is approved.

## History and scope

All 55 reachable commits were included in the history inventory; Gitleaks processed 54 commits with
scannable changes. No credential filenames or private network addresses were found. Commit authors
use a GitHub noreply email address. The compressed reference fixtures contained no local absolute
paths.

Old Python bytecode caches remain in historical commits, despite being deleted from the current
tree. They contain the checkout path `/mnt/data/dev2/partscript`, not credentials. Removing them is
an optional history-cleanup decision; no history was rewritten. Do not publish backup branches
incidentally with a blanket `git push --all`.

Existing `AGENTS.md`, theme, page-layout and documentation-generator edits were preserved. The
language's geometry and fixtures were not changed. Untrusted input can still consume large amounts
of CPU or memory; the security policy describes the required host/process boundaries rather than
claiming that individual count limits provide a sandbox.
