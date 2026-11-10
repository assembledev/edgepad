#!/bin/sh
set -eu

INSTALLER_VERSION="v0.4.0"
REPO="${EDGEPAD_REPO:-assembledev/edgepad}"
VERSION="${EDGEPAD_VERSION:-latest}"
BIN_DIR="${HOME}/.local/bin"
CONFIG_HOME="${XDG_CONFIG_HOME:-${HOME}/.config}"
CONFIG_DIR="${CONFIG_HOME}/edgepad"
CONFIG_FILE="${CONFIG_DIR}/edgepad.toml"
REFERENCE_FILE="${CONFIG_DIR}/edgepad.toml.example"
STARTER_VOLUME=0
STARTER_BRIGHTNESS=0
STARTER_NOTIFICATIONS=0
NEW_CONFIG=0
SYSTEMD_USER_DIR="${CONFIG_HOME}/systemd/user"
SERVICE_FILE="${SYSTEMD_USER_DIR}/edgepad.service"
UDEV_RULE_FILE="${EDGEPAD_UDEV_RULE_FILE:-/etc/udev/rules.d/70-edgepad.rules}"
DRY_RUN=0
MODE="install"
PURGE=0

usage() {
    cat <<'EOF'
usage: install.sh [--dry-run] [--uninstall] [--purge]

Installs edgepad for the current user from GitHub Releases.

Options:
  --dry-run       Print the install or uninstall plan without changing files
  --uninstall     Remove files installed by this script
  --purge         With --uninstall, also remove ~/.config/edgepad

Environment:
  EDGEPAD_REPO=owner/repo      GitHub repository [default: assembledev/edgepad]
  EDGEPAD_VERSION=vX.Y.Z      Release tag to install [default: latest]
  XDG_CONFIG_HOME=path        User config root [default: ~/.config]
EOF
}

info() {
    printf '%s\n' "$*"
}

warn() {
    printf 'warning: %s\n' "$*" >&2
}

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

run() {
    "$@"
}

run_optional() {
    "$@" || warn "command failed: $*"
}

need_command() {
    command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

download() {
    url="$1"
    out="$2"
    curl -fsSL --retry 3 -o "$out" "$url"
}

render_user_service() {
    # ExecStart expands both % specifiers and $ variables after unquoting.
    EDGEPAD_SERVICE_CONFIG="$CONFIG_FILE" awk '
        function quote_argument(value, result, i, character) {
            result = "\""
            for (i = 1; i <= length(value); i++) {
                character = substr(value, i, 1)
                if (character == "\\" || character == "\"")
                    result = result "\\" character
                else if (character == "%" || character == "$")
                    result = result character character
                else if (character == "\n")
                    result = result "\\n"
                else if (character == "\r")
                    result = result "\\r"
                else if (character == "\t")
                    result = result "\\t"
                else
                    result = result character
            }
            return result "\""
        }
        /^ExecStart=/ {
            print "ExecStart=%h/.local/bin/edgepad daemon --config " quote_argument(ENVIRON["EDGEPAD_SERVICE_CONFIG"])
            found = 1
            next
        }
        { print }
        END {
            if (!found) {
                print "error: downloaded edgepad.service has no ExecStart" > "/dev/stderr"
                exit 1
            }
        }
    ' "$1"
}

sudo_run() {
    sudo "$@"
}

sudo_run_optional() {
    sudo "$@" || warn "command failed: sudo $*"
}

remove_file_if_present() {
    path="$1"
    if [ -e "$path" ] || [ -L "$path" ]; then
        rm -f "$path"
        info "Removed: ${path}"
    else
        info "Not present: ${path}"
    fi
}

sudo_remove_file_if_present() {
    path="$1"
    if [ -e "$path" ] || [ -L "$path" ]; then
        sudo_run rm -f "$path"
        info "Removed: ${path}"
        return 0
    fi

    info "Not present: ${path}"
    return 1
}

action_available() {
    (PATH="$SERVICE_PATH" command -v "$1" >/dev/null 2>&1)
}

write_starter_config() {
    action_available pamixer && STARTER_VOLUME=1
    action_available brightnessctl && STARTER_BRIGHTNESS=1
    if [ "$STARTER_VOLUME" -eq 0 ] && [ "$STARTER_BRIGHTNESS" -eq 0 ]; then
        if action_available notify-send; then
            STARTER_NOTIFICATIONS=1
        else
            die "No starter tools found (pamixer, brightnessctl or notify-send). Binary and reference installed; configure an action in ${CONFIG_FILE}, then rerun the installer. Service was not started."
        fi
    fi
    {
        printf 'device = "auto"\n'
        if [ "$STARTER_VOLUME" -eq 1 ]; then
            cat <<'EOF'

[[sliders]]
zone = "left"
up = ["pamixer", "-i", "3"]
down = ["pamixer", "-d", "3"]

[[gestures]]
zone = "left"
direction = "tap"
action = ["pamixer", "-t"]
EOF
        fi
        if [ "$STARTER_BRIGHTNESS" -eq 1 ]; then
            cat <<'EOF'

[[sliders]]
zone = "right"
up = ["brightnessctl", "--class=backlight", "set", "+5%"]
down = ["brightnessctl", "--class=backlight", "set", "5%-"]
EOF
        fi
        if [ "$STARTER_NOTIFICATIONS" -eq 1 ]; then
            cat <<'EOF'

[[sliders]]
zone = "left"
up = ["notify-send", "edgepad", "Slide up"]
down = ["notify-send", "edgepad", "Slide down"]

[[gestures]]
zone = "left"
direction = "tap"
action = ["notify-send", "edgepad", "Tap"]
EOF
        fi
    } > "$CONFIG_FILE"
    NEW_CONFIG=1
}

print_install_finish() {
    info ""
    info "Edgepad is running."
    if [ "$NEW_CONFIG" -eq 1 ]; then
        info ""
        if [ "$STARTER_VOLUME" -eq 1 ]; then
            info "Left edge: slide up/down for volume; tap to toggle mute."
        else
            info "Volume starter omitted: pamixer not found."
        fi
        if [ "$STARTER_BRIGHTNESS" -eq 1 ]; then
            info "Right edge: slide up/down for backlight brightness."
        else
            info "Brightness starter omitted: brightnessctl not found."
        fi
        if [ "$STARTER_NOTIFICATIONS" -eq 1 ]; then
            info "Left edge: slide or tap to show test notifications."
        fi
    else
        info "Existing configuration preserved. Inspect it with: ${BIN_DIR}/edgepad status"
    fi
    info ""
    info "Config     ${CONFIG_FILE}"
    info "Reference  ${REFERENCE_FILE}"
    info "Reload     systemctl --user reload edgepad.service"
    info "Stop       systemctl --user stop edgepad.service"
    case ":${PATH:-}:" in
        *":${BIN_DIR}:"*) ;;
        *) info ""; info "${BIN_DIR} is not on your PATH. For now, run: ${BIN_DIR}/edgepad status" ;;
    esac
}

