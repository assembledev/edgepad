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
    check_install_case(
        prefix,
        custom_config,
        upgrade,
        InstallCase {
            tools: &["pamixer", "brightnessctl"],
            ..InstallCase::default()
        },
    );
}

#[derive(Default)]
struct InstallCase<'a> {
    tools: &'a [&'a str],
    preflight: i32,
    start_fails: bool,
    doctor_fails: bool,
    bin_on_path: bool,
}

#[test]
fn starter_adapts_to_available_tools() {
    for (name, tools) in [
        ("all", vec!["pamixer", "brightnessctl", "notify-send"]),
        ("volume", vec!["pamixer"]),
        ("brightness", vec!["brightnessctl"]),
        ("notifications", vec!["notify-send"]),
        ("none", vec![]),
    ] {
        check_install_case(
            &format!("edgepad-starter-{name}"),
            None,
            false,
            InstallCase {
                tools: &tools,
                ..InstallCase::default()
            },
        );
    }
}

#[test]
fn install_recognizes_binary_directory_already_on_path() {
    check_install_case(
        "edgepad-install-on-path",
        None,
        false,
        InstallCase {
            tools: &["pamixer", "brightnessctl"],
            bin_on_path: true,
            ..InstallCase::default()
        },
    );
}

#[test]
fn upgrade_keeps_missing_actions_and_continues() {
    check_install_case(
        "edgepad-missing-upgrade",
        None,
        true,
        InstallCase {
            preflight: 1,
            doctor_fails: true,
            ..InstallCase::default()
        },
    );
}

#[test]
fn invalid_config_does_not_restart_or_change_access() {
    check_install_case(
        "edgepad-invalid-upgrade",
        None,
        true,
        InstallCase {
            preflight: 2,
            ..InstallCase::default()
        },
    );
}

#[test]
fn startup_failure_runs_diagnostics() {
    check_install_case(
        "edgepad-start-failure",
        None,
        true,
        InstallCase {
            start_fails: true,
            ..InstallCase::default()
        },
    );
}

fn check_install_case(
    prefix: &str,
    custom_config: Option<(&str, &str)>,
    upgrade: bool,
    case: InstallCase<'_>,
) {
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
        r#"#!/bin/sh
printf 'systemctl %s\n' "$*" >> "$EDGEPAD_TEST_LOG"
if [ "$2" = enable ] && [ "$3" != "$EDGEPAD_TEST_SERVICE_FILE" ]; then
    echo 'Unit edgepad.service does not exist in manager search path' >&2
    exit 1
fi
case "$*" in *restart*) exit "$EDGEPAD_TEST_START_RESULT" ;; esac
"#,
    );
    write_executable(&fake_bin.join("udevadm"), "#!/bin/sh\nexit 0\n");
    // Tools in the invoking shell must not affect service-PATH selection.
    for tool in ["pamixer", "brightnessctl", "notify-send"] {
        write_executable(&fake_bin.join(tool), "#!/bin/sh\nexit 99\n");
    }
    for tool in case.tools {
        write_executable(
            &home.join(".local/bin").join(tool),
            "#!/bin/sh\necho ACTION_EXECUTED >> \"$EDGEPAD_TEST_LOG\"\nexit 99\n",
        );
    }
    write_executable(
        &assets.join("edgepad-x86_64-unknown-linux-musl"),
        r#"#!/bin/sh
printf 'edgepad %s\n' "$*" >> "$EDGEPAD_TEST_LOG"
case "$*" in
    *--config-only*) exit "$EDGEPAD_TEST_PREFLIGHT" ;;
    *) exit "$EDGEPAD_TEST_DOCTOR_RESULT" ;;
