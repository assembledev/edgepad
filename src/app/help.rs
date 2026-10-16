//! User-facing command syntax, kept verbatim alongside parser usage errors.

pub(super) const USAGE: &str = "usage: edgepad replay <fixture.ev> [--config <file> | --built-in-defaults] | edgepad replay-raw <raw.ev> [--config <file> | --built-in-defaults] | edgepad devices [--input-root <input-root>] [--all] | edgepad status [--config <file>] [--device auto|<event-node>] [--input-root <input-root>] [--service <unit>] | edgepad doctor [--config <file>] [--device auto|<event-node>] [--input-root <input-root>] [--uinput <path>] [--service <unit>] | edgepad dump --device auto|<event-node> --out <file.ev> [--input-root <input-root>] [--frames N] [--raw] | edgepad proxy --frames N (--dry-run | --uinput --grab) [--config <file> | --built-in-defaults] [--device auto|<event-node>] [--input-root <input-root>] [--edge-width F] | edgepad daemon [--config <file>] [--device auto|<event-node>] [--input-root <input-root>] [--edge-width F]";

pub(super) const HELP: &str = "\
Usage:
  edgepad <command> [options]

Commands:
  devices     List readable input devices and touchpad candidates
  status      Show a short daemon/config/device summary
  doctor      Check runtime prerequisites and service health
  daemon      Run the live edge-gesture proxy
  dump        Capture touchpad events into a replay fixture
  proxy       Run a bounded live proxy session for diagnostics
  replay      Replay a parsed fixture through the recognizer
  replay-raw  Replay a raw evdev capture through routing and output composition

Global options:
  -h, --help     Show this help
  -V, --version  Show version
";

pub(super) const REPLAY_USAGE: &str =
    "usage: edgepad replay <fixture.ev> [--config <file> | --built-in-defaults]";

pub(super) const REPLAY_HELP: &str = "\
Usage:
  edgepad replay <fixture.ev> [--config <file> | --built-in-defaults]

Replay a parsed fixture through the recognizer and print summary statistics.

Options:
      --config <file>        TOML config path
                             [default: $XDG_CONFIG_HOME/edgepad/edgepad.toml or ~/.config/edgepad/edgepad.toml]
      --built-in-defaults    Use the built-in recognizer profile instead of a config
";

pub(super) const REPLAY_RAW_USAGE: &str =
    "usage: edgepad replay-raw <raw.ev> [--config <file> | --built-in-defaults]";

pub(super) const REPLAY_RAW_HELP: &str = "\
Usage:
  edgepad replay-raw <raw.ev> [--config <file> | --built-in-defaults]

Replay a raw evdev capture through recognizer routing and output composition.

Options:
      --config <file>        TOML config path
                             [default: $XDG_CONFIG_HOME/edgepad/edgepad.toml or ~/.config/edgepad/edgepad.toml]
      --built-in-defaults    Use the built-in recognizer profile instead of a config
";

pub(super) const DEVICES_USAGE: &str = "usage: edgepad devices [--input-root <input-root>] [--all]";

pub(super) const DEVICES_HELP: &str = "\
Usage:
  edgepad devices [--input-root <input-root>] [--all]

Options:
      --input-root <input-root>  Input device directory [default: /dev/input]
      --root <input-root>        Alias for --input-root
      --all                      Show all readable event devices, not only touchpad candidates
";

pub(super) const DUMP_USAGE: &str = "usage: edgepad dump --device auto|<event-node> --out <file.ev> [--input-root <input-root>] [--frames N] [--raw]";

pub(super) const DUMP_HELP: &str = "\
Usage:
  edgepad dump --device auto|<event-node> --out <file.ev> [--input-root <input-root>] [--frames N] [--raw]

The physical device must not be grabbed by another process.
Stop edgepad.service before capturing when the daemon owns the touchpad.