print_install_dry_run_plan() {
    info "Dry run: edgepad install preview"
    info ""
    info "Release source:"
    info "  repository: ${REPO}"
    info "  version: ${VERSION}"
    info "  target: ${target}"
    info "  base URL: ${release_base}"
    info ""
    info "Would download release assets:"
    info "  ${asset} -> ${tmp_dir}/${asset}"
    info "  70-edgepad.rules -> ${tmp_dir}/70-edgepad.rules"
    info "  edgepad.service -> ${tmp_dir}/edgepad.service"
    info "  edgepad.toml.example -> ${tmp_dir}/edgepad.toml.example"
    info "  checksums -> ${tmp_dir}/checksums"
    info ""
    info "Would run checksum verification:"
    info "  ${tmp_dir}/checksums"
    info ""
    info "Would copy binary to:"
    info "  ${tmp_dir}/${asset} -> ${BIN_DIR}/edgepad"
    info ""
    if [ -e "$CONFIG_FILE" ]; then
        info "Would keep existing config:"
        info "  ${CONFIG_FILE}"
    else
        info "Would generate ${CONFIG_FILE} using available service-PATH tools:"
        info "  pamixer: left volume slider and mute tap"
        info "  brightnessctl: right brightness slider"
        info "  neither: left notification gestures if notify-send is available"
        info "  none: leave service unstarted and request a custom configuration"
    fi
    info ""
    info "Would install the reference example if absent: ${REFERENCE_FILE}"
    info "Would check configuration before changing the service:"
    info "  ${BIN_DIR}/edgepad doctor --config-only --config ${CONFIG_FILE}"
    info "  Missing action tools warn; invalid configuration stops installation."
    info ""
    info "Would copy udev rule with sudo:"
    info "  ${tmp_dir}/70-edgepad.rules -> ${UDEV_RULE_FILE}"
    info "Would run:"
    info "  sudo udevadm control --reload"
    info "  sudo udevadm trigger --subsystem-match=input --action=change"
    info "  sudo udevadm trigger --subsystem-match=misc --action=change"
    info ""
    info "Would configure the systemd user service to read:"
    info "  ${CONFIG_FILE}"
    info "Would install systemd user service to:"
    info "  ${SERVICE_FILE}"
    info "Would run:"
    info "  systemctl --user daemon-reload"
    info "  systemctl --user enable ${SERVICE_FILE}"
    info "  systemctl --user restart edgepad.service"
    info "  systemctl --user is-active --quiet edgepad.service"
    info ""
    info "Would run:"
    info "  ${BIN_DIR}/edgepad doctor --config ${CONFIG_FILE}"
    info ""
    info "Dry run complete; no files were downloaded or changed."
}

