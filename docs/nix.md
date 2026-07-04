# Nix

`edgepad` provides a project flake so Nix/NixOS users can build and run it without relying on binaries built on another distribution.

The flake uses `rust-overlay` for an explicit Rust toolchain, reads the package version from `Cargo.toml`, and keeps the package definition in `nix/package.nix`.

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
nix run .#edgepad -- devices --all
```

For read-only capture from `/dev/input/event*`, permissions may require `sudo`, membership in the `input` group, or active seat/logind ACLs:

```bash
sudo ./result/bin/edgepad devices
sudo ./result/bin/edgepad dump --device /dev/input/eventX --out bug.ev --frames 300
./result/bin/edgepad replay bug.ev
```

For raw passthrough/output inspection:

```bash
sudo ./result/bin/edgepad dump --raw --device /dev/input/eventX --out bug.raw.ev --frames 300
./result/bin/edgepad replay-raw bug.raw.ev
```

For bounded live dry-run inspection without forwarding input:

```bash
sudo ./result/bin/edgepad proxy --device /dev/input/eventX --frames 300 --dry-run
```

Tune the edge zone for hardware validation when needed:

```bash
sudo ./result/bin/edgepad proxy --device /dev/input/eventX --frames 300 --edge-width 0.20 --dry-run
```

For bounded live passthrough through a virtual touchpad:

```bash
sudo ./result/bin/edgepad proxy --device /dev/input/eventX --frames 300 --uinput --grab
```

For long-running live proxy mode with auto-detection:

```bash
sudo ./result/bin/edgepad daemon --device auto
```

Daemon config files use TOML:

```toml
device = "auto"
edge_width = 0.10

[[gestures]]
zone = "left"
direction = "right"
action = ["notify-send", "edgepad", "left-right"]
```

## Development shell

```bash
nix develop
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -- replay tests/fixtures/left-edge-swipe-right.ev
```

The dev shell includes:

- stable Rust from `rust-overlay`;
- `rust-src` and `rust-analyzer`;
- `clippy` and `rustfmt`;
- `evtest` and `libinput` for manual input-device debugging.

To load the dev environment through direnv:

```bash
direnv allow
```

Or run one command without entering a shell:

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

The flake currently packages the CLI and provides a dev shell. A NixOS/Home Manager service module belongs later, after gesture action dispatch is ready.
