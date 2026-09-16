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

      toolchainFor = pkgs: pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

      version =
        (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
    in
    {
      packages = eachSystem (pkgs:
        let
          rust = toolchainFor pkgs;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rust;
            rustc = rust;
          };
        in
        rec {
          default = squill;

          squill = rustPlatform.buildRustPackage {
            pname = "squill";
            inherit version;
            src = self;
            cargoLock.lockFile = ./Cargo.lock;

            cargoBuildFlags = [ "-p" "cli" ];

            # The cli suite shells out to `git init` to check that
            # directory recursion honors .gitignore.
            nativeCheckInputs = [ pkgs.git ];

            meta = {
              description = "A SQL formatter for Postgres and SQLite";
              homepage = "https://github.com/aslilac/squill";
              license = pkgs.lib.licenses.mpl20;
              mainProgram = "squill";
            };
          };
        });

      devShells = eachSystem (pkgs:
        let
          rust = (toolchainFor pkgs).override {
            extensions = [ "rust-src" "rust-analyzer" ];
	          # wasm32-wasip1 is needed for the docs playground.
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
        }
      );
    };
}