print_uninstall_dry_run_plan() {
    info "Dry run: edgepad uninstall preview"
    info ""
    info "Would run:"
    info "  systemctl --user disable --now edgepad.service"
    info ""
    info "Would remove user service:"
    info "  ${SERVICE_FILE}"
    info "Would run:"
    info "  systemctl --user daemon-reload"
    info ""
    info "Would remove binary:"
    info "  ${BIN_DIR}/edgepad"
    info ""
    info "Would remove udev rule with sudo:"
    info "  ${UDEV_RULE_FILE}"
    info "Would run if udev rule is removed:"
    info "  sudo udevadm control --reload"
    info "  sudo udevadm trigger --subsystem-match=input --action=change"
    info "  sudo udevadm trigger --subsystem-match=misc --action=change"
    info ""
    if [ "$PURGE" -eq 1 ]; then
        info "Would remove config directory:"
        info "  ${CONFIG_DIR}"
    else
        info "Would keep config:"
        info "  ${CONFIG_FILE}"
        info "Use --uninstall --purge to remove ${CONFIG_DIR}."
    fi
    info ""
    info "Dry run complete; no files were changed."
}

# Select one release before parsing its installer options or changing the system.
# Local help and uninstall remain available without a network connection.
dispatch_release=1
for arg in "$@"; do
    case "$arg" in -h|--help|--uninstall) dispatch_release=0 ;; esac
done
if [ "$dispatch_release" -eq 1 ]; then
    if [ "$VERSION" = latest ]; then
        need_command curl
        release_url=$(curl -fsSLI --retry 3 -o /dev/null -w '%{url_effective}' "https://github.com/${REPO}/releases/latest")
        case "$release_url" in
            "https://github.com/${REPO}/releases/tag/"*) VERSION=${release_url##*/} ;;
            *) die "could not resolve latest release tag" ;;
        esac
    fi
    case "$VERSION" in
        ""|*[!a-zA-Z0-9._-]*) die "invalid release tag: ${VERSION}" ;;
    esac
    if [ "$VERSION" != "$INSTALLER_VERSION" ]; then
        need_command curl
        handoff_dir=$(mktemp -d)
        trap 'rm -rf -- "$handoff_dir"' EXIT HUP INT TERM
        download "https://raw.githubusercontent.com/${REPO}/${VERSION}/install.sh" "$handoff_dir/install.sh"
        info "Using the installer from ${VERSION}."
        EDGEPAD_VERSION="$VERSION" sh "$handoff_dir/install.sh" "$@"
        exit $?
    fi
fi

for arg in "$@"; do
    case "$arg" in
        --dry-run)
            DRY_RUN=1
            ;;
        --uninstall)
            MODE="uninstall"
            ;;
        --purge)
            PURGE=1
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            usage >&2
            die "unknown option: $arg"
            ;;
    esac
done

if [ "$PURGE" -eq 1 ] && [ "$MODE" != "uninstall" ]; then
    die "--purge requires --uninstall"
fi

if [ "$(id -u)" -eq 0 ]; then
    die "do not run install.sh as root; it installs a user service and uses sudo only for udev rules"
fi

if [ "$MODE" = "uninstall" ]; then
    if [ "$DRY_RUN" -eq 1 ]; then
        print_uninstall_dry_run_plan
        exit 0
    fi

    need_command sudo
    need_command systemctl
    need_command udevadm

    info "Uninstalling edgepad"
    run_optional systemctl --user disable --now edgepad.service
    remove_file_if_present "$SERVICE_FILE"
    run_optional systemctl --user daemon-reload
    remove_file_if_present "${BIN_DIR}/edgepad"
    if sudo_remove_file_if_present "$UDEV_RULE_FILE"; then
        sudo_run udevadm control --reload
        sudo_run_optional udevadm trigger --subsystem-match=input --action=change
        sudo_run_optional udevadm trigger --subsystem-match=misc --action=change
    fi

    if [ "$PURGE" -eq 1 ]; then
        if [ -e "$CONFIG_DIR" ] || [ -L "$CONFIG_DIR" ]; then
            rm -rf "$CONFIG_DIR"
            info "Removed: ${CONFIG_DIR}"
        else
            info "Not present: ${CONFIG_DIR}"
        fi
    else
        info "Keeping config: ${CONFIG_FILE}"
    fi

    info "edgepad uninstalled"
    exit 0
