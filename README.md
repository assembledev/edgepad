# edgepad

Touchpad edge gestures for Linux/Wayland.

`edgepad` turns touchpad edges into command zones while keeping normal pointer movement on the center of the pad. Swipe from an edge to run commands such as changing workspaces, opening a launcher, or sending desktop notifications.

The hard part is input correctness: Type-B multitouch slots, mixed edge/center contacts, `SYN_DROPPED` recovery, and virtual touch cleanup are covered by replay tests before they touch real hardware.

## Features

- Edge gestures on the left, right, top, and bottom touchpad zones.
- Normal touchpad passthrough for unclaimed center contacts.
- Long-running user-session daemon with TOML config.
- Command actions as argv arrays, without shell re-splitting.
- Automatic touchpad discovery when exactly one readable candidate is present.
- Read-only device discovery and capture tools for debugging.
- Bounded live proxy mode for testing real hardware.
- Nix package, NixOS module, Home Manager module, release installer, and systemd user service.

## Install

### Release installer

```bash
curl -fsSL https://raw.githubusercontent.com/assembledev/edgepad/main/install.sh | sh
```

Preview the install plan first:

```bash
curl -fsSL https://raw.githubusercontent.com/assembledev/edgepad/main/install.sh | sh -s -- --dry-run
```

The installer downloads the x86_64 Linux release binary, installs udev rules, writes a default config to `~/.config/edgepad/edgepad.toml`, installs a systemd user service, starts it, and runs `edgepad doctor`.

Uninstall files created by the release installer:

```bash
curl -fsSL https://raw.githubusercontent.com/assembledev/edgepad/main/install.sh | sh -s -- --uninstall
```

This keeps `~/.config/edgepad/edgepad.toml`. To remove the config too:

```bash
curl -fsSL https://raw.githubusercontent.com/assembledev/edgepad/main/install.sh | sh -s -- --uninstall --purge
```

### Nix

Build and run from the repository:

```bash
nix build .#edgepad
./result/bin/edgepad --help
```

Run without installing:

```bash
nix run github:assembledev/edgepad -- --help
nix run github:assembledev/edgepad -- devices
```

Development shell:

```bash
nix develop
cargo test
```

See [Nix](docs/nix.md) for flake outputs, NixOS, and Home Manager setup.

### Cargo

```bash
cargo install --git https://github.com/assembledev/edgepad
```

For local development:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -- --help
```

## Quick start

Check the current daemon, config, device, zones, and actions:

```bash
edgepad status
```

Run deeper diagnostics for device access, action executables, and service state:

```bash
edgepad doctor
```

List touchpad candidates:

```bash
edgepad devices
```

Edit your config:

```bash
$EDITOR ~/.config/edgepad/edgepad.toml
```

Example config:

```toml
device = "auto"
edge_width = 0.10

[[sliders]]
zone = "left"
up = ["notify-send", "edgepad", "volume-up"]
down = ["notify-send", "edgepad", "volume-down"]

[[sliders]]
zone = "right"
up = ["notify-send", "edgepad", "brightness-up"]
down = ["notify-send", "edgepad", "brightness-down"]

