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
- `edgepad proxy --device <event-node> --frames N --dry-run [--edge-width F]` for bounded live routing/output-composer inspection without forwarding input;
- `edgepad proxy --device <event-node> --frames N --uinput --grab [--edge-width F]` for bounded live virtual-device passthrough with explicit physical-device grab;
- `edgepad daemon [--config <file>] [--device auto|<event-node>] [--edge-width F]` for long-running live proxy mode with Ctrl+C/SIGTERM shutdown;
- TOML config parsing for `device = "auto"|"<event-node>"`, `edge_width = F`, and gesture command arrays;
- gesture action dispatch through a bounded worker queue that runs argv commands without a shell and waits for child processes;
- `.ev` metadata headers with real slot/X/Y ranges;
- raw output composition that synthesizes `BTN_TOUCH`, `BTN_TOOL_*`, and legacy `ABS_X/Y` from unclaimed passthrough contacts;
- tested raw output sink and buffered uinput sink plumbing;
- virtual touchpad capability spec for future uinput device creation;
- Nix flake for `nix build`, `nix run`, `nix develop`, NixOS system support, and a Home Manager user service.

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

### NixOS/Home Manager Service

The NixOS module prepares system access to `/dev/input` and `/dev/uinput`; the Home Manager module runs `edgepad` as a user service so gesture actions inherit the user session.

```nix
{
  inputs.edgepad.url = "github:assembledev/edgepad";

  outputs = { nixpkgs, edgepad, ... }: {
    nixosConfigurations.host = nixpkgs.lib.nixosSystem {
      modules = [
        edgepad.nixosModules.default
        {
          services.edgepad = {
            enable = true;
            users = [ "alice" ];
          };
        }
      ];
    };
  };
}
```

```nix
{
  imports = [ inputs.edgepad.homeManagerModules.default ];

  services.edgepad = {
    enable = true;
    device = "auto";
    edgeWidth = 0.10;
    gestures = [
      {
        zone = "right";
        direction = "down";
        action = [ "notify-send" "edgepad" "right-down" ];
      }
    ];
  };
}
```

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

For bounded live routing/output inspection without forwarding input:

```bash
sudo edgepad proxy --device /dev/input/eventX --frames 300 --dry-run
```

For bounded live passthrough through a virtual touchpad:

```bash
sudo edgepad proxy --device /dev/input/eventX --frames 300 --uinput --grab
```

This mode refuses to start if the physical touchpad is already touched, creates the virtual touchpad, grabs the physical device, processes the requested frame budget, drains briefly until the physical touchpad is idle when the budget ends mid-touch, emits a final synthetic release frame if the virtual touchpad still has an active passthrough contact, sends a neutral settle frame to the virtual touchpad, waits briefly, ungrabs, prints the same summary, and exits.

`proxy` uses a default edge width of `0.10` on each side. Use `--edge-width 0.15` or `--edge-width 0.20` when validating hardware/user gesture comfort on a real touchpad.

Replace `/dev/input/eventX` with the touchpad event node reported by `edgepad devices`.

For frame-limited edge gesture captures, a useful flow is: start capture, perform the edge or mixed gesture, release the gesture finger, then place a finger in the center until the frame budget finishes. This captures the gesture release while keeping the event stream active.

For long-running live proxy mode:

```bash
sudo edgepad daemon --device auto
```

`daemon` auto-detects a single readable touchpad candidate by default. If multiple touchpads are present, pass `--device /dev/input/eventX` explicitly. Stop it with Ctrl+C or SIGTERM; it drains briefly until the physical touchpad is idle before ungrabbing.

Minimal TOML config file:

```toml
device = "auto"
edge_width = 0.10

[[gestures]]
zone = "left"
direction = "up"
action = ["notify-send", "edgepad", "left-up"]

[[gestures]]
zone = "right"
direction = "down"
action = ["notify-send", "edgepad", "right-down"]

[[gestures]]
zone = "top"
direction = "right"
action = ["notify-send", "edgepad", "top-right"]

[[gestures]]
zone = "bottom"
direction = "left"
action = ["notify-send", "edgepad", "bottom-left"]
```

Load it with:

```bash
sudo edgepad daemon --config edgepad.conf
```

Command actions run from a bounded daemon worker queue. They are launched without a shell, and the worker waits for each child process so short-lived commands are reaped instead of accumulating zombies.

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
