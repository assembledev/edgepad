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
- `edgepad dump --device <event-node> --out <file.ev> [--frames N]` for read-only replay-format capture;
- `edgepad dump --raw --device <event-node> --out <file.raw.ev> [--frames N]` for read-only raw evdev capture;
- `edgepad replay-raw <file.raw.ev>` for raw capture routing/output-composer inspection;
- `.ev` metadata headers with real slot/X/Y ranges;
- raw output composition that synthesizes `BTN_TOUCH`, `BTN_TOOL_*`, and legacy `ABS_X/Y` from unclaimed passthrough contacts;
- tested raw output sink and buffered uinput sink plumbing;
- virtual touchpad capability spec for future uinput device creation;
- Nix flake for `nix build`, `nix run`, and `nix develop`.

Not implemented yet:

- live virtual-device passthrough/proxy via `uinput`;
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

For recognizer-level debugging:

```bash
edgepad devices              # touchpad candidates only
edgepad devices --all        # full /dev/input list for debugging
sudo edgepad dump --device /dev/input/eventX --out bug.ev --frames 300
edgepad replay bug.ev
```

For raw passthrough/output debugging:

```bash
sudo edgepad dump --raw --device /dev/input/eventX --out bug.raw.ev --frames 300
edgepad replay-raw bug.raw.ev
```

Replace `/dev/input/eventX` with the touchpad event node reported by `edgepad devices`.

For frame-limited edge gesture captures, a useful flow is: start capture, perform the edge or mixed gesture, release the gesture finger, then place a finger in the center until the frame budget finishes. This captures the gesture release while keeping the event stream active.

## Docs

- [`docs/replay-format.md`](docs/replay-format.md)
- [`docs/device-discovery.md`](docs/device-discovery.md)
- [`docs/dump-capture.md`](docs/dump-capture.md)
- [`docs/passthrough-uinput.md`](docs/passthrough-uinput.md)
- [`docs/nix.md`](docs/nix.md)

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