[[gestures]]
zone = "top"
direction = "tap"
action = ["notify-send", "edgepad", "play-pause"]
```

Restart the user service after config changes:

```bash
systemctl --user restart edgepad.service
```

Watch logs:

```bash
journalctl --user -u edgepad.service -f
```

For a foreground run, `edgepad daemon` reads `~/.config/edgepad/edgepad.toml` by default.

## Gesture config

`device` can be `"auto"` or an explicit event node:

```toml
device = "auto"
# device = "/dev/input/event7"
```

`edge_width` is the fraction of the touchpad reserved for edge zones on each side:

```toml
edge_width = 0.10
```

Each gesture binding has a zone, direction, and action:

```toml
[[gestures]]
zone = "top"
direction = "right"
action = ["notify-send", "edgepad", "top-right"]
```

Zones:

```text
left, right, top, bottom
```

Directions:

```text
up, down, left, right, tap
```

Continuous controls use `[[sliders]]`. Side zones use vertical `up`/`down` steps; top and bottom zones use horizontal `left`/`right` steps. `step` is normalized touchpad travel and defaults to `0.04`.

```toml
[[sliders]]
zone = "left"
step = 0.04
up = ["pamixer", "-i", "3"]
down = ["pamixer", "-d", "3"]
```

Slider zones can share the same edge with `tap` gestures, but not with directional `[[gestures]]`.

Actions are argv arrays. They are not run through a shell, so write shell logic explicitly when needed:

```toml
action = ["sh", "-c", "date >> /tmp/edgepad-actions.log"]
```

For desktop commands, prefer running `edgepad` as a user service so actions inherit the user session instead of root's environment.

## How it works

`edgepad` reads the physical touchpad, claims contacts that begin in configured edge zones, and forwards unclaimed contacts through a virtual touchpad.

For live forwarding it:

1. opens the physical touchpad;
2. reads the device's multitouch capabilities;
3. creates a virtual touchpad through `/dev/uinput`;
4. grabs the physical device;
5. routes edge contacts to the gesture recognizer;
6. emits normal contacts through the virtual device;
7. releases virtual contacts and ungrabs the physical device on shutdown.

The output side does not blindly copy raw pointer-emulation events. `BTN_TOUCH`, `BTN_TOOL_*`, and legacy `ABS_X/Y` are synthesized from unclaimed passthrough contacts so an edge-owned finger does not leak into normal pointer movement.

## Permissions

`edgepad` needs access to:

- the physical touchpad event node under `/dev/input/event*`;
- `/dev/uinput` for virtual touchpad output.

The preferred desktop setup is a user service with logind/uaccess ACLs. The NixOS module and release installer install udev rules for that mode.

Manual commands that read real input devices may need `sudo`, the `input` group, or active seat ACLs. Use `edgepad doctor` to see what your system is missing.

## Diagnostics and capture

Device discovery is read-only:

```bash
edgepad devices
edgepad devices --all
```

Capture recognizer-level events from a real touchpad:

```bash
edgepad dump --device /dev/input/eventX --out bug.ev --frames 300
edgepad replay bug.ev
```

Capture raw evdev events for passthrough/output debugging:

```bash
edgepad dump --raw --device /dev/input/eventX --out bug.raw.ev --frames 300
edgepad replay-raw bug.raw.ev
```

Inspect live routing without forwarding input:

```bash
edgepad proxy --device /dev/input/eventX --frames 300 --dry-run
```

Run a bounded live virtual-touchpad proxy test:

```bash
edgepad proxy --device /dev/input/eventX --frames 300 --uinput --grab
```

Replace `/dev/input/eventX` with the touchpad node reported by `edgepad devices`. If the OS denies access to the event node or `/dev/uinput`, run `edgepad doctor` and use the access model it reports for your system.

## Commands

```text
edgepad devices     List readable input devices and touchpad candidates
edgepad status      Show a short daemon/config/device summary
edgepad doctor      Check config, runtime prerequisites, actions, and service health
edgepad daemon      Run the live edge-gesture proxy
edgepad dump        Capture touchpad events into a replay fixture
edgepad proxy       Run a bounded live proxy session for diagnostics
edgepad replay      Replay a parsed fixture through the recognizer
edgepad replay-raw  Replay a raw evdev capture through routing and output composition
```

## Documentation

- [Device discovery](docs/device-discovery.md)
- [Dump capture](docs/dump-capture.md)
- [Passthrough and uinput](docs/passthrough-uinput.md)
- [Replay fixture format](docs/replay-format.md)
- [Nix](docs/nix.md)

## Contributing

Feedback, issues, and pull requests are welcome.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
