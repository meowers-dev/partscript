#!/bin/sh
# Builds everything partscript.dev serves into site/: the models of every example, the playground's
# WebAssembly and examples, the docs and the home page.
#
# Cloudflare Pages: build command `sh site/build.sh`, output directory `site`. Installs Rust (rustup) and
# the wasm32 target when they are missing.
set -e
cd "$(dirname "$0")/.."
if ! command -v cargo >/dev/null 2>&1; then
	if [ ! -x "$HOME/.cargo/bin/cargo" ]; then
		curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
	fi
	PATH="$HOME/.cargo/bin:$PATH"
fi
if command -v rustup >/dev/null 2>&1; then
	rustup target add wasm32-unknown-unknown >/dev/null
fi
cargo build --profile wasm --target wasm32-unknown-unknown -p partscript-wasm
mkdir -p site/pkg
cp target/wasm32-unknown-unknown/wasm/partscript_wasm.wasm site/pkg/partscript.wasm
cargo run --release -p site -- all
