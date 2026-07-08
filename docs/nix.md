# Nix

`edgepad` ships a Nix flake for local builds, development shells, NixOS device access, and a Home Manager user service.

The flake uses `rust-overlay` for an explicit Rust toolchain, reads the package version from `Cargo.toml`, and keeps the package definition in `nix/package.nix`.

## Build and run

```bash
nix build .#edgepad
./result/bin/edgepad --help
```

Run commands without installing:

```bash
nix run .#edgepad -- devices
nix run .#edgepad -- doctor
nix run .#edgepad -- replay tests/fixtures/left-edge-swipe-right.ev
```

For read-only capture from `/dev/input/event*`, permissions may require `sudo`, group access, or active seat ACLs:

```bash
sudo ./result/bin/edgepad devices
sudo ./result/bin/edgepad dump --device /dev/input/eventX --out bug.ev --frames 300
./result/bin/edgepad replay bug.ev
```

For live diagnostics:

```bash
sudo ./result/bin/edgepad proxy --device /dev/input/eventX --frames 300 --dry-run
sudo ./result/bin/edgepad proxy --device /dev/input/eventX --frames 300 --uinput --grab
```

For normal desktop gesture use, prefer the NixOS + Home Manager modules below. They run the daemon as a user service instead of a root shell command.

## NixOS module

The NixOS module prepares system access. It installs the package, loads `uinput`, and installs udev rules for the touchpad event node and `/dev/uinput`.

```nix
{
  imports = [ inputs.edgepad.nixosModules.default ];

  services.edgepad = {
    enable = true;
  };
}
```

The default access mode uses systemd-logind seat ACLs through `TAG+="uaccess"`. This is the preferred desktop mode because the active local user gets device access without permanent membership in the `input` group.

For systems without a normal local seat/logind session, use the group fallback:

```nix
{
  services.edgepad = {
    enable = true;
    accessMode = "group";
    users = [ "alice" ];
  };
}
```

After adding a user to the input group, start a new login session before expecting group membership to be visible.

## Home Manager module

The Home Manager module writes `~/.config/edgepad/edgepad.toml` and starts a systemd user service bound to `graphical-session.target`.

```nix
{
  imports = [ inputs.edgepad.homeManagerModules.default ];

  services.edgepad = {
    enable = true;
    device = "auto";
    edgeWidth = 0.10;

    gestures = [
      {
        zone = "top";
        direction = "tap";
        action = [ "notify-send" "edgepad" "play-pause" ];
      }
    ];

    sliders = [
      {
        zone = "right";
        up = [ "notify-send" "edgepad" "brightness-up" ];
        down = [ "notify-send" "edgepad" "brightness-down" ];
      }
    ];
  };
}
```

Gesture and slider actions are argv arrays and are not run through a shell. Use full paths or add packages to the user environment when you do not want to rely on `PATH`.

## Device selection

`device = "auto"` scans readable `/dev/input/event*` nodes and succeeds only when exactly one touchpad candidate is present. If there are multiple candidates, list them:

```bash
edgepad devices
```

and configure the explicit path:

```nix
services.edgepad.device = "/dev/input/event7";
```

## Development shell

```bash
nix develop
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Or run one command inside the shell:

```bash
nix develop -c cargo test
```

The dev shell includes stable Rust from `rust-overlay`, `rust-src`, `rust-analyzer`, `clippy`, `rustfmt`, `evtest`, and `libinput`.

For direnv:

```bash
direnv allow
```

## Flake outputs

```text
packages.<system>.edgepad
packages.<system>.default
apps.<system>.edgepad
apps.<system>.default
checks.<system>.edgepad
checks.<system>.module-tests
devShells.<system>.default
formatter.<system>
nixosModules.default
nixosModules.edgepad
homeManagerModules.default
homeManagerModules.edgepad
```

Supported systems:

```text
x86_64-linux
aarch64-linux
```
