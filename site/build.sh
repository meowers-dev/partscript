#!/bin/sh
# Builds everything partscript.dev serves into site/: the models of every example, the playground's
# package and examples, the docs and the home page.
#
# Cloudflare Pages: build command `sh site/build.sh`, output directory `site` (Python 3.10 or newer).
set -e
cd "$(dirname "$0")/.."
if command -v uv >/dev/null 2>&1; then
	run="uv run --with markdown python"
else
	python3 -m pip install --quiet -e . markdown
	run="python3"
fi
$run site/build.py
$run site/docs.py
