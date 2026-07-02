# edgepad

Correctness-first Wayland touchpad edge gesture daemon.

## Goal

`edgepad` turns physical touchpad edge zones into command surfaces while preserving normal touchpad behavior through a virtual uinput device.

This project exists because the hard part is not dispatching `hyprctl`; the hard part is not corrupting the input stream.

## Foundation rules

- Rust implementation.
- Type-B evdev multi-touch slot lifecycle first.
- Replay tests before real-device polish.
- No hardcoded slot count; derive slot range from device capabilities.
- Claimed edge touches must not leak partial down events into passthrough.
- `SYN_DROPPED` must enter explicit resync handling.
- NixOS/Home Manager support, but after the input core has tests.

## Current status

Implemented:

- pure Type-B multitouch core invariants;
- replay fixture parser/runner;
- regression fixtures for left-edge swipe, center passthrough, mixed claimed/passthrough slots, duplicate tracking IDs, and `SYN_DROPPED` reset;
- minimal `edgepad replay <fixture.ev>` CLI for inspecting fixture behavior;
- read-only `edgepad devices` discovery foundation using `evdev`;
- read-only `edgepad dump --device <event-node> --out <file.ev>` capture skeleton for real `.ev` bug reports;
- `.ev` capability metadata header parsed by replay and written by dump;
- bounded dump capture via `--frames N`.

Docs:

- [`docs/replay-format.md`](docs/replay-format.md)
- [`docs/device-discovery.md`](docs/device-discovery.md)
- [`docs/dump-capture.md`](docs/dump-capture.md)
