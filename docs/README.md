## Building the docs site

`nix develop` already provides the Wasi target and a C toolchain for the tree-sitter grammars, so `pnpm dev` and `pnpm build` work there with no further setup. The rest of this section is for building without nix.

Add the Wasi target with `rustup`:

```sh
rustup target add wasm32-wasip1
```

<details>
<summary>
Install wasi-sdk
</summary>

```sh
# macOS
curl -LO https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-33/wasi-sdk-33.0-arm64-macos.tar.gz
tar xf wasi-sdk-33.0-arm64-macos.tar.gz
mv wasi-sdk-33.0-arm64-macos ~/.local/wasi-sdk
```

```sh
# Linux (Intel/AMD)
curl -LO https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-33/wasi-sdk-33.0-x86_64-linux.tar.gz
tar xf wasi-sdk-33.0-x86_64-linux.tar.gz
mv wasi-sdk-33.0-x86_64-linux ~/.local/wasi-sdk
```

```sh
# Linux (ARM)
curl -LO https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-33/wasi-sdk-33.0-arm64-linux.tar.gz
tar xf wasi-sdk-33.0-arm64-linux.tar.gz
mv wasi-sdk-33.0-arm64-linux ~/.local/wasi-sdk
```

</details>

## Language icons

The language icons are taken from the delightful [Catppuccin Icons](https://github.com/catppuccin/vscode-icons/) (Mocha theme) under the [MIT license](https://github.com/catppuccin/vscode-icons/blob/b6915da9f6889b683a110aa747de96c2820a537d/LICENSE). Some minor modifications have been made to suit this particular context.
