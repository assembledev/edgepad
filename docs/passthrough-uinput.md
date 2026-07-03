# Passthrough and uinput

`edgepad` is moving toward a live proxy that reads a physical touchpad and emits safe passthrough events through a virtual touchpad. This is the risky part of the project, so it is being built in layers.

## Current layers

Implemented:

1. `RawFrame` preserves raw evdev event order.
2. `route_raw_frame` feeds only recognizer-relevant multitouch events into the engine.
3. `RawOutputComposer` synthesizes output state for unclaimed passthrough contacts.
4. `RawOutputSink` writes composed raw events frame-by-frame into a sink.
5. `UinputRawOutputSink` buffers one composed frame and flushes it to a uinput writer on `sync()`.
6. `VirtualTouchpadSpec` describes the virtual touchpad capability set from captured device ranges.
7. `proxy --dry-run` reads bounded live frames from a physical touchpad, routes/composes them, and prints counters without forwarding input.

Not wired into a live forwarding command yet:

- opening `/dev/uinput` from the CLI;
- creating a real virtual input device during normal commands;
- writing live proxy output to a virtual node;
- `EVIOCGRAB`;
- daemon/service mode.

## Live dry-run proxy

`proxy --dry-run` is a bounded live inspection mode. It reads a physical touchpad stream, runs raw routing and output composition, prints counters, and exits after the requested frame boundary budget.

```bash
edgepad proxy --device /dev/input/event5 --frames 300 --dry-run
```

It does **not** create a virtual device, emit uinput events, suppress the physical touchpad, or call `EVIOCGRAB`. Use it to inspect what the live proxy would decide before enabling virtual output/grabbing.

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

The X/Y and slot ranges come from captured device metadata. `ABS_MT_TRACKING_ID` currently uses a conservative `0..=65535` range.

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

Default tests do not require `/dev/uinput` and do not touch real hardware. The live uinput test creates only a virtual device; it does not read, suppress, or grab a physical touchpad. Physical device grabbing belongs later, after live virtual output is proven.