fi

kernel="$(uname -s)"
machine="$(uname -m)"
case "${kernel}-${machine}" in
    Linux-x86_64|Linux-amd64)
        target="x86_64-unknown-linux-musl"
        ;;
    *)
        die "unsupported platform: ${kernel}-${machine}; the release installer ships x86_64 Linux only"
        ;;
esac

asset="edgepad-${target}"
release_base="https://github.com/${REPO}/releases/download/${VERSION}"

if [ "$DRY_RUN" -eq 0 ]; then
    need_command curl
    need_command install
    need_command sha256sum
    need_command awk
    need_command sudo
    need_command systemctl
    need_command udevadm
fi

tmp_dir="${TMPDIR:-/tmp}/edgepad-install-dry-run"
if [ "$DRY_RUN" -eq 0 ]; then
    tmp_dir="$(mktemp -d)"
fi
cleanup() {
    if [ "$DRY_RUN" -eq 0 ] && [ -n "${tmp_dir:-}" ]; then
        rm -rf "$tmp_dir"
    fi
}
trap cleanup EXIT HUP INT TERM

if [ "$DRY_RUN" -eq 1 ]; then
    print_install_dry_run_plan
    exit 0
fi

info "Installing edgepad from ${release_base}"
download "${release_base}/${asset}" "${tmp_dir}/${asset}"
download "${release_base}/70-edgepad.rules" "${tmp_dir}/70-edgepad.rules"
download "${release_base}/edgepad.service" "${tmp_dir}/edgepad.service"
download "${release_base}/edgepad.toml.example" "${tmp_dir}/edgepad.toml.example"
download "${release_base}/checksums" "${tmp_dir}/checksums"

(cd "$tmp_dir" && sha256sum -c checksums)
# Check action availability in the PATH supplied by this release's service,
# not the invoking shell's PATH. Expand the service's home specifier literally.
SERVICE_PATH=$(EDGEPAD_INSTALL_HOME="$HOME" awk '
    /^Environment=PATH=/ {
        count++
        n = split(substr($0, 18), parts, /%h/)
        printf "%s", parts[1]
        for (i = 2; i <= n; i++) printf "%s%s", ENVIRON["EDGEPAD_INSTALL_HOME"], parts[i]
    }
    END { if (count != 1) exit 1 }
' "${tmp_dir}/edgepad.service") || die "release service must define one action PATH"
render_user_service "${tmp_dir}/edgepad.service" > "${tmp_dir}/edgepad.service.configured"

run install -Dm0755 "${tmp_dir}/${asset}" "${BIN_DIR}/edgepad"
if [ ! -e "$REFERENCE_FILE" ]; then
    run install -Dm0644 "${tmp_dir}/edgepad.toml.example" "$REFERENCE_FILE"
fi
if [ -e "$CONFIG_FILE" ]; then
    info "Keeping existing config: ${CONFIG_FILE}"
else
    write_starter_config
fi

check_result=0
PATH="$SERVICE_PATH" "${BIN_DIR}/edgepad" doctor --config-only --config "$CONFIG_FILE" || check_result=$?
case "$check_result" in
    0) ;;
    1) warn "Some configured action tools are unavailable; keeping your bindings and continuing." ;;
    *) die "Configuration check failed. Fix ${CONFIG_FILE} and rerun; service was not restarted." ;;
esac

sudo_run install -Dm0644 "${tmp_dir}/70-edgepad.rules" "$UDEV_RULE_FILE"
sudo_run udevadm control --reload
sudo_run_optional udevadm trigger --subsystem-match=input --action=change
sudo_run_optional udevadm trigger --subsystem-match=misc --action=change
run install -Dm0644 "${tmp_dir}/edgepad.service.configured" "$SERVICE_FILE"
run systemctl --user daemon-reload
run systemctl --user enable "$SERVICE_FILE"
if ! systemctl --user restart edgepad.service || ! systemctl --user is-active --quiet edgepad.service; then
    PATH="$SERVICE_PATH" "${BIN_DIR}/edgepad" doctor --config "$CONFIG_FILE" || :
    die "Edgepad could not start. See the diagnostics above and journalctl --user -u edgepad.service -b."
fi
# Missing optional action tools must not turn a running installation into a failure.
PATH="$SERVICE_PATH" "${BIN_DIR}/edgepad" doctor --config "$CONFIG_FILE" || warn "Doctor reported issues; review the diagnostics above."
print_install_finish