Options:
      --device auto|<event-node>  Physical touchpad selection
      --input-root <input-root>   Input device directory for auto-detect [default: /dev/input]
      --out <file.ev>             Output fixture path
      --frames N                  Capture at least N frame boundaries, then stop when idle
      --raw                       Write raw evdev events instead of replay fixture events
";

pub(super) const PROXY_USAGE: &str = "usage: edgepad proxy --frames N (--dry-run | --uinput --grab) [--config <file> | --built-in-defaults] [--device auto|<event-node>] [--input-root <input-root>] [--edge-width F]";

pub(super) const PROXY_HELP: &str = "\
Usage:
  edgepad proxy --frames N (--dry-run | --uinput --grab) [--config <file> | --built-in-defaults] [--device auto|<event-node>] [--input-root <input-root>] [--edge-width F]

Options:
      --config <file>             TOML config path
                                  [default: $XDG_CONFIG_HOME/edgepad/edgepad.toml or ~/.config/edgepad/edgepad.toml]
      --built-in-defaults         Use the built-in recognizer profile instead of a config
      --device auto|<event-node>  Override config device selection; required with built-in defaults
      --input-root <input-root>   Input device directory for auto-detect [default: /dev/input]
      --frames N                  Stop after N frame boundaries
      --dry-run                   Recognize and report without emitting output
      --uinput --grab             Grab the physical device and emit through /dev/uinput
      --edge-width F              Override edge zone width from the selected profile
";

pub(super) const DAEMON_USAGE: &str = "usage: edgepad daemon [--config <file>] [--device auto|<event-node>] [--input-root <input-root>] [--edge-width F]";

pub(super) const DAEMON_HELP: &str = "\
Usage:
  edgepad daemon [--config <file>] [--device auto|<event-node>] [--input-root <input-root>] [--edge-width F]

Options:
      --config <file>             TOML config path
                                  [default: $XDG_CONFIG_HOME/edgepad/edgepad.toml or ~/.config/edgepad/edgepad.toml]
      --device auto|<event-node>  Touchpad device selection [default: auto]
      --input-root <input-root>   Input device directory for auto-detect [default: /dev/input]
      --edge-width F              Edge zone width as a fraction of the touchpad axis

Signals:
      SIGHUP                      Reload config after active contacts lift
";

pub(super) const DOCTOR_USAGE: &str = "usage: edgepad doctor [--config <file>] [--device auto|<event-node>] [--input-root <input-root>] [--uinput <path>] [--service <unit>]";

pub(super) const DOCTOR_HELP: &str = "\
Usage:
  edgepad doctor [--config <file>] [--device auto|<event-node>] [--input-root <input-root>] [--uinput <path>] [--service <unit>]

Options:
      --config <file>             TOML config path
                                  [default: $XDG_CONFIG_HOME/edgepad/edgepad.toml or ~/.config/edgepad/edgepad.toml]
      --device auto|<event-node>  Override touchpad device selection from config
      --input-root <input-root>   Input device directory for auto-detect [default: /dev/input]
      --uinput <path>             uinput device path [default: /dev/uinput]
      --service <unit>            systemd user unit to inspect [default: edgepad.service]
";

pub(super) const STATUS_USAGE: &str = "usage: edgepad status [--config <file>] [--device auto|<event-node>] [--input-root <input-root>] [--service <unit>]";

pub(super) const STATUS_HELP: &str = "\
Usage:
  edgepad status [--config <file>] [--device auto|<event-node>] [--input-root <input-root>] [--service <unit>]

Options:
      --config <file>             TOML config path
                                  [default: $XDG_CONFIG_HOME/edgepad/edgepad.toml or ~/.config/edgepad/edgepad.toml]
      --device auto|<event-node>  Override touchpad device selection from config
      --input-root <input-root>   Input device directory for auto-detect [default: /dev/input]
      --service <unit>            systemd user unit to inspect [default: edgepad.service]
";
