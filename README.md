# edgepad

Touchpad edge gestures for Linux/Wayland. Use the edges to control volume, adjust
brightness, switch workspaces, or run any command. The center stays available for
normal pointer movement.

Gestures start with a finger on the touchpad's left, right, top, or bottom edge.
Swipes, taps, and double taps run an action when you lift your finger; sliders
repeat an action as you move along an edge.

## Install

On an **x86_64 Linux desktop with systemd**, run:

```bash
curl -fsSL https://raw.githubusercontent.com/assembledev/edgepad/main/install.sh | sh
```

The installer puts `edgepad` in `~/.local/bin`, creates a config, sets up device
access, and starts a user service. It asks for `sudo` to install the udev rules.
Make sure `~/.local/bin` is on your `PATH`.

To preview the changes, append `-s -- --dry-run` to `sh`. Run the installer again
to update; it keeps your config.

**NixOS:** follow the [Nix setup](docs/nix.md). Use both the NixOS module for
device access and the Home Manager module for configuration and the user service.

## Try it

Check that edgepad is ready:

```bash
edgepad status
```

With the installer’s default config, slide a finger up or down along the left
edge. You should see a `volume-up` or `volume-down` notification. The examples
require `notify-send` and a desktop notification service; they don't change your
volume or other settings yet.

If nothing happens, run `edgepad doctor` to check device access, missing commands,
and service health.

## Make it yours

Edit `~/.config/edgepad/edgepad.toml`. For example, this complete config makes the
left edge a volume slider and a tap on the top edge toggle media playback:

```toml
device = "auto"

[[sliders]]
zone = "left"
up = ["pamixer", "-i", "3"]
down = ["pamixer", "-d", "3"]

[[gestures]]
zone = "top"
direction = "tap"
action = ["playerctl", "play-pause"]
```

Install `pamixer` and `playerctl` for this example, or replace them with commands
you use. Each array contains the command followed by its arguments. Edgepad does
not run commands through a shell.

Reload after editing:

```bash
systemctl --user reload edgepad.service
```

If the config is invalid, edgepad keeps the previous one and logs the error.
Home Manager users should edit their Nix configuration and apply it instead.

See the [configuration guide](docs/configuration.md) for swipes, double taps,
edge widths, and sensitivity settings, or start from the
[full example config](examples/edgepad.toml.example).

## Troubleshooting

If pointer movement behaves incorrectly, stop edgepad to release the touchpad:

```bash
systemctl --user stop edgepad.service
```

For other problems, check the diagnostics and logs:

```bash
edgepad doctor
journalctl --user -u edgepad.service -b
```

If more than one touchpad is found, run `edgepad devices`, set `device` to the
chosen `/dev/input/eventX` path in your config, and restart the service. Device
changes require a restart rather than a reload.

Start it again with `systemctl --user start edgepad.service`.
[Report a bug](https://github.com/assembledev/edgepad/issues) with the relevant
logs; the [capture guide](docs/dump-capture.md) explains how to record a gesture
for debugging.

## Uninstall

For installations made with the release installer:

```bash
curl -fsSL https://raw.githubusercontent.com/assembledev/edgepad/main/install.sh | sh -s -- --uninstall
```

Your config is kept. Add `--purge` to remove it too.

## Further reading

- [Configuration](docs/configuration.md) — gesture bindings and tuning.
- [Nix](docs/nix.md) — installation, modules, and development shell.
- [Device discovery](docs/device-discovery.md) and [event capture](docs/dump-capture.md).
- [Input forwarding](docs/passthrough-uinput.md), [double-tap recognition](docs/double-tap.md),
  and [replay format](docs/replay-format.md) — implementation and testing.

## Development

From a checkout, run `nix develop` to get the Rust toolchain, then:

```bash
cargo build --locked
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

## License

[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
