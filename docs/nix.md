# Nix

`edgepad` provides a project flake so Nix/NixOS users can build and run it without relying on binaries built on another distribution.

## Build

```bash
nix build .#edgepad
./result/bin/edgepad replay tests/fixtures/left-edge-swipe-right.ev
```

`buildRustPackage` runs the Rust test suite during the check phase.

## Run

```bash
nix run .#edgepad -- replay tests/fixtures/left-edge-swipe-right.ev
nix run .#edgepad -- devices
```

For read-only capture from `/dev/input/event*`, permissions may require `sudo`, membership in the `input` group, or active seat/logind ACLs:

```bash
sudo ./result/bin/edgepad devices
sudo ./result/bin/edgepad dump --device /dev/input/eventX --out bug.ev --frames 60
./result/bin/edgepad replay bug.ev
```

## Development shell

```bash
nix develop
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -- replay tests/fixtures/left-edge-swipe-right.ev
```

Or run a single command inside the shell:

```bash
nix develop -c cargo test
```

## Outputs

The flake exposes:

- `packages.<system>.edgepad`
- `packages.<system>.default`
- `apps.<system>.edgepad`
- `apps.<system>.default`
- `checks.<system>.edgepad`
- `devShells.<system>.default`
- `formatter.<system>`

Supported systems in the flake:

- `x86_64-linux`
- `aarch64-linux`

## Scope

The flake currently packages the CLI and provides a dev shell. A NixOS/Home Manager service module belongs later, after the daemon and virtual-device passthrough exist.
