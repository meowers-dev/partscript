# Security

## Reporting a vulnerability

Use GitHub's private **Report a vulnerability** option on the repository's
[Security page](https://github.com/meowers-dev/partscript/security) when it is available.
If that option is unavailable, open an issue requesting a private contact channel, without exploit
details or sensitive files. Please keep vulnerability details out of public issues and pull requests.

Include the affected commit, OS, a minimal reproducer, the impact and any proposed fix. Remove
credentials and private assets from the reproducer. Reports should reproduce on the current code;
there are no maintained older release branches yet.

## Input and host boundaries

PartScript is a local modelling tool, not an isolation boundary for hostile programs.

- Native imports can read files accessible to the process, including paths outside the initial
  source directory. `Project::from_text` can also resolve imports from disk. For an embedded service,
  use `Project::new` with an explicit `Reader` that only supplies permitted sources.
- The 100,000 count limit applies to individual operations. Nested copies, variants and geometry can
  still use substantial CPU and memory. Run untrusted builds in a separate process with time and
  memory limits, restricted filesystem access and no credentials.
- `Embed` and `TextureProvider` implementations are trusted host code. Validate their inputs and
  keep output/cache paths under the application's control.
- The browser playground compiles in a WebAssembly worker and reads imports from its supplied
  library. It has no native filesystem access. Expensive input can still exhaust browser resources;
  the worker is not a total build-time or memory limit.

The playground stores edits in browser local storage. The site's viewer loads the pinned three.js
version from jsDelivr; model compilation itself runs locally in the browser.
