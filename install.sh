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

usage() {
    cat <<'EOF'
usage: install.sh [--dry-run]

Installs edgepad for the current user from GitHub Releases.

Environment:
  EDGEPAD_REPO=owner/repo      GitHub repository [default: assembledev/edgepad]
  EDGEPAD_VERSION=v0.1.0      Release tag to install [default: latest]
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

print_dry_run_plan() {
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

for arg in "$@"; do
    case "$arg" in
        --dry-run)
            DRY_RUN=1
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

if [ "$(id -u)" -eq 0 ]; then
    die "do not run install.sh as root; it installs a user service and uses sudo only for udev rules"
fi

kernel="$(uname -s)"
machine="$(uname -m)"
case "${kernel}-${machine}" in
    Linux-x86_64|Linux-amd64)
        target="x86_64-unknown-linux-gnu"
        ;;
    *)
        die "unsupported platform: ${kernel}-${machine}; edgepad 0.1.0 release installer ships x86_64 Linux only"
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
    print_dry_run_plan
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
