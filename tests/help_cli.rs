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
        assert!(
            stdout.contains("edgepad <command> [options]"),
            "stdout was: {stdout}"
        );
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
    for (command, usage) in [
        (
            "devices",
            "edgepad devices [--input-root <input-root>] [--all]",
        ),
        (
            "status",
            "edgepad status [--config <file>] [--device auto|<event-node>] [--input-root <input-root>]",
        ),
        (
            "doctor",
            "edgepad doctor [--config <file>] [--device auto|<event-node>] [--input-root <input-root>]",
        ),
        (
            "daemon",
            "edgepad daemon [--config <file>] [--device auto|<event-node>]",
        ),
        (
            "dump",
            "edgepad dump --device auto|<event-node> --out <file.ev> [--input-root <input-root>] [--frames N] [--raw]",
        ),
        (
            "proxy",
            "edgepad proxy --frames N (--dry-run | --uinput --grab)",
        ),
        (
            "replay",
            "edgepad replay <fixture.ev> [--config <file> | --built-in-defaults]",
        ),
        (
            "replay-raw",
            "edgepad replay-raw <raw.ev> [--config <file> | --built-in-defaults]",
        ),
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
            assert!(stdout.contains(usage), "stdout was: {stdout}");
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
