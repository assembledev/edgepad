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
        let
          rust = pkgs.buildPackages.rust-bin.stable.latest.minimal.override {
            targets = [ pkgs.stdenv.hostPlatform.rust.rustcTarget ];
          };
        in
        pkgs.makeRustPlatform {
          cargo = rust;
          rustc = rust;
        };

      nixosModule = import ./nix/nixos-module.nix self;
      homeManagerModule = import ./nix/home-manager-module.nix self;
    in
    {
      nixosModules = {
        edgepad = nixosModule;
        default = nixosModule;
      };

      homeManagerModules = {
        edgepad = homeManagerModule;
        default = homeManagerModule;
      };

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
        // pkgs.lib.optionalAttrs (system == "x86_64-linux") {
          release = pkgs.callPackage ./nix/release.nix {
            inherit version;
            edgepad = pkgs.pkgsStatic.callPackage ./nix/package.nix {
              inherit version;
              rustPlatform = rustPlatformFor pkgs.pkgsStatic;
            };
          };
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

      checks = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          edgepad = self.packages.${system}.edgepad;
          module-tests = import ./nix/module-tests.nix {
            inherit
              self
              pkgs
              ;
            inherit (pkgs) lib;
          };
        }
        // pkgs.lib.optionalAttrs (system == "x86_64-linux") {
          release = self.packages.${system}.release;
        }
      );

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
              set -- flake.nix nix/package.nix nix/nixos-module.nix nix/home-manager-module.nix nix/module-tests.nix nix/release.nix
            fi
            exec nixfmt "$@"
          '';
        }
      );
    };
}
