# edgepad

Correctness-first touchpad edge gestures for Linux/Wayland.

`edgepad` is a small input daemon in progress. The goal is to turn physical touchpad edge zones into command surfaces while preserving normal touchpad behavior through a virtual input device.

The project focuses on input correctness: no leaked finger-down events, no stuck contacts, no broken multitouch state, and explicit recovery after `SYN_DROPPED`.

## Status

Implemented:

- Type-B multitouch slot lifecycle model;
- replay fixture parser and runner;
- regression fixtures for edge claiming, normal passthrough, mixed slots, duplicate tracking IDs, and `SYN_DROPPED` recovery;
- `edgepad replay <fixture.ev>` for inspecting fixture/capture behavior;
- `edgepad devices` for read-only `/dev/input/event*` discovery;
- `edgepad dump --device <event-node> --out <file.ev> [--frames N]` for read-only capture;
- `.ev` metadata headers with real slot/X/Y ranges;
- Nix flake for `nix build`, `nix run`, and `nix develop`.

Not implemented yet:

- virtual-device passthrough via `uinput`;
- device grabbing;
- daemon/service mode;
- gesture/action configuration;
- NixOS/Home Manager service module.

## Quick start

### Nix

```bash
nix build .#edgepad
./result/bin/edgepad replay tests/fixtures/left-edge-swipe-right.ev
```

Run without installing:

```bash
nix run .#edgepad -- replay tests/fixtures/left-edge-swipe-right.ev
nix run .#edgepad -- devices
```

Development shell:

```bash
nix develop
cargo test
```

See [`docs/nix.md`](docs/nix.md).

### Cargo

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -- replay tests/fixtures/left-edge-swipe-right.ev
```

## Capturing a real touchpad sample

`dump` is read-only. It does not grab devices, suppress input, or create a virtual device.

```bash
edgepad devices              # touchpad candidates only
edgepad devices --all        # full /dev/input list for debugging
sudo edgepad dump --device /dev/input/eventX --out bug.ev --frames 60
edgepad replay bug.ev
```

Replace `/dev/input/eventX` with the touchpad event node reported by `edgepad devices`.

## Docs

- [`docs/replay-format.md`](docs/replay-format.md)
- [`docs/device-discovery.md`](docs/device-discovery.md)
- [`docs/dump-capture.md`](docs/dump-capture.md)
- [`docs/nix.md`](docs/nix.md)

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
