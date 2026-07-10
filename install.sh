#!/bin/sh
set -eu

REPO="${EDGEPAD_REPO:-assembledev/edgepad}"
VERSION="${EDGEPAD_VERSION:-latest}"
BIN_DIR="${HOME}/.local/bin"
CONFIG_HOME="${XDG_CONFIG_HOME:-${HOME}/.config}"
CONFIG_DIR="${CONFIG_HOME}/edgepad"
CONFIG_FILE="${CONFIG_DIR}/edgepad.toml"
SYSTEMD_USER_DIR="${CONFIG_HOME}/systemd/user"
SERVICE_FILE="${SYSTEMD_USER_DIR}/edgepad.service"
UDEV_RULE_FILE="/etc/udev/rules.d/70-edgepad.rules"
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
    info "Would copy udev rule with sudo:"
    info "  ${tmp_dir}/70-edgepad.rules -> ${UDEV_RULE_FILE}"
    info "Would run:"
    info "  sudo udevadm control --reload"
    info "  sudo udevadm trigger --subsystem-match=input --action=change"
    info "  sudo udevadm trigger --subsystem-match=misc --action=change"
    info ""
    if [ -e "$CONFIG_FILE" ]; then
        info "Would keep existing config:"
        info "  ${CONFIG_FILE}"
    else
        info "Would copy default config to:"
        info "  ${tmp_dir}/edgepad.toml.example -> ${CONFIG_FILE}"
    fi
    info ""
    info "Would copy systemd user service to:"
    info "  ${tmp_dir}/edgepad.service -> ${SERVICE_FILE}"
    info "Would run:"
    info "  systemctl --user daemon-reload"
    info "  systemctl --user enable --now edgepad.service"
    info ""
    info "Would run:"
    info "  ${BIN_DIR}/edgepad doctor"
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
        target="x86_64-unknown-linux-gnu"
        ;;
    *)
        die "unsupported platform: ${kernel}-${machine}; the release installer ships x86_64 Linux only"
        ;;
esac

asset="edgepad-${target}"
if [ "$VERSION" = "latest" ]; then
    release_base="https://github.com/${REPO}/releases/latest/download"
else
    release_base="https://github.com/${REPO}/releases/download/${VERSION}"
fi

if [ "$DRY_RUN" -eq 0 ]; then
    need_command curl
    need_command install
    need_command sha256sum
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

run install -Dm0755 "${tmp_dir}/${asset}" "${BIN_DIR}/edgepad"
sudo_run install -Dm0644 "${tmp_dir}/70-edgepad.rules" "$UDEV_RULE_FILE"
sudo_run udevadm control --reload
sudo_run_optional udevadm trigger --subsystem-match=input --action=change
sudo_run_optional udevadm trigger --subsystem-match=misc --action=change

if [ -e "$CONFIG_FILE" ]; then
    info "Keeping existing config: ${CONFIG_FILE}"
else
    run install -Dm0644 "${tmp_dir}/edgepad.toml.example" "$CONFIG_FILE"
fi

run install -Dm0644 "${tmp_dir}/edgepad.service" "$SERVICE_FILE"
run systemctl --user daemon-reload
run systemctl --user enable --now edgepad.service
run "${BIN_DIR}/edgepad" doctor

info "edgepad installed"
