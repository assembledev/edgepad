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
