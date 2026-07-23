# Nix

`edgepad` provides two modules for a normal desktop installation:

- the NixOS module installs the package, loads `uinput`, and grants device access;
- the Home Manager module writes the config and runs edgepad as a user service.

Use both modules for the complete setup. Building the package alone does not configure device access
or start the daemon.

## Normal desktop installation

### 1. Add the flake input

Add edgepad to the inputs of your system flake:

```nix
{
  inputs.edgepad = {
    url = "github:assembledev/edgepad";
    inputs.nixpkgs.follows = "nixpkgs";
  };
}
```

The examples below use `inputs` in NixOS and Home Manager modules. If your flake does not already
pass it to modules, use:

- `specialArgs = { inherit inputs; };` in `nixosSystem`;
- `extraSpecialArgs = { inherit inputs; };` in a standalone `homeManagerConfiguration`; or
- `home-manager.extraSpecialArgs = { inherit inputs; };` when Home Manager is a NixOS module.

### 2. Enable system access

Import the NixOS module in your system configuration:

```nix
{ inputs, ... }:

{
  imports = [ inputs.edgepad.nixosModules.default ];

  services.edgepad.enable = true;
}
```

The default access mode uses systemd-logind seat ACLs through `TAG+="uaccess"`. The active local
user gets access to the touchpad and `/dev/uinput` without permanent membership in the `input`
group.

For a machine without a normal local seat or logind session, use the group fallback:

```nix
{
  services.edgepad = {
    enable = true;
    accessMode = "group";
    users = [ "alice" ];
  };
}
```

Start a new login session after adding a user to the group.

### 3. Configure the user service

Import the Home Manager module in your home configuration:

```nix
{ inputs, pkgs, ... }:

{
  imports = [ inputs.edgepad.homeManagerModules.default ];

  services.edgepad = {
    enable = true;
    device = "auto";
    edgeWidth = 0.10;
    tapMinDurationMs = 80;
    swipeMinDistance = 0.02;

    gestures = [
      {
        zone = "top";
        direction = "tap";
        action = [ "${pkgs.libnotify}/bin/notify-send" "edgepad" "play-pause" ];
      }
    ];

    sliders = [
      {
        zone = "right";
        up = [ "${pkgs.libnotify}/bin/notify-send" "edgepad" "brightness-up" ];
        down = [ "${pkgs.libnotify}/bin/notify-send" "edgepad" "brightness-down" ];
      }
    ];
  };
}
```

These notification actions are safe examples. Replace them with the commands used by your desktop.
Actions are argv arrays and are not run through a shell. Using package paths, as above, avoids
depending on the service's `PATH`.

### 4. Apply and verify

Apply the NixOS and Home Manager configurations using your normal rebuild commands. If Home Manager
is integrated into the NixOS configuration, the NixOS rebuild applies both modules.

Then check the user service:

```bash
edgepad status
edgepad doctor
systemctl --user status edgepad.service
```

The service is ready only after edgepad has created the virtual touchpad and grabbed the physical
device. If pointer input behaves incorrectly, stop it immediately:

```bash
systemctl --user stop edgepad.service
```

### Update

Update the locked edgepad input, then apply your system and Home Manager configurations again:

```bash
nix flake update edgepad
```

## Device selection

`device = "auto"` succeeds only when exactly one readable touchpad candidate is present. If edgepad
finds more than one candidate, list them:

```bash
edgepad devices
```

Then set the chosen event node in Home Manager:

```nix
services.edgepad.device = "/dev/input/event7";
```

## Package-only use

Build the package from a checkout:

```bash
nix build .#edgepad
./result/bin/edgepad --help
```

Run a command without installing:

```bash
nix run .#edgepad -- devices
nix run .#edgepad -- replay tests/fixtures/left-edge-swipe-right.ev --built-in-defaults
```

From outside the repository, use the GitHub flake:

```bash
nix run github:assembledev/edgepad -- --help
```

These commands provide the binary only. For the daemon, device rules, and user service, use the two
modules above.

## Development shell

```bash
nix develop
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

Or run one command inside the shell:

```bash
nix develop -c cargo test --locked
```

The shell includes stable Rust, `rust-src`, `rust-analyzer`, `clippy`, `rustfmt`, `evtest`, and
`libinput`.

The repository includes an `.envrc`, so direnv users can run:

```bash
direnv allow
```

## Manual diagnostics

Read-only device discovery and capture may require `sudo` when the current shell does not have a
seat ACL:

```bash
sudo ./result/bin/edgepad devices
sudo ./result/bin/edgepad dump --device auto --out bug.ev --frames 300
./result/bin/edgepad replay bug.ev
```

Dry-run proxy mode reads and routes events without grabbing the touchpad:

```bash
sudo ./result/bin/edgepad proxy \
  --config "$HOME/.config/edgepad/edgepad.toml" \
  --device /dev/input/eventX --frames 300 --dry-run
```

The live proxy below grabs the physical touchpad and sends normal pointer input through a temporary
virtual touchpad until the frame limit is reached:

```bash
sudo ./result/bin/edgepad proxy \
  --config "$HOME/.config/edgepad/edgepad.toml" \
  --device /dev/input/eventX --frames 300 --uinput --grab
```

For normal desktop use, run the Home Manager service instead of a root shell command. Gesture
actions started under `sudo` inherit root's environment, not the graphical user session.

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
