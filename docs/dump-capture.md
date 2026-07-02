# Dump Capture

`edgepad dump` captures replayable `.ev` samples from a real touchpad.

```bash
edgepad dump --device /dev/input/event5 --out bug.ev
```

Workflow:

```bash
edgepad devices
edgepad dump --device /dev/input/event5 --out bug.ev --frames 60
edgepad replay bug.ev
```

With `--frames N`, capture stops automatically after N frame boundaries (`SYN_REPORT` or `SYN_DROPPED`). Without `--frames`, stop capture manually with `Ctrl+C` after reproducing the gesture.

The output format is the same text fixture format used by replay tests, so a useful bug capture can later be copied into `tests/fixtures/` and turned into a regression test.

The capture header includes real device capabilities when evdev exposes them:

```text
# slots: 0..=4
# x: 10..=1210
# y: 20..=820
```

`edgepad replay` reads this header and runs the engine with those ranges instead of temporary defaults.

## Safety

Current `dump` behavior is read-only:

- opens the event node for reading;
- uses raw evdev reads so `SYN_DROPPED` is preserved instead of silently hidden;
- writes only replay-relevant events: `ABS_MT_SLOT`, `ABS_MT_TRACKING_ID`, `ABS_MT_POSITION_X`, `ABS_MT_POSITION_Y`, `SYN_REPORT`, `SYN_DROPPED`;
- ignores unrelated key/mouse/metadata events;
- does **not** call `EVIOCGRAB`;
- does **not** create `uinput` devices;
- does **not** forward or suppress real input.

Reading `/dev/input/event*` may require `sudo`, group `input`, or seat/logind ACLs.

## Current limitations

- No automatic touchpad selection yet; use `edgepad devices` first.
- No duration/time limit yet; use `--frames N` or `Ctrl+C`.
- No richer capture metadata beyond source path and core capabilities.

These are intentional boundaries. This commit is about making `.ev` files easy to obtain and replay with real device ranges without touching dangerous passthrough/uinput behavior.
