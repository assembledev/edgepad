# Passthrough and uinput

`edgepad` is moving toward a live proxy that reads a physical touchpad and emits safe passthrough events through a virtual touchpad. This is the risky part of the project, so it is being built in layers.

## Current layers

Implemented:

1. `RawFrame` preserves raw evdev event order.
2. `route_raw_frame` feeds only recognizer-relevant multitouch events into the engine.
3. `RawOutputComposer` synthesizes output state for unclaimed passthrough contacts.
4. `RawOutputSink` writes composed raw events frame-by-frame into a sink.
5. `UinputRawOutputSink` buffers one composed frame and flushes it to a uinput writer on `sync()`.
6. `VirtualTouchpadSpec` describes the virtual touchpad capability set from the physical device's absolute-axis metadata.
7. `proxy --dry-run` reads bounded live frames from a physical touchpad, routes/composes them, and prints counters without forwarding input.
8. `proxy --uinput --grab` creates a virtual touchpad, explicitly grabs the physical device, forwards composed passthrough frames to uinput, then ungrabs after the requested frame budget.
9. `daemon` reuses the same live proxy runtime without a frame budget and stops through Ctrl+C/SIGTERM.
10. Daemon gesture actions are dispatched through a bounded worker queue, and argv commands are waited on after spawn.

Still outside this layer:

- NixOS/Home Manager service wiring.

## Live dry-run proxy

`proxy --dry-run` is a bounded live inspection mode. It reads a physical touchpad stream, runs raw routing and output composition, prints counters, and exits after the requested frame boundary budget.

```bash
edgepad proxy --device /dev/input/event5 --frames 300 --dry-run
```

It does **not** create a virtual device, emit uinput events, suppress the physical touchpad, or call `EVIOCGRAB`. Use it to inspect what the live proxy would decide before enabling virtual output/grabbing.

The summary includes raw/event volume, recognizer events, passthrough vs claimed-edge frame counts, empty-output frames, composed output volume, final cleanup output volume, live uinput settle output volume, edge width, individual gestures, and aggregate gesture counts by zone/direction.

The default edge width is `0.10` on each side. Use `--edge-width F` to validate wider/narrower edge gesture comfort, for example:

```bash
edgepad proxy --device /dev/input/event5 --frames 300 --edge-width 0.20 --dry-run
```

## Bounded grab/uinput proxy

`proxy --uinput --grab` is the first live forwarding mode:

```bash
sudo edgepad proxy --device /dev/input/event5 --frames 300 --uinput --grab
```

It is intentionally bounded. The command:

1. opens the physical touchpad and reads its capabilities;
2. refuses to continue if the physical touchpad is already touched, so the proxy does not start in the middle of an unknown contact lifecycle;
3. creates the virtual touchpad through `/dev/uinput`;
4. only then calls `EVIOCGRAB` on the physical device;
5. routes/composes the requested frame boundary budget;
6. if the frame budget ends while the physical touchpad is still touched, keeps proxying briefly until all fingers are up, with a bounded timeout;
7. emits composed passthrough frames to the virtual touchpad;
8. emits a final synthetic release frame if the frame budget/drain stopped while a passthrough contact was still active;
9. emits one neutral settle frame that marks all virtual MT slots and touch/tool keys as released;
10. waits briefly so the compositor can consume the virtual neutral state before the physical device is ungrabbed;
11. ungrabs and exits.

The idle drain is not an unbounded shutdown lock. It is a short grace period for smooth handoff at the end of a bounded manual run; timeout status is reported as `idle_drain_timed_out`.

If virtual device creation fails, the physical device is not grabbed. `RawDevice` also ungrabs on drop, so errors during the bounded run do not intentionally leave the device grabbed.

There is no `--no-grab` duplicate-input mode in the main test path because duplicate touchpad streams are too noisy to evaluate by hand. Use `--dry-run` for inspection and `--uinput --grab` for the bounded live proxy test.

## Daemon mode

