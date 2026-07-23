# Device discovery

`edgepad devices` lists touchpad candidates from `/dev/input/event*`.

```bash
edgepad devices
```

Default output is intentionally filtered. A typical laptop has keyboards, lid switches, audio jacks, hotkeys, touchscreens, and other event nodes; for edge gestures the useful default is the touchpad candidate list.

Example:

```text
/dev/input/event7 kind=touchpad name="Example Touchpad" id=1234:5678 slots=0..=4 x=0..=4000 y=0..=2500
```

For the full raw list:

```bash
edgepad devices --all
```

For tests/debugging, the input root can be overridden:

```bash
edgepad devices --input-root /tmp/fake-input-root
edgepad devices --input-root /tmp/fake-input-root --all
```

`--root` remains available as an alias for compatibility.

## Permissions

Without permission to open event nodes, Linux may expose `/dev/input/event*` paths while denying reads. In that case `edgepad devices` prints a permission hint instead of pretending no hardware exists.

Typical options:

- run the discovery command with `sudo`;
- use the `input` group if that matches the system policy;
- rely on seat/logind ACLs from an active graphical session.

## Safety

This command is read-only:

- no `EVIOCGRAB`;
- no `uinput`;
- no event forwarding;
- no command dispatch.

## Rationale

Auto-selection in `edgepad dump --device auto` and `daemon --device auto` uses these same discovery
rules. When multiple candidates are present, use `edgepad devices` and pass the chosen path
explicitly.

Auto-selection succeeds only when exactly one readable touchpad candidate is found.
