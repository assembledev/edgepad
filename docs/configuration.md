# Configuration

Edgepad reads `~/.config/edgepad/edgepad.toml`, or
`$XDG_CONFIG_HOME/edgepad/edgepad.toml` when that variable is set. Use
`edgepad daemon --config <file>` to select another file for a foreground run.
For declarative configuration, see the [Home Manager setup](nix.md).

The [example config](../examples/edgepad.toml.example) has bindings for all four
edges using notification commands. Replace those commands with your own.

## Gestures

A gesture starts in an edge zone and runs its action when the finger lifts.
Zones are `left`, `right`, `top`, and `bottom`. Directions are `up`, `down`,
`left`, `right`, `tap`, and `double-tap`.

For example, swipe right from the top edge to show a notification:

```toml
[[gestures]]
zone = "top"
direction = "right"
action = ["notify-send", "edgepad", "top-right"]
```

Each zone/direction pair can have one binding. Edges without bindings remain
available for normal touchpad input.

## Sliders

Sliders repeat actions while your finger moves. Left and right edges use
`up`/`down`; top and bottom edges use `left`/`right`.

```toml
[[sliders]]
zone = "left"
step = 0.04
up = ["pamixer", "-i", "3"]
down = ["pamixer", "-d", "3"]
```

`step` is the distance between actions as a fraction of the corresponding
touchpad axis. The default, `0.04`, means one step per 4% of travel. Smaller values
produce more steps for the same movement.

An edge can have one slider alongside tap and double-tap bindings. It cannot
have both a slider and a directional swipe binding. Once a slider emits a step,
lifting the finger won't also trigger a tap.

## Actions

An action is an array containing a program and its arguments. The program must
be installed and available on the user service's `PATH`, or specified by its
full path. Run `edgepad doctor` to check for missing commands.

Commands are not passed through a shell. If you need pipes, redirection, or other
shell syntax, invoke a shell explicitly:

```toml
[[gestures]]
zone = "bottom"
direction = "tap"
action = ["sh", "-c", "date >> /tmp/edgepad-actions.log"]
```

Run edgepad as a user service for desktop actions. A daemon started with `sudo`
runs actions in root's environment.

## Device and edge widths

Put these settings **before** any `[[gestures]]` or `[[sliders]]` tables:

```toml
device = "auto"
edge_width = 0.10
left_edge_width = 0.08
right_edge_width = 0.12
```

`auto` selects the touchpad when exactly one readable candidate is found. If
there are several, run `edgepad devices` and set an explicit path, such as
`device = "/dev/input/event7"`.

`edge_width` defaults to `0.10`: 10% of the relevant touchpad dimension for each
edge. `left_edge_width`, `right_edge_width`, `top_edge_width`, and
`bottom_edge_width` override individual edges. Omitted overrides use `edge_width`.
Only edges with bindings reserve space for gestures.

## Tap and swipe sensitivity

These are also top-level settings, placed before the binding tables. Distances
are normalized to the touchpad's axes rather than measured in raw device units.

| Setting | Default | Meaning |
| --- | --- | --- |
| `tap_min_duration_ms` | `40` | Minimum tap duration. Set to `0` to allow very short taps. |
| `tap_max_duration_ms` | `180` | A tap must end before this duration; longer contacts are holds. |
| `swipe_min_distance` | `0.02` | Per-axis travel needed to become a swipe: 2% of the axis. |
| `double_tap_timeout_ms` | `300` | Maximum time from the first tap's release to the second tap's start. |
| `double_tap_max_distance` | `0.04` | Maximum Euclidean distance between the first tap's release and the second tap's start, in normalized coordinates. |

A contact stops qualifying as a tap once it reaches the swipe distance, even if
it returns to its starting point.

Double taps must land on the same edge within both the time and distance limits.
If an edge has both `tap` and `double-tap` bindings, the single-tap action waits
for the double-tap window to expire, unless another interaction rules out the
second tap first. Edges without a double-tap binding run taps immediately on
release. See [double-tap recognition](double-tap.md) for the full behavior.

## Apply changes

```bash
systemctl --user reload edgepad.service
```

Edgepad validates the whole file before applying it. An invalid reload leaves
the previous configuration running; check errors with:

```bash
journalctl --user -u edgepad.service -b
```

A reload waits for active touches and pending tap sequences to finish. Changing
`device` requires `systemctl --user restart edgepad.service`.

Home Manager applies recognition and binding changes with a reload when you
activate the new generation. Package or device changes restart the service.
