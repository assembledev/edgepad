# Device Discovery

`edgepad devices` is the first read-only bridge from replay fixtures toward real hardware.

It lists readable `/dev/input/event*` devices and classifies Type-B multitouch pointer devices as touchpad candidates.

```bash
edgepad devices
```

Output shape:

```text
/dev/input/event5 kind=touchpad name="Example Touchpad" id=1234:5678 slots=0..=4 x=0..=1000 y=0..=700
```

For tests/debugging, a root can be overridden:

```bash
edgepad devices --root /tmp/fake-input-root
```

## Safety

This command is read-only:

- no `EVIOCGRAB`;
- no `uinput`;
- no event forwarding;
- no daemon loop;
- no command dispatch.

If no event devices are readable, it prints a no-devices message. On a real machine, reading `/dev/input/event*` may require `sudo`, group `input`, or seat/logind ACLs.

## Rationale

Before `edgepad dump --device ... --out bug.ev`, users need a way to identify the correct touchpad event node without guessing.
