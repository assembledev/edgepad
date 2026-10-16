use std::process::Command;

fn edgepad() -> Command {
    Command::new(env!("CARGO_BIN_EXE_edgepad"))
}

#[test]
fn root_help_flags_print_help() {
    for flag in ["--help", "-h"] {
        let output = edgepad()
            .arg(flag)
            .output()
            .expect("edgepad binary should run");

        assert!(
            output.status.success(),
            "{flag} should succeed, stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "{flag} should not print stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Usage:"), "stdout was: {stdout}");
        assert!(stdout.contains("edgepad <COMMAND>"), "stdout was: {stdout}");
        assert!(stdout.contains("--version"), "stdout was: {stdout}");
    }
}

#[test]
fn root_version_flags_print_package_version() {
    for flag in ["--version", "-V"] {
        let output = edgepad()
            .arg(flag)
            .output()
            .expect("edgepad binary should run");

        assert!(
            output.status.success(),
            "{flag} should succeed, stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stderr.is_empty(),
            "{flag} should not print stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!("edgepad {}\n", env!("CARGO_PKG_VERSION"))
        );
    }
}

#[test]
fn subcommand_help_flags_print_command_help() {
    for (command, options) in [
        ("devices", vec!["--input-root", "--root", "--all"]),
        (
            "status",
            vec!["--config", "--device", "--input-root", "--service"],
        ),
        (
            "doctor",
            vec![
                "--config",
                "--device",
                "--input-root",
                "--uinput",
                "--service",
            ],
        ),
        (
            "daemon",
            vec!["--config", "--device", "--input-root", "--edge-width"],
        ),
        (
            "dump",
            vec!["--device", "--out", "--input-root", "--frames", "--raw"],
        ),
        (
            "proxy",
            vec![
                "--frames",
                "--dry-run",
                "--uinput",
                "--grab",
                "--config",
                "--built-in-defaults",
                "--device",
                "--input-root",
                "--edge-width",
            ],
        ),
        ("replay", vec!["--config", "--built-in-defaults"]),
        ("replay-raw", vec!["--config", "--built-in-defaults"]),
    ] {
        for flag in ["--help", "-h"] {
            let output = edgepad()
                .arg(command)
                .arg(flag)
                .output()
                .expect("edgepad binary should run");

            assert!(
                output.status.success(),
                "{command} {flag} should succeed, stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                output.stderr.is_empty(),
                "{command} {flag} should not print stderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(stdout.contains("Usage:"), "stdout was: {stdout}");
            assert!(
                stdout.contains(&format!("Usage: edgepad {command}")),
                "stdout was: {stdout}"
            );
            for option in &options {
                assert!(stdout.contains(option), "missing {option}: {stdout}");
            }
        }
    }
}

#[test]
fn dump_help_explains_running_daemon_limit() {
    let output = edgepad()
        .arg("dump")
        .arg("--help")
        .output()
        .expect("edgepad binary should run");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("must not be grabbed by another process"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("Stop edgepad.service before capturing"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.contains("Capture at least N frame boundaries, then stop when idle"),
        "stdout was: {stdout}"
    );
}

#[test]
fn invalid_arguments_exit_with_two_before_execution() {
    for args in [
        vec![],
        vec!["unknown"],
        vec!["status", "--wat"],
        vec!["dump", "--device", "auto"],
        vec!["proxy", "--frames", "2", "--uinput"],
        vec!["proxy", "--frames", "0", "--dry-run"],
    ] {
        let output = edgepad().args(&args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("--help"), "{args:?}: {stderr}");
    }
}

#[test]
fn runtime_errors_keep_exit_code_one() {
    let missing = std::env::temp_dir().join(format!(
        "edgepad-cli-missing-capture-{}.ev",
        std::process::id()
    ));
    let output = edgepad()
        .arg("replay")
        .arg(&missing)
        .arg("--built-in-defaults")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to read"));
}

#[test]
fn daemon_help_bypasses_config_loading() {
    let missing = std::env::temp_dir().join(format!(
        "edgepad-help-missing-config-{}.toml",
        std::process::id()
    ));
    for flag in ["--help", "-h"] {
        let output = edgepad()
            .arg("daemon")
            .arg("--config")
            .arg(&missing)
            .arg(flag)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty());
        assert!(String::from_utf8_lossy(&output.stdout).contains("SIGHUP"));
    }
}
