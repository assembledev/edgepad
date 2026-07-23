# edgepad

Touchpad edge gestures for Linux/Wayland.

`edgepad` turns touchpad edges into command zones while keeping normal pointer movement on the center of the pad. Swipe from an edge to run commands such as changing workspaces, opening a launcher, or sending desktop notifications.

The hard part is input correctness: Type-B multitouch slots, mixed edge/center contacts, `SYN_DROPPED` recovery, and virtual touch cleanup are covered by replay tests before they touch real hardware.

## Features

- Edge gestures on the left, right, top, and bottom touchpad zones.
- Continuous edge sliders for stepwise controls such as volume and brightness.
- Normal touchpad passthrough for unclaimed center contacts.
- Long-running user-session daemon with TOML config.
- Command actions as argv arrays, without shell re-splitting.
- Automatic touchpad discovery when exactly one readable candidate is present.
- Read-only device discovery and capture tools for debugging.
- Bounded live proxy mode for testing real hardware.
- Nix package, NixOS module, Home Manager module, release installer, and systemd user service.

## Core concepts

The touchpad is split into four edge zones: `left`, `right`, `top`, and `bottom`.

- A **gesture** runs one action when the finger lifts. It can match either a directional swipe or a
  tap.
- A **tap** is a gesture whose contact never leaves the configured movement tolerance.
- A **slider** runs repeated steps while the finger moves, which is useful for volume or brightness.
- An **action** is the command that edgepad starts for a gesture or slider step.

A slider and a tap gesture can share the same edge. A slider and a directional gesture cannot,
because both would need to own the same movement.

## Install

### Release installer

```bash
curl -fsSL https://raw.githubusercontent.com/assembledev/edgepad/main/install.sh | sh
```

Preview the install plan first:

```bash
curl -fsSL https://raw.githubusercontent.com/assembledev/edgepad/main/install.sh | sh -s -- --dry-run
```

The release installer is the complete setup for an x86_64 Linux desktop with systemd. It downloads
a static binary, installs the udev rules, writes a default config to
`~/.config/edgepad/edgepad.toml`, installs and starts the user service, then runs `edgepad doctor`.
It asks for `sudo` only when installing the udev rules.

### Update

Run the same installer command again. It keeps your config, replaces the installed files, restarts
the daemon, and checks that the service is healthy.

### Uninstall

Uninstall files created by the release installer:

```bash
curl -fsSL https://raw.githubusercontent.com/assembledev/edgepad/main/install.sh | sh -s -- --uninstall
```

This keeps `~/.config/edgepad/edgepad.toml`. To remove the config too:

```bash
curl -fsSL https://raw.githubusercontent.com/assembledev/edgepad/main/install.sh | sh -s -- --uninstall --purge
```

### Nix

For normal desktop use, install both the NixOS module and the Home Manager module. The NixOS module
provides device access; the Home Manager module owns the config and user service. See the complete
[Nix setup](docs/nix.md).

The commands below only build or run the package. They do not install device rules or a user service.

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

## Quick start

The config installed by the release script uses desktop notifications as safe example actions.
Gestures will show what they matched, but they will not change your volume, brightness, media state,
or workspace until you replace those commands.

Check that the service, config, and touchpad are ready:

```bash
edgepad status
```

The service becomes active only after the virtual touchpad is created and the physical device is
grabbed. Status output includes the ready daemon's PID, version, and selected device when systemd
provides them.

If status reports a problem, run the full check:

```bash
edgepad doctor
```

Edit your config:

```bash
$EDITOR ~/.config/edgepad/edgepad.toml
```

Example config:

```toml
device = "auto"
edge_width = 0.10
tap_min_duration_ms = 80
swipe_min_distance = 0.02

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

If pointer input behaves incorrectly, stop edgepad immediately:

```bash
systemctl --user stop edgepad.service
```

The physical touchpad is ungrabbed when the daemon stops. Start it again with:

```bash
systemctl --user start edgepad.service
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

`tap_min_duration_ms` ignores very short edge taps. It defaults to `80`; set it to `0` to disable the guard.

```toml
tap_min_duration_ms = 80
```

