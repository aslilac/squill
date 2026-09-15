{
  description = "squill — a SQL formatter for Postgres and SQLite";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];

      eachSystem = f: nixpkgs.lib.genAttrs systems (system: f (import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      }));
    in
    {
      devShells = eachSystem (pkgs:
        let
          # The channel comes from rust-toolchain.toml, so nix and rustup
          # users share a compiler; wasm32-wasip1 is the docs playground.
          rust = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
            extensions = [ "rust-src" "rust-analyzer" ];
            targets = [ "wasm32-wasip1" ];
          };

          # Stands in for a hand-installed wasi-sdk: the tree-sitter grammars
          # are C, so the wasm build needs a libc and a clang to reach it.
          # Kept off `packages` on purpose — the cc-wrapper setup hook would
          # point the host build's CC at wasm too.
          wasiCC = pkgs.pkgsCross.wasi32.stdenv.cc;
        in
        {
          default = pkgs.mkShell {
            packages = [
              rust
              pkgs.nodejs_24
              pkgs.corepack
            ];

            env = {
              CC_wasm32_wasip1 = "${wasiCC}/bin/wasm32-unknown-wasip1-clang";
              AR_wasm32_wasip1 = "${wasiCC}/bin/wasm32-unknown-wasip1-ar";
              # The cc wrapper already knows its sysroot.
              CFLAGS_wasm32_wasip1 = "";
            };
          };
        });
    };
}
