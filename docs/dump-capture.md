# Dump capture

`edgepad dump` records touchpad samples without grabbing, suppressing, or forwarding input. Use it when you want a real device trace that can be replayed, shared, or turned into a regression test.

## Find the touchpad

```bash
edgepad devices
```

Show every readable event node when the filtered list is not enough:

```bash
edgepad devices --all
```

Reading `/dev/input/event*` may require `sudo`, the `input` group, or seat/logind ACLs.

## Running daemon

The live edgepad daemon holds the physical touchpad with `EVIOCGRAB`, so a separate `edgepad dump`
process cannot receive physical events while `edgepad.service` is running. Stop the service before
capturing and start it again afterward:

```bash
(
  systemctl --user stop edgepad.service
  trap 'systemctl --user start edgepad.service' EXIT
  edgepad dump --device auto --out bug.ev --frames 300
)
```

`dump` never stops the service or probes the device with `EVIOCGRAB`. It prints the selected device
and waits normally; if no input arrives within three seconds, it warns that the device may be
grabbed and continues waiting. If input arrives later, it confirms that capture has started and
writes the events normally.

## Recognition profile

`edgepad replay`, `edgepad replay-raw`, and `edgepad proxy` use the default user config unless
`--config <file>` selects another one. This keeps active zones, edge width, swipe threshold, and
sliders aligned with the daemon. Use `--built-in-defaults` only when intentionally testing the
standalone recognizer profile. The commands print `profile: config ...` or
`profile: built-in defaults`; replay never executes configured actions.
Dump files include the kernel timestamp on every frame boundary, so replay applies
`tap_min_duration_ms` exactly like the live proxy.

## Replay-format capture

Replay-format capture keeps the Type-B multitouch events used by the recognizer:

```bash
edgepad dump --device auto --out bug.ev --frames 300
edgepad replay bug.ev
```

The output uses the same text fixture format as the replay tests. A useful bug capture can be copied into `tests/fixtures/` and promoted into a regression test.
If auto-detection finds multiple touchpads, use the event node reported by `edgepad devices`.

## Raw capture

Raw capture keeps the evdev event shape needed for passthrough and virtual touchpad output:

```bash
edgepad dump --raw --device auto --out bug.raw.ev --frames 300
edgepad replay-raw bug.raw.ev
```

Known touchpad-relevant events are written by name:

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

The final value is the frame timestamp in microseconds.

Unknown event types and codes are kept with numeric fallback instead of being silently dropped.

## Frame limits

With `--frames N`, capture records at least N frame boundaries (`SYN_REPORT` or `SYN_DROPPED`).
If the budget is reached while a contact is active, dump asks you to release all contacts and keeps
recording through the release frame before it prints the summary:

- output path;
- device path;
- captured capabilities;
- written frame boundaries;
- written events;
- next replay command.

For frame-limited edge gesture captures, a useful flow is:

1. start capture;
2. perform the edge or mixed gesture;
3. release the gesture finger;
4. if dump is still below its frame budget, move a finger in the center to produce more frames;
5. when dump reports that the frame budget was reached, release all contacts.

Without `--frames`, stop capture manually with Ctrl+C after reproducing the gesture.

## Capability metadata

Captures include real device ranges when evdev exposes them:

```text
# edgepad .ev dump
# device: /dev/input/event5
# slots: 0..=4
# x: 10..=1210
# y: 20..=820
```

`edgepad replay` and `edgepad replay-raw` use this metadata for slot and edge-zone decisions. Handwritten fixtures without metadata still work with default ranges.

## Safety

`dump` is read-only:

- opens the event node for reading;
- preserves `SYN_DROPPED` instead of hiding it;
- writes only to the requested output file;
- does not call `EVIOCGRAB`;
- does not create `uinput` devices;
- does not forward or suppress real input.

## Related commands

```bash
edgepad replay bug.ev
edgepad replay-raw bug.raw.ev
edgepad proxy --device /dev/input/event5 --frames 300 --dry-run
```
