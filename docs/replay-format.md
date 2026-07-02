# Replay Fixture Format

Replay fixtures are small text files that describe evdev-style input frames without needing a real touchpad.

They are for tests and debugging, not user configuration.

## Syntax

Each non-empty line is one event:

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

## Inspecting a fixture manually

The minimal CLI can run a fixture through the current engine and print a summary:

```bash
cargo run -- replay tests/fixtures/left-edge-swipe-right.ev
```

Expected shape:

```text
frames: 3
passthrough_events: 0
gestures: 1
gesture slot=0 tracking_id=123 zone=left direction=right
resync_required: false
```

This is a debug/demo helper, not a replacement for `cargo test`. The CLI currently uses temporary fixture defaults: slots `0..=9`, X range `0..1000`, Y range `0..700`, edge width `10%`. Real device capabilities belong in the later dump/capture path.

## Rationale

Input daemons fail in ugly ways when slot lifecycle is wrong: ghost fingers, stuck touches, shifted finger counts, or compositor gestures needing one extra finger. Fixtures let us turn every such bug into a regression test before touching real `/dev/input` or `uinput`.
