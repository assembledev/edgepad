{
  description = "Correctness-first Linux touchpad edge gesture daemon";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems f;

      cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      version = cargoToml.package.version;

      pkgsFor =
        system:
        import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

      rustPlatformFor =
        pkgs:
        pkgs.makeRustPlatform {
          cargo = pkgs.rust-bin.stable.latest.minimal;
          rustc = pkgs.rust-bin.stable.latest.minimal;
        };
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          edgepad = pkgs.callPackage ./nix/package.nix {
            inherit version;
            rustPlatform = rustPlatformFor pkgs;
          };
        in
        {
          inherit edgepad;
          default = edgepad;
        }
      );

      apps = forAllSystems (
        system:
        let
          edgepad = self.packages.${system}.edgepad;
          edgepadApp = {
            type = "app";
            program = "${edgepad}/bin/edgepad";
            meta.description = "Correctness-first Linux touchpad edge gesture daemon";
          };
        in
        {
          edgepad = edgepadApp;
          default = edgepadApp;
        }
      );

      checks = forAllSystems (system: {
        edgepad = self.packages.${system}.edgepad;
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          rust = pkgs.rust-bin.stable.latest.default.override {
            extensions = [
              "rust-src"
              "rust-analyzer"
              "clippy"
              "rustfmt"
            ];
          };
        in
        {
          default = pkgs.mkShell {
            packages = [
              rust
              pkgs.pkg-config
              pkgs.evtest
              pkgs.libinput
            ];

            RUST_BACKTRACE = "1";
            RUST_SRC_PATH = "${rust}/lib/rustlib/src/rust/library";
          };
        }
      );

      formatter = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        pkgs.writeShellApplication {
          name = "edgepad-nixfmt";
          runtimeInputs = [ pkgs.nixfmt ];
          text = ''
            if [ "$#" -eq 0 ]; then
              set -- flake.nix nix/package.nix
            fi
            exec nixfmt "$@"
          '';
        }
      );
    };
}