`daemon` is the long-running live proxy surface. It creates the virtual touchpad, grabs the physical touchpad, proxies until Ctrl+C or SIGTERM, then drains briefly until the physical touchpad is idle before cleanup and ungrab.

```bash
sudo edgepad daemon --device auto
```

The default device config is `auto`. Auto-detection scans readable `/dev/input/event*` nodes and succeeds only when exactly one touchpad candidate is present. If auto-detection is ambiguous, pass the event node explicitly:

```bash
sudo edgepad daemon --device /dev/input/event5
```

Config files use TOML:

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

Load a config with:

```bash
sudo edgepad daemon --config edgepad.conf
```

Gesture bindings are parsed, validated, and dispatched by daemon mode. Command actions are launched without a shell, so arguments are not re-split or interpreted. The action worker waits for each spawned child process before running the next command, which prevents short-lived action commands from accumulating as zombies while the daemon keeps running.

## Output policy

After raw routing, `edgepad` does **not** blindly forward raw global pointer-emulation events:

- `BTN_TOUCH`
- `BTN_TOOL_*`
- legacy `ABS_X`
- legacy `ABS_Y`

Those values can follow a claimed edge-owned contact while another center contact is active. Instead, output state is synthesized from unclaimed passthrough slots:

- multitouch slot events are preserved only for passthrough contacts;
- `BTN_TOUCH` follows the count of unclaimed active contacts;
- `BTN_TOOL_FINGER` / `BTN_TOOL_DOUBLETAP` / etc. follow the unclaimed active contact count;
- legacy `ABS_X/Y` come from a representative unclaimed active slot;
- `SYN_DROPPED` releases tracked virtual contacts and marks resync.

## uinput batching

The Rust `evdev` crate's `VirtualDevice::emit(&[InputEvent])` appends `SYN_REPORT` itself. `UinputRawOutputSink` therefore buffers events until `sync()` and calls `emit` once per composed frame. It does not send each event as a separate uinput batch and does not include an explicit `SYN_REPORT` in the batch.

## Virtual touchpad capability spec

`VirtualTouchpadSpec` mirrors the output events that the composer can emit:

- properties: `INPUT_PROP_POINTER`;
- keys: `BTN_TOUCH`, `BTN_TOOL_FINGER`, `BTN_TOOL_DOUBLETAP`, `BTN_TOOL_TRIPLETAP`, `BTN_TOOL_QUADTAP`, `BTN_TOOL_QUINTTAP`;
- absolute axes: `ABS_X`, `ABS_Y`, `ABS_MT_SLOT`, `ABS_MT_TRACKING_ID`, `ABS_MT_POSITION_X`, `ABS_MT_POSITION_Y`.

For live proxy mode, the virtual touchpad mirrors the physical device's absolute-axis `value`, min/max, fuzz, flat, and resolution where the physical device exposes them. This matters for libinput pointer acceleration and touchpad speed. For replay-only paths without a physical device handle, the spec falls back to captured metadata ranges and a conservative `ABS_MT_TRACKING_ID` range of `0..=65535`.

## Manual live uinput test

Normal test runs do not touch `/dev/uinput`:

```bash
cargo test
```

The live uinput boundary check is an ignored integration test:

```bash
cargo test --test uinput_live -- --ignored
```

It creates a temporary virtual touchpad from `VirtualTouchpadSpec`, emits one center contact down/up frame through `UinputRawOutputSink`, then lets the virtual device drop at test end.

This test requires a kernel/user environment that exposes `/dev/uinput` and allows the current user to create virtual input devices. If `/dev/uinput` is missing or permission is denied, the test fails with the underlying OS error. That failure means the live environment is not ready; it does not mean the default replay/unit test suite is broken.

## Safety boundary

Default tests do not require `/dev/uinput` and do not touch real hardware. The ignored live uinput test creates only a virtual device; it does not read, suppress, or grab a physical touchpad. The bounded `proxy --uinput --grab` command is the explicit manual hardware test path for physical-device grabbing.
