#!/usr/bin/env bash
# Rebuild the playground wasm module and drop it into public/.
# Needs: rustup target add wasm32-wasip1, and wasi-sdk (for the
# tree-sitter C code) at $WASI_SDK (default ~/.local/wasi-sdk):
#   https://github.com/WebAssembly/wasi-sdk/releases
set -euo pipefail
cd "$(dirname "$0")/../.."
WASI_SDK="${WASI_SDK:-$HOME/.local/wasi-sdk}"
export CC_wasm32_wasip1="$WASI_SDK/bin/clang"
export AR_wasm32_wasip1="$WASI_SDK/bin/llvm-ar"
export CFLAGS_wasm32_wasip1="--sysroot=$WASI_SDK/share/wasi-sysroot"
cargo build --profile wasm-release --target wasm32-wasip1 -p playground
cp target/wasm32-wasip1/wasm-release/playground.wasm docs/public/playground.wasm
ls -la docs/public/playground.wasm
