#!/usr/bin/env bash
# Rebuild the playground wasm module and drop it into public/.
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
cargo build --profile wasm-release --target wasm32-wasip1 -p playground
cp target/wasm32-wasip1/wasm-release/playground.wasm docs/public/playground.wasm
ls -la docs/public/playground.wasm
