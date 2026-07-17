# Replay Fixture Format

Replay fixtures are small text files that describe evdev-style input frames without needing a real touchpad.

They are for tests and debugging, not user configuration.

## Recognition profile

`edgepad replay` and `edgepad replay-raw` load the default user config so their active zones, edge
width, swipe threshold, and sliders match the daemon. Select another config with `--config <file>`.
For a hermetic fixture run that intentionally ignores user configuration, pass
`--built-in-defaults`. The output always names the selected profile and recognizer settings. Replay
does not execute configured actions.
Every frame carries a timestamp, so replay applies `tap_min_duration_ms` like the live proxy.

## Replay-format syntax

Each non-empty non-comment line is one recognizer-level event:

```text
ABS_MT_SLOT <slot>
ABS_MT_TRACKING_ID <id|-1>
ABS_MT_POSITION_X <x>
ABS_MT_POSITION_Y <y>
SYN_REPORT <timestamp_us>
SYN_DROPPED <timestamp_us>
```

Comments are allowed:

```text
# full line comment
ABS_MT_SLOT 0 # inline comment
```

`SYN_REPORT` ends the current frame. Events before it are processed together at the supplied kernel
timestamp. Timestamps are integer microseconds and must not decrease. Handwritten fixtures may use
relative values starting at zero because the recognizer only compares elapsed time.

`SYN_DROPPED` creates a standalone timestamped frame that tells the engine to clear local touch
state and require resync. A frame boundary without a timestamp is rejected.
The live proxy additionally ignores events through the next `SYN_REPORT`, queries the kernel slot
snapshot, and restores already-held contacts as passthrough. Text replay has no physical device to
query, so a fixture must provide a fresh complete contact after `SYN_DROPPED` when it wants to model
post-resync input.

## Raw dump syntax

Raw dumps use Linux event type and code names when known:

```text
EV_ABS ABS_MT_SLOT 0
EV_ABS ABS_MT_TRACKING_ID 123
EV_ABS ABS_MT_POSITION_X 500
EV_ABS ABS_MT_POSITION_Y 300
EV_KEY BTN_TOUCH 1
EV_KEY BTN_TOOL_FINGER 1
EV_ABS ABS_X 500
EV_ABS ABS_Y 300
EV_MSC MSC_TIMESTAMP 123456
EV_SYN SYN_REPORT 0 123456789
```

For `SYN_REPORT` and `SYN_DROPPED`, the fourth value is the required frame timestamp in
microseconds. `EV_MSC MSC_TIMESTAMP` remains a raw device event and is not a substitute for the
kernel frame timestamp.

Unknown event types/codes are preserved with numeric fallback. Raw replay routes only recognizer-relevant MT events into the engine, then composes output events for passthrough contacts. It does not blindly forward raw `BTN_TOUCH`, `BTN_TOOL_*`, or legacy `ABS_X/Y`; those are synthesized from unclaimed passthrough contacts.

## Metadata header

Real captures can include capability metadata in comments:

```text
# edgepad .ev dump
# device: /dev/input/event5
# slots: 0..=4
# x: 10..=1210
# y: 20..=820
```

`edgepad replay` and `edgepad replay-raw` use this metadata when present instead of fixture defaults. Edge-zone decisions use the real touchpad coordinate range and slot range.

Fixtures without capability metadata use these temporary ranges:

```text
slots: 0..=9
x: 0..=1000
y: 0..=700
```

If any of `slots`, `x`, or `y` is present, all three must be present. Partial metadata is rejected so broken captures do not silently run with fake defaults.

## Example: left-edge swipe right

```text
ABS_MT_SLOT 0
ABS_MT_TRACKING_ID 123
ABS_MT_POSITION_X 20
ABS_MT_POSITION_Y 300
SYN_REPORT 0

ABS_MT_SLOT 0
ABS_MT_POSITION_X 220
ABS_MT_POSITION_Y 310
SYN_REPORT 50000

ABS_MT_SLOT 0
ABS_MT_TRACKING_ID -1
SYN_REPORT 100000
```

Human translation:

1. Finger appears in slot 0 at `x=20`, `y=300`.
2. The same finger moves right to `x=220`, `y=310`.
3. The finger is lifted with `ABS_MT_TRACKING_ID -1`.

For a device with X range `0..1000` and a left edge width of `10%`, this fixture produces a left-zone swipe-right gesture and emits no passthrough events.

## Current regression fixtures

```text
tests/fixtures/left-edge-swipe-right.ev
tests/fixtures/center-touch-passthrough.ev
tests/fixtures/mixed-edge-and-center.ev
tests/fixtures/duplicate-tracking-id.ev
tests/fixtures/syn-dropped-reset.ev
```

These cover the minimum lifecycle cases before real device I/O: claimed edge contact, normal passthrough contact, mixed claimed/passthrough slots in one stream, duplicate tracking ID rejection, and `SYN_DROPPED` recovery.

## Inspecting captures manually

Run a replay-format fixture or capture through the current engine:

```bash
cargo run -- replay tests/fixtures/left-edge-swipe-right.ev --built-in-defaults
```

Expected shape:

```text
capabilities: defaults slots=0..=9 x=0..=1000 y=0..=700
frames: 3
events: total=9 slot=3 tracking_start=1 tracking_end=1 x=2 y=2 syn_dropped=0
contacts: started=1 ended=1
passthrough_events: 0
gestures: 1
gesture slot=0 tracking_id=123 zone=left direction=right
slider_steps: 0
resync_required: false
```

Run a raw capture through routing and output composition:

```bash
cargo run -- replay-raw bug.raw.ev --built-in-defaults
```

Expected shape:

```text
capabilities: metadata slots=0..=4 x=10..=1210 y=20..=820
raw_frames: 300
raw_events: total=...
recognizer_passthrough_events: ...
composed_events: ...
gestures: ...
slider_steps: ...
resync_required: false
```

If the raw capture ends with an active passthrough contact, output composition includes a final synthetic release frame so replay inspection matches the bounded live proxy cleanup behavior.

The summary also prints lightweight capture diagnostics. With pure `--frames N`, a capture can end with an active center contact because the frame budget stopped mid-contact. For edge gesture captures, a useful workflow is to perform the gesture, release it, then place a finger in the center until `--frames` finishes.

This is a debug/demo helper, not a replacement for `cargo test`. Fixtures without metadata use default ranges; captures produced by `edgepad dump` include real device ranges and replay uses those instead.

## Rationale

Input daemons fail in ugly ways when slot lifecycle is wrong: ghost fingers, stuck touches, shifted finger counts, or compositor gestures needing one extra finger. Fixtures let us turn every such bug into a regression test before touching real `/dev/input` or `uinput`.
