# Double-tap recognition

Double tap is a recognizer state, not an action-runner shortcut. Edgepad decides it in the same core
engine that owns Type-B slots, edge claims, tap slop, and `SYN_DROPPED` recovery. The action layer
only receives a finished `tap` or `double-tap` gesture.

## Input contract

A contact is a tap candidate only while all of these remain true:

- it starts in an active edge zone;
- its duration is at least `tap_min_duration_ms` and strictly below `tap_max_duration_ms`;
- neither normalized axis reaches `swipe_min_distance`;
- a slider on the same edge has not emitted a step.

To participate in a temporal sequence, the contact must also remain single-finger: any competing
contact makes the involved taps ineligible for double-tap recognition.

After the first valid tap, the engine keeps one pending sequence. A second contact completes the
double tap only when it starts on the same edge, starts before `double_tap_timeout_ms` expires, and
its initial position is within `double_tap_max_distance` of the first tap's release position. The
distance check is Euclidean in normalized X/Y space, so it behaves consistently across device
coordinate ranges.

Type-B coordinates belong to the kernel slot, not only to one contact. Linux can omit an X or Y
event when a new tracking ID reuses that axis value. The live proxy seeds all slot positions before
it announces readiness, the engine retains them across contact releases, and new coordinates win
only after their complete `SYN_REPORT` frame. This makes an exact same-position double tap work
without prematurely claiming a contact that actually moved to another zone.

The second contact must itself finish as a valid tap. Motion, a long hold, another finger, center
passthrough, a physical click, or a slider step breaks the sequence. `SYN_DROPPED` cancels it instead
of guessing across missing kernel events.

## Single-tap arbitration

An edge with both `tap` and `double-tap` bindings cannot emit the first tap immediately: doing so
would run both actions for every double tap. Edgepad therefore holds the first tap until one of three
things happens:

1. a matching second tap completes, producing one `double-tap`;
2. another interaction makes a double tap impossible, releasing the pending `tap` immediately;
3. the deadline expires, releasing the pending `tap` while the device is otherwise idle.

When an edge has only a `double-tap` binding, an expired first tap is dropped quietly instead of
being reported as an unmatched action. Edges without a `double-tap` binding keep immediate tap
behavior and pay no deadline latency.

## Timing architecture

The core engine stores deadlines in the same timestamp domain as evdev frames. The live proxy maps
the remaining duration onto `std::time::Instant` and includes it in the device poll timeout. When the
poll wakes without input, it advances the recognizer clock and dispatches the pending single tap.
This is what prevents the common bug where a tap action waits for the user's next movement.

Replay uses recorded frame timestamps and advances to the final pending deadline after the last
frame. Config reload also waits for both physical contacts and pending tap arbitration to finish, so
one sequence cannot straddle two recognition profiles.

Default thresholds:

| Setting | Default | Meaning |
| --- | ---: | --- |
| `tap_min_duration_ms` | `40` | Reject sensor chatter and accidental brushes. |
| `tap_max_duration_ms` | `180` | Reject holds. |
| `double_tap_timeout_ms` | `300` | Maximum first-release to second-contact gap. |
| `double_tap_max_distance` | `0.04` | Maximum normalized distance between tap targets. |
| `swipe_min_distance` | `0.02` | Per-axis movement that permanently leaves tap state. |

The Linux kernel documents `BTN_TOOL_DOUBLETAP` as the two-finger tool-count signal, so it is not a
temporal double-tap event: [Linux input event codes](https://docs.kernel.org/input/event-codes.html).
The recognizer instead follows the normal tap constraints described by
[libinput](https://wayland.freedesktop.org/libinput/doc/latest/tapping.html): bounded contact time,
bounded movement, explicit state transitions, and timer-driven completion.

## Regression surface

The focused contract lives in `tests/core_invariants.rs`. Replay coverage uses
`tests/fixtures/left-edge-double-tap.ev`, while CLI, action dispatch, deadline wake-up, reload, and
Home Manager serialization have separate tests at their owning layers. Ignored tests in
`tests/uinput_live.rs` exercise a real source device, `EVIOCGRAB`, virtual output, kernel timestamp
delivery, unchanged-axis suppression, and idle deadline wake-up through `/dev/uinput`.
