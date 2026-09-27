#!/usr/bin/env bash
# Rebuild the playground's wasm modules and drop them into public/wasm/:
# sql.wasm for plain SQL, and one per built-in grammar, so a page loads
# only the grammar it formats.
# Needs: rustup target add wasm32-wasip1, and wasi-sdk (for the
# tree-sitter C code) at $WASI_SDK (default ~/.local/wasi-sdk):
#   https://github.com/WebAssembly/wasi-sdk/releases
# The nix dev shell supplies its own C toolchain via CC_/AR_wasm32_wasip1.
set -euo pipefail
cd "$(dirname "$0")/.."
WASI_SDK="${WASI_SDK:-$HOME/.local/wasi-sdk}"
# Defaults only: a shell that already points these at its own toolchain
# (the nix dev shell does) keeps them.
: "${CC_wasm32_wasip1=$WASI_SDK/bin/clang}"
: "${AR_wasm32_wasip1=$WASI_SDK/bin/llvm-ar}"
: "${CFLAGS_wasm32_wasip1=--sysroot=$WASI_SDK/share/wasi-sysroot}"
export CC_wasm32_wasip1 AR_wasm32_wasip1 CFLAGS_wasm32_wasip1
out=docs/public/wasm
mkdir -p "$out"
# The playground crate's features; `sql` is none of them.
for module in sql rust go python javascript typescript gleam \
	cpp csharp java kotlin swift; do
	features=()
	[ "$module" = sql ] || features=(--features "$module")
	cargo build --profile wasm-release --target wasm32-wasip1 -p playground \
		--no-default-features "${features[@]}"
	cp target/wasm32-wasip1/wasm-release/playground.wasm "$out/$module.wasm"
done
ls -la "$out"