esac
"#,
    );
    fs::write(assets.join("70-edgepad.rules"), "# test rule\n").expect("rule should be written");
    fs::write(
        assets.join("edgepad.service"),
        include_str!("../packaging/edgepad.service").replace(
            "Environment=PATH=%h/.local/bin:/usr/local/bin:/usr/bin:/bin",
            "Environment=PATH=%h/.local/bin",
        ),
    )
    .expect("service should be written");
    fs::write(assets.join("edgepad.toml.example"), "device = \"auto\"\n")
        .expect("config should be written");
    write_checksums(&assets);

    if upgrade {
        fs::write(
            config_home.join("edgepad/edgepad.toml.example"),
            "# kept reference\n",
        )
        .unwrap();
        fs::write(home.join(".local/bin/edgepad"), "old binary\n")
            .expect("old binary should be present");
        fs::write(&config_file, "# existing user config\n")
            .expect("existing config should be present");
        fs::write(&service_file, "# old service\n").expect("old service should be present");
    }

    let mut path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    if case.bin_on_path {
        path.push_str(&format!(":{}", home.join(".local/bin").display()));
    }
    let installer = || {
        let mut command = Command::new("sh");
        command
            .arg("install.sh")
            .env("HOME", &home)
            .env_remove("XDG_CONFIG_HOME")
            .env("PATH", &path)
            .env("EDGEPAD_TEST_ASSETS", &assets)
            .env("EDGEPAD_TEST_LOG", &log)
            .env("EDGEPAD_TEST_SERVICE_FILE", &service_file)
            .env("EDGEPAD_TEST_PREFLIGHT", case.preflight.to_string())
            .env(
                "EDGEPAD_TEST_START_RESULT",
                if case.start_fails { "1" } else { "0" },
            )
            .env(
                "EDGEPAD_TEST_DOCTOR_RESULT",
                if case.doctor_fails { "2" } else { "0" },
            )
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

    let no_starter = !upgrade && case.tools.is_empty();
    let fails = no_starter || case.preflight > 1 || case.start_fails;
    assert_eq!(
        output.status.success(),
        !fails,
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let commands = fs::read_to_string(&log).unwrap_or_default();
    assert!(!commands.contains("ACTION_EXECUTED"));
    assert_eq!(
        fs::read_to_string(config_home.join("edgepad/edgepad.toml.example")).unwrap(),
        if upgrade {
            "# kept reference\n"
        } else {
            "device = \"auto\"\n"
        }
    );
    if no_starter || case.preflight > 1 {
        assert!(!commands.contains("systemctl"));
        assert!(!commands.contains("sudo"));
        assert!(!udev_rule.exists());
        if upgrade {
            assert_eq!(
                fs::read_to_string(&config_file).unwrap(),
                "# existing user config\n"
            );
        }
        fs::remove_dir_all(root).unwrap();
        return;
    }
    if case.start_fails {
        assert_ordered(
            &commands,
            &["systemctl --user restart", "edgepad doctor --config "],
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("Edgepad is running."));
        fs::remove_dir_all(root).unwrap();
        return;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Edgepad is running."));
    assert_eq!(stdout.contains("is not on your PATH"), !case.bin_on_path);
    if upgrade {
        assert!(!stdout.contains("Left edge:"));
    }
    assert_ordered(
        &commands,
        &[
            "edgepad doctor --config-only",
            "sudo install",
            "systemctl --user daemon-reload",
            &format!("systemctl --user enable {}", service_file.display()),
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
        without_exec_start(&fs::read_to_string(assets.join("edgepad.service")).unwrap()),
        "other service settings should remain intact"
    );
    let installed = fs::read_to_string(&config_file).unwrap();
    if upgrade {
        assert_eq!(installed, "# existing user config\n");
    } else {
        let config =
            edgepad::config::EdgepadConfig::parse(&installed).expect("generated config parses");
        let has_volume = case.tools.contains(&"pamixer");
        let has_brightness = case.tools.contains(&"brightnessctl");
        let notifications = !has_volume && !has_brightness;
        assert_eq!(
            config.sliders.len(),
            if has_volume && has_brightness { 2 } else { 1 }
        );
        assert_eq!(installed.contains("pamixer"), has_volume);
        assert_eq!(installed.contains("brightnessctl"), has_brightness);
        assert_eq!(installed.contains("notify-send"), notifications);
        assert_eq!(
            stdout.contains("Left edge: slide up/down for volume"),
            has_volume
        );
        assert_eq!(
            stdout.contains("Right edge: slide up/down for backlight brightness"),
            has_brightness
        );
        assert_eq!(
            stdout.contains("Left edge: slide or tap to show test notifications"),
            notifications
        );
        assert!(!installed.contains("tap_min_duration_ms"));
    }
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
