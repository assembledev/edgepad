use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn install_upgrade_restarts_daemon_and_checks_active_service() {
    check_install("edgepad-install-upgrade", None, true);
}

#[test]
fn install_uses_custom_xdg_config_path_in_service() {
    check_install(
        "edgepad-install-custom-xdg",
        Some(("xdg config", "xdg config")),
        false,
    );
}

#[test]
fn install_escapes_literal_config_path_for_systemd() {
    check_install(
        "edgepad-install-escaped-xdg",
        Some((
            "xdg %h ${HOME} \\ \"quoted\"\n\t",
            r#"xdg %%h $${HOME} \\ \"quoted\"\n\t"#,
        )),
        false,
    );
}

fn check_install(prefix: &str, custom_config: Option<(&str, &str)>, upgrade: bool) {
    let root = unique_temp_dir(prefix);
    let home = root.join("home");
    let (config_dir, escaped_config_dir) = custom_config.unwrap_or((".config", ".config"));
    let config_home = home.join(config_dir);
    let config_file = config_home.join("edgepad/edgepad.toml");
    let service_file = config_home.join("systemd/user/edgepad.service");
    let fake_bin = root.join("bin");
    let assets = root.join("assets");
    let log = root.join("commands.log");
    let udev_rule = root.join("etc/udev/rules.d/70-edgepad.rules");
    fs::create_dir_all(home.join(".local/bin")).expect("home bin should be created");
    fs::create_dir_all(config_home.join("edgepad")).expect("config dir should be created");
    fs::create_dir_all(config_home.join("systemd/user"))
        .expect("systemd user dir should be created");
    fs::create_dir_all(&fake_bin).expect("fake bin should be created");
    fs::create_dir_all(&assets).expect("assets should be created");

    write_executable(
        &fake_bin.join("uname"),
        "#!/bin/sh\ncase \"$1\" in -s) echo Linux ;; -m) echo x86_64 ;; esac\n",
    );
    write_executable(&fake_bin.join("id"), "#!/bin/sh\necho 1000\n");
    write_executable(
        &fake_bin.join("curl"),
        r##"#!/bin/sh
out=
url=
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o) out="$2"; shift 2 ;;
        *) url="$1"; shift ;;
    esac
done
name="${url##*/}"
cp "$EDGEPAD_TEST_ASSETS/$name" "$out"
"##,
    );
    write_executable(
        &fake_bin.join("sudo"),
        r#"#!/bin/sh
printf 'sudo %s\n' "$*" >> "$EDGEPAD_TEST_LOG"
case "$1" in
    install|rm) command="$1"; shift; exec "$command" "$@" ;;
    udevadm) exit 0 ;;
esac
exit 1
"#,
    );
    write_executable(
        &fake_bin.join("systemctl"),
        "#!/bin/sh\nprintf 'systemctl %s\\n' \"$*\" >> \"$EDGEPAD_TEST_LOG\"\n",
    );
    write_executable(&fake_bin.join("udevadm"), "#!/bin/sh\nexit 0\n");

    write_executable(
        &assets.join("edgepad-x86_64-unknown-linux-musl"),
        "#!/bin/sh\nprintf 'edgepad %s\\n' \"$*\" >> \"$EDGEPAD_TEST_LOG\"\n",
    );
    fs::write(assets.join("70-edgepad.rules"), "# test rule\n").expect("rule should be written");
    fs::write(
        assets.join("edgepad.service"),
        include_str!("../packaging/edgepad.service"),
    )
    .expect("service should be written");
    fs::write(assets.join("edgepad.toml.example"), "device = \"auto\"\n")
        .expect("config should be written");
    write_checksums(&assets);

    if upgrade {
        fs::write(home.join(".local/bin/edgepad"), "old binary\n")
            .expect("old binary should be present");
        fs::write(&config_file, "# existing user config\n")
            .expect("existing config should be present");
        fs::write(&service_file, "# old service\n").expect("old service should be present");
    }

    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let installer = || {
        let mut command = Command::new("sh");
        command
            .arg("install.sh")
            .env("HOME", &home)
            .env_remove("XDG_CONFIG_HOME")
            .env("PATH", &path)
            .env("EDGEPAD_TEST_ASSETS", &assets)
            .env("EDGEPAD_TEST_LOG", &log)
            .env("EDGEPAD_UDEV_RULE_FILE", &udev_rule);
        if custom_config.is_some() {
            command.env("XDG_CONFIG_HOME", &config_home);
        }
        command
    };

    let preview = installer()
        .arg("--dry-run")
        .output()
        .expect("installer preview should run");
    assert!(preview.status.success());
    let plan = String::from_utf8_lossy(&preview.stdout);
    assert!(
        plan.contains(&format!(
            "Would configure the systemd user service to read:\n  {}",
            config_file.display()
        )),
        "preview should show the service config path; plan: {plan}"
    );
    assert!(!log.exists(), "preview should not invoke system commands");
    assert_eq!(config_file.exists(), upgrade);
    assert_eq!(service_file.exists(), upgrade);

    let output = installer().output().expect("installer should run");

    assert!(
        output.status.success(),
        "installer failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let commands = fs::read_to_string(&log).expect("command log should be readable");
    assert_ordered(
        &commands,
        &[
            "systemctl --user daemon-reload",
            "systemctl --user enable edgepad.service",
            "systemctl --user restart edgepad.service",
            "systemctl --user is-active --quiet edgepad.service",
            "edgepad doctor",
        ],
    );
    assert!(home.join(".local/bin/edgepad").is_file());
    let service = fs::read_to_string(&service_file).expect("installed service should be readable");
    let expected_exec_start = format!(
        "ExecStart=%h/.local/bin/edgepad daemon --config \"{}/{escaped_config_dir}/edgepad/edgepad.toml\"",
        home.display()
    );
    assert!(
        service.lines().any(|line| line == expected_exec_start),
        "installed service should use the resolved config path; service: {service}"
    );
    let without_exec_start = |contents: &str| {
        contents
            .lines()
            .filter(|line| !line.starts_with("ExecStart="))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        without_exec_start(&service),
        without_exec_start(include_str!("../packaging/edgepad.service")),
        "other service settings should remain intact"
    );
    assert_eq!(
        fs::read_to_string(&config_file).expect("installed config should be readable"),
        if upgrade {
            "# existing user config\n"
        } else {
            "device = \"auto\"\n"
        }
    );
    assert!(
        commands.contains(&format!(
            "edgepad doctor --config {}",
            config_file.display()
        )),
        "doctor should check the installed config; commands: {commands}"
    );
    assert!(udev_rule.is_file());

    fs::remove_dir_all(root).expect("temp tree should be removed");
}

fn write_checksums(assets: &Path) {
    let output = Command::new("sha256sum")
        .current_dir(assets)
        .args([
            "edgepad-x86_64-unknown-linux-musl",
            "70-edgepad.rules",
            "edgepad.service",
            "edgepad.toml.example",
        ])
        .output()
        .expect("sha256sum should run");
    assert!(output.status.success());
    fs::write(assets.join("checksums"), output.stdout).expect("checksums should be written");
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("script should be written");
    let mut permissions = fs::metadata(path)
        .expect("script metadata should be readable")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("script should be executable");
}

fn assert_ordered(haystack: &str, needles: &[&str]) {
    let mut offset = 0;
    for needle in needles {
        let relative = haystack[offset..]
            .find(needle)
            .unwrap_or_else(|| panic!("missing {needle:?} in command log:\n{haystack}"));
        offset += relative + needle.len();
    }
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}-{}", std::process::id()))
}
