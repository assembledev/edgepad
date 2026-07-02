{
  description = "Correctness-first Wayland touchpad edge gesture daemon";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      lib = nixpkgs.lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = lib.genAttrs systems;
      pkgsFor = system: import nixpkgs { inherit system; };
      sourceFor = pkgs:
        pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: _type:
            let
              rel = pkgs.lib.removePrefix ((toString ./.) + "/") (toString path);
            in
            !(rel == "target" || pkgs.lib.hasPrefix "target/" rel || rel == ".git" || pkgs.lib.hasPrefix ".git/" rel);
        };
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        rec {
          edgepad = pkgs.rustPlatform.buildRustPackage {
            pname = "edgepad";
            version = "0.1.0";

            src = sourceFor pkgs;
            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = [ pkgs.pkg-config ];

            doCheck = true;

            meta = {
              description = "Correctness-first Wayland touchpad edge gesture daemon";
              homepage = "https://github.com/assembledev/edgepad";
              mainProgram = "edgepad";
              platforms = pkgs.lib.platforms.linux;
            };
          };

          default = edgepad;
        }
      );

      apps = forAllSystems (
        system:
        let
          edgepadPackage = self.packages.${system}.edgepad;
        in
        rec {
          edgepad = {
            type = "app";
            program = "${edgepadPackage}/bin/edgepad";
          };

          default = edgepad;
        }
      );

      checks = forAllSystems (system: {
        edgepad = self.packages.${system}.edgepad;
      });

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              clippy
              pkg-config
              rust-analyzer
              rustc
              rustfmt
            ];

            RUST_BACKTRACE = "1";
          };
        }
      );

      formatter = forAllSystems (system: (pkgsFor system).nixfmt-rfc-style);
    };
}
