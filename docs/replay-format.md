# Replay Fixture Format

Replay fixtures are small text files that describe evdev-style input frames without needing a real touchpad.

They are for tests and debugging, not user configuration.

## Replay-format syntax

Each non-empty non-comment line is one recognizer-level event:

```text
ABS_MT_SLOT <slot>
ABS_MT_TRACKING_ID <id|-1>
ABS_MT_POSITION_X <x>
ABS_MT_POSITION_Y <y>
SYN_REPORT
SYN_DROPPED
```

Comments are allowed:

```text
# full line comment
ABS_MT_SLOT 0 # inline comment
```

`SYN_REPORT` ends the current frame. Events before the next `SYN_REPORT` are processed together.

`SYN_DROPPED` creates a standalone frame that tells the engine to clear local touch state and require resync.

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
EV_SYN SYN_REPORT 0
```

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

Old handwritten fixtures without metadata still work; replay falls back to temporary defaults:

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
SYN_REPORT

ABS_MT_SLOT 0
ABS_MT_POSITION_X 220
ABS_MT_POSITION_Y 310
SYN_REPORT

ABS_MT_SLOT 0
ABS_MT_TRACKING_ID -1
SYN_REPORT
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
cargo run -- replay tests/fixtures/left-edge-swipe-right.ev
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
cargo run -- replay-raw bug.raw.ev
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
