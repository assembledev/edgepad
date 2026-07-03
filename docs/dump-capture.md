# Dump Capture

`edgepad dump` captures `.ev` samples from a real touchpad without grabbing or suppressing the device.

## Replay-format capture

Replay-format capture keeps only recognizer-relevant Type-B multitouch events:

```bash
edgepad devices              # touchpad candidates only
edgepad devices --all        # optional full device list
edgepad dump --device /dev/input/event5 --out bug.ev --frames 300
edgepad replay bug.ev
```

The output format is the same text fixture format used by replay tests, so a useful bug capture can later be copied into `tests/fixtures/` and turned into a regression test.

## Raw capture

Raw capture keeps the real evdev event shape needed for passthrough/output policy work:

```bash
edgepad dump --raw --device /dev/input/event5 --out bug.raw.ev --frames 300
edgepad replay-raw bug.raw.ev
```

Use raw capture when investigating virtual-device passthrough behavior. It preserves frame boundaries and names known touchpad-relevant events such as:

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

Unknown event types/codes are kept with numeric fallback instead of being silently dropped.

## Frame limits and capture flow

With `--frames N`, capture stops after N frame boundaries (`SYN_REPORT` or `SYN_DROPPED`) and prints a short summary: output path, device path, captured capabilities, written frame boundaries, written events, and the next command.

For frame-limited edge gesture captures, a useful flow is:

1. start capture;
2. perform the edge or mixed gesture;
3. release the gesture finger;
4. place a finger in the center until the frame budget finishes.

This captures the edge gesture release while keeping the event stream active. Without `--frames`, stop capture manually with `Ctrl+C` after reproducing the gesture.

## Capability metadata

The capture header includes real device capabilities when evdev exposes them:

```text
# slots: 0..=4
# x: 10..=1210
# y: 20..=820
```

`edgepad replay` and `edgepad replay-raw` use this metadata when present instead of temporary defaults.

## Safety

Current `dump` behavior is read-only:

- opens the event node for reading;
- uses raw evdev reads so `SYN_DROPPED` is preserved instead of silently hidden;
- writes to the requested output file only;
- does **not** call `EVIOCGRAB`;
- does **not** create `uinput` devices;
- does **not** forward or suppress real input.

Reading `/dev/input/event*` may require `sudo`, group `input`, or seat/logind ACLs.

## Current limitations

- No automatic touchpad selection yet; use `edgepad devices` first.
- No duration/time limit yet; use `--frames N` or `Ctrl+C`.
- Live proxy output through `uinput` is not implemented yet; use `proxy --dry-run` for bounded live routing/composer inspection.

The capture path stays read-only so samples can be collected before device grabbing or live virtual input are implemented.
