# Passthrough and uinput

`edgepad` uses a virtual touchpad to keep normal pointer movement working while edge contacts are used as gestures.

The live path reads a physical touchpad, claims contacts that start inside configured edge zones, and forwards unclaimed contacts through `/dev/uinput`. The compositor still sees one touchpad stream: edge gestures become actions, center touches remain pointer input.

## Modes

The bounded `proxy` uses the recognizer profile from the default user config. `--config <file>`
selects another config, while `--built-in-defaults` opts into the standalone profile and requires an
explicit `--device`. CLI `--device` and `--edge-width` values override the selected config. Proxy
reports gestures and slider steps but does not execute configured actions.

### Dry-run proxy

`proxy --dry-run` is read-only inspection. It reads live frames, routes them through the recognizer and output composer, prints counters, and exits after the requested frame budget.

```bash
edgepad proxy --device /dev/input/event5 --frames 300 --dry-run
```

It does not create a virtual device, emit uinput events, suppress physical input, or call `EVIOCGRAB`.

Tune the edge zone for hardware validation:

```bash
edgepad proxy --device /dev/input/event5 --frames 300 --edge-width 0.20 --dry-run
```

The summary includes raw event volume, recognizer volume, passthrough frames, claimed-edge frames, composed output, cleanup output, settle output, gestures, slider steps, and counts by zone/direction.

### Bounded grab/uinput proxy

`proxy --uinput --grab` is the manual live forwarding test:

```bash
edgepad proxy --device /dev/input/event5 --frames 300 --uinput --grab
```

It is intentionally bounded. The command:

1. opens the physical touchpad and reads its capabilities;
2. refuses to start if the physical touchpad is already touched;
3. creates the virtual touchpad through `/dev/uinput`;
4. grabs the physical device;
5. routes/composes the requested frame budget;
6. drains briefly if the frame budget ends mid-touch;
7. emits composed passthrough frames to the virtual touchpad;
8. emits a synthetic release frame if a virtual passthrough contact is still active;
9. emits one neutral settle frame;
10. waits briefly so the compositor sees the neutral state;
11. ungrabs and exits.

The idle drain is bounded and reported as `idle_drain_timed_out` when it expires.

### Daemon mode

`daemon` is the long-running live proxy:

```bash
edgepad daemon --config ~/.config/edgepad/edgepad.toml
```

It uses the same proxy runtime as the bounded mode and stops on Ctrl+C or SIGTERM. During shutdown it drains briefly until the physical touchpad is idle, emits cleanup output, and ungrabs the physical device.
The packaged systemd units use `Type=notify`: they become active only after `/dev/uinput` setup and
the physical-device grab both succeed. While startup retry is waiting for hardware or permissions,
systemd keeps the service in `activating` instead of reporting a false ready state.

For normal desktop use, run the daemon as a user service with access to `/dev/input` and `/dev/uinput`. That lets gesture actions inherit the user session. Running the daemon with `sudo` is useful for manual diagnostics, but command actions then run with root's environment.

## Config example

The commands below are safe notification examples. Replace them with the commands used by your
desktop.

```toml
device = "auto"
edge_width = 0.10
tap_min_duration_ms = 80
swipe_min_distance = 0.02

[[gestures]]
zone = "top"
direction = "tap"
action = ["notify-send", "edgepad", "play-pause"]

[[sliders]]
zone = "right"
up = ["notify-send", "edgepad", "brightness-up"]
down = ["notify-send", "edgepad", "brightness-down"]
```

`device = "auto"` succeeds only when exactly one readable touchpad candidate is present. If auto-detection is ambiguous, choose a device from:

```bash
edgepad devices
```

and set:

```toml
device = "/dev/input/event5"
```

## Output policy

Raw Linux input streams include both multitouch slot events and pointer-emulation events. `edgepad` does not blindly forward the global pointer-emulation events:

- `BTN_TOUCH`
- `BTN_TOOL_*`
- legacy `ABS_X`
- legacy `ABS_Y`

Those values can follow an edge-owned contact while another center contact is active. Instead, output state is synthesized from unclaimed passthrough slots:

- multitouch slot events are preserved only for passthrough contacts;
- `BTN_TOUCH` follows the count of unclaimed active contacts;
- `BTN_TOOL_FINGER`, `BTN_TOOL_DOUBLETAP`, and related tool keys follow the unclaimed active contact count;
- legacy `ABS_X/Y` come from a representative unclaimed active slot;
- physical `BTN_LEFT`, `BTN_RIGHT`, and related pointer-button events are forwarded independently
  of contact ownership and are released during output cleanup;
- `SYN_DROPPED` releases tracked virtual contacts, ignores the unreliable tail through the next
  `SYN_REPORT`, then queries the kernel's current multitouch slot and physical-button state.
  Contacts and buttons that were already held during resync are restored so incomplete history
  cannot create a gesture or leave a virtual button stuck.

## uinput batching

The Rust `evdev` crate's `VirtualDevice::emit(&[InputEvent])` appends `SYN_REPORT` itself. `UinputRawOutputSink` buffers events until `sync()` and calls `emit` once per composed frame. It does not send each event as a separate uinput batch and does not include an explicit `SYN_REPORT` in the batch.

## Virtual touchpad capabilities

`VirtualTouchpadSpec` mirrors the output events that the composer can emit:

- properties: `INPUT_PROP_POINTER` plus the physical touchpad's properties, including
  `INPUT_PROP_BUTTONPAD` for clickpads;
- keys: `BTN_TOUCH`, `BTN_TOOL_FINGER`, `BTN_TOOL_DOUBLETAP`, `BTN_TOOL_TRIPLETAP`,
  `BTN_TOOL_QUADTAP`, `BTN_TOOL_QUINTTAP`, plus supported physical pointer buttons;
- absolute axes: `ABS_X`, `ABS_Y`, `ABS_MT_SLOT`, `ABS_MT_TRACKING_ID`, `ABS_MT_POSITION_X`, `ABS_MT_POSITION_Y`.

For live proxy mode, the virtual touchpad mirrors the physical device's input properties, physical
pointer-button capabilities, and absolute-axis value, min/max, fuzz, flat, and resolution where the
physical device exposes them. This keeps compositor/libinput behavior close to the real device.
Replay-only paths use captured metadata ranges and a conservative tracking-id range.

## Manual live uinput test

Normal test runs do not touch `/dev/uinput`:

```bash
cargo test
```

The live uinput boundary check is ignored by default:

```bash
cargo test --test uinput_live -- --ignored
```

It creates a temporary virtual touchpad, emits one center contact down/up frame through `UinputRawOutputSink`, and lets the virtual device drop at test end.

This test requires `/dev/uinput` and permission to create virtual input devices. If the OS denies that, the default unit and replay test suite can still pass.

## Safety boundary

- `devices`, `dump`, and `proxy --dry-run` do not grab the physical touchpad.
- `proxy --uinput --grab` is bounded by `--frames` and exits after cleanup.
- `daemon` is the long-running mode and should normally run as a user service.