`swipe_min_distance` is the minimum normalized touchpad travel that turns an edge contact into a
directional gesture. It defaults to `0.02`, or 2% of the corresponding touchpad axis. Smaller
movement remains a tap, so the same physical gesture behaves consistently across coordinate ranges.
Once a contact reaches this distance it no longer qualifies as a tap, even if it returns to its
starting point. A slider contact that emits any steps is also consumed by the slider and does not
emit an additional tap when released.

```toml
swipe_min_distance = 0.02
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

The output side does not blindly copy raw pointer-emulation events. `BTN_TOUCH`, `BTN_TOOL_*`, and legacy `ABS_X/Y` are synthesized from unclaimed passthrough contacts so an edge-owned finger does not leak into normal pointer movement. Physical touchpad buttons (`BTN_LEFT` and related pointer buttons) are passed through, and live mode preserves input properties such as `INPUT_PROP_BUTTONPAD` so libinput keeps clickpad behavior.

On a buttonpad/clickpad, a physical button press takes priority over edge recognition. Active edge-owned contacts are promoted to normal passthrough contacts before the button event, and remain passthrough until they lift, so physical clicks and click-drag work even inside configured edge zones. This cancels the pending edge gesture; slider steps already emitted are not rolled back. Tap-to-click does not produce a physical button event, so an edge tap keeps the normal edge-gesture behavior. Touchpads with separate buttons keep independent edge and button handling.

## Permissions

`edgepad` needs access to:

- the physical touchpad event node under `/dev/input/event*`;
- `/dev/uinput` for virtual touchpad output.

The preferred desktop setup is a user service with logind/uaccess ACLs. The NixOS module and release installer install udev rules for that mode.

Manual commands that read real input devices may need `sudo`, the `input` group, or active seat ACLs. Use `edgepad doctor` to see what your system is missing.

## Troubleshooting

Start with these two commands:

```bash
edgepad status
edgepad doctor
```

Common problems:

- **More than one touchpad was found:** run `edgepad devices`, then set an explicit
  `device = "/dev/input/eventX"` in the config.
- **The touchpad or `/dev/uinput` is not accessible:** use the access fix reported by
  `edgepad doctor`. If udev rules or group membership just changed, start a new login session.
- **The service stays in `activating`:** edgepad is still waiting for a readable touchpad or
  `/dev/uinput`. Check `edgepad doctor` and the service log.
- **An action command is missing:** install that program, use its full path, or replace the example
  action with a command available on your desktop.
- **Pointer input is wrong:** stop the service with `systemctl --user stop edgepad.service`, then
  inspect `journalctl --user -u edgepad.service -b`.

## Diagnostics and capture

`proxy`, `replay`, and `replay-raw` use the normal edgepad config. Pass `--config <file>` to use
another config, or `--built-in-defaults` to ignore it.

Device discovery is read-only:

```bash
edgepad devices
edgepad devices --all
```

Capture recognizer-level events from a real touchpad:

```bash
edgepad dump --device auto --out bug.ev --frames 300
edgepad replay bug.ev
```

Capture raw evdev events for passthrough/output debugging:

```bash
edgepad dump --raw --device auto --out bug.raw.ev --frames 300
edgepad replay-raw bug.raw.ev
```

Inspect live routing without forwarding input:

```bash
edgepad proxy --device /dev/input/eventX --frames 300 --dry-run
```

Run a bounded live virtual-touchpad proxy test:

This command grabs the physical touchpad for the duration of the test. Normal pointer input is sent
through edgepad's temporary virtual touchpad until the frame limit is reached.

```bash
edgepad proxy --device /dev/input/eventX --frames 300 --uinput --grab
```

If auto-detection finds multiple touchpads, use the event node reported by `edgepad devices`. If the
OS denies access to the event node or `/dev/uinput`, run `edgepad doctor` and use the access model it
reports for your system.

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

## Development

Clone the repository and enter its development shell:

```bash
git clone https://github.com/assembledev/edgepad.git
cd edgepad
nix develop
```

Build and check the project from there:

```bash
cargo build --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

## Contributing

Feedback, issues, and pull requests are welcome.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
