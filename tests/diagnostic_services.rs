use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
const UNIT: &str = "custom-edgepad.service";

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "edgepad-diagnostic-services-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("bin")).unwrap();
        fs::create_dir(root.join("input")).unwrap();
        let systemctl = root.join("bin/systemctl");
        fs::write(
            &systemctl,
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$EDGEPAD_TEST_SYSTEMCTL_LOG"
case "$1 $2" in
    "--user show-environment")
        printf '%s' "$EDGEPAD_TEST_MANAGER_ERROR" >&2
        exit "$EDGEPAD_TEST_MANAGER_EXIT"
        ;;
    "--user show")
        printf '%s' "$EDGEPAD_TEST_PROPERTIES"
        printf '%s' "$EDGEPAD_TEST_QUERY_ERROR" >&2
        exit "$EDGEPAD_TEST_QUERY_EXIT"
        ;;
    *) exit 99 ;;
esac
"#,
        )
        .unwrap();
        fs::set_permissions(systemctl, fs::Permissions::from_mode(0o755)).unwrap();
        Self { root }
    }

    fn command(&self, subcommand: &str) -> Command {
        fs::write(self.root.join("systemctl.log"), "").unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_edgepad"));
        command
            .env("PATH", self.root.join("bin"))
            .env("USER", "edgepad-test")
            .env(
                "EDGEPAD_TEST_SYSTEMCTL_LOG",
                self.root.join("systemctl.log"),
            )
            .env("EDGEPAD_TEST_PROPERTIES", "")
            .env("EDGEPAD_TEST_QUERY_ERROR", "")
            .env("EDGEPAD_TEST_QUERY_EXIT", "0")
            .env("EDGEPAD_TEST_MANAGER_ERROR", "")
            .env("EDGEPAD_TEST_MANAGER_EXIT", "0")
            .arg(subcommand)
            .arg("--config")
            .arg(self.root.join("missing.toml"))
            .arg("--input-root")
            .arg(self.root.join("input"))
            .arg("--service")
            .arg(UNIT);
        if subcommand == "doctor" {
            command
                .arg("--uinput")
                .arg(self.root.join("missing-uinput"));
        }
        command
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.root.join("systemctl.log"))
            .unwrap()
            .lines()
            .map(ToString::to_string)
            .collect()
    }

    fn assert_service_query(&self, subcommand: &str) {
        let query = format!(
            "--user show {UNIT} -p LoadState -p ActiveState -p SubState -p MainPID -p StatusText"
        );
        let expected = if subcommand == "doctor" {
            vec!["--user show-environment".to_string(), query]
        } else {
            vec![query]
        };
        assert_eq!(self.calls(), expected);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn stdout(output: &Output) -> String {
    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn doctor_unit_line(stdout: &str) -> &str {
    stdout
        .lines()
        .find(|line| line.split_whitespace().nth(1) == Some("unit"))
        .expect("doctor should report the unit")
}

#[test]
fn both_commands_query_the_same_service_fields_and_report_each_state() {
    let fixture = Fixture::new();
    for (load, active, sub, doctor_severity, doctor_detail, status_detail) in [
        (
            "loaded",
            "active",
            "running",
            "ok",
            "is active (running)",
            "active (running), pid 1234, waiting=seat",
        ),
        (
            "loaded",
            "activating",
            "start",
            "warn",
            "is loaded=loaded active=activating sub=start",
            "activating (start), pid 1234, waiting=seat",
        ),
        (
            "loaded",
            "inactive",
            "dead",
            "fail",
            "is inactive (dead)",
            "inactive (dead), pid 1234, waiting=seat",
        ),
        (
            "loaded",
            "failed",
            "failed",
            "fail",
            "is failed (failed)",
            "failed (failed), pid 1234, waiting=seat",
        ),
        (
            "not-found",
            "inactive",
            "dead",
            "fail",
            "is not installed in the user manager",
            "custom-edgepad.service is not installed",
        ),
    ] {
        let properties = format!(
            "LoadState={load}\nActiveState={active}\nSubState={sub}\nMainPID=1234\nStatusText=waiting=seat\n"
        );
        for subcommand in ["doctor", "status"] {
            let output = fixture
                .command(subcommand)
                .env("EDGEPAD_TEST_PROPERTIES", &properties)
                .output()
                .unwrap();
            let stdout = stdout(&output);
            if subcommand == "doctor" {
                let line = doctor_unit_line(&stdout);
                assert_eq!(
                    line.split_whitespace().next(),
                    Some(doctor_severity),
                    "{stdout}"
                );
                assert!(
                    line.ends_with(&format!("{UNIT} {doctor_detail}")),
                    "{stdout}"
                );
            } else {
                assert!(
                    stdout
                        .lines()
                        .any(|line| line == format!("Service  {status_detail}")),
                    "{stdout}"
                );
            }
            fixture.assert_service_query(subcommand);
        }
    }
}

#[test]
fn failed_queries_use_stderr_with_stdout_as_a_fallback() {
    let fixture = Fixture::new();
    for (query_stdout, query_stderr, expected) in [
        (
            "ActiveState=active\n",
            " bus unavailable\n",
            "bus unavailable",
        ),
        (" query failed\n", "", "query failed"),
    ] {
        for subcommand in ["doctor", "status"] {
            let output = fixture
                .command(subcommand)
                .env("EDGEPAD_TEST_PROPERTIES", query_stdout)
                .env("EDGEPAD_TEST_QUERY_ERROR", query_stderr)
                .env("EDGEPAD_TEST_QUERY_EXIT", "1")
                .output()
                .unwrap();
            let stdout = stdout(&output);
            if subcommand == "doctor" {
                let line = doctor_unit_line(&stdout);
                assert_eq!(line.split_whitespace().next(), Some("warn"));
                assert!(
                    line.ends_with(&format!("could not inspect {UNIT}: {expected}")),
                    "{stdout}"
                );
            } else {
                assert!(
                    stdout.contains(&format!(
                        "Service  unknown: systemctl --user failed: {expected}"
                    )),
                    "{stdout}"
                );
            }
            fixture.assert_service_query(subcommand);
        }
    }
}

#[test]
fn doctor_skips_the_unit_query_when_the_user_manager_is_unavailable() {
    let fixture = Fixture::new();
    let output = fixture
        .command("doctor")
        .env("EDGEPAD_TEST_MANAGER_ERROR", "manager unavailable")
        .env("EDGEPAD_TEST_MANAGER_EXIT", "1")
        .output()
        .unwrap();
    let stdout = stdout(&output);
    assert!(
        stdout.contains("systemctl --user failed: manager unavailable"),
        "{stdout}"
    );
    assert!(
        doctor_unit_line(&stdout).ends_with("skipped because systemctl --user is not available"),
        "{stdout}"
    );
    assert_eq!(fixture.calls(), vec!["--user show-environment"]);
}

#[test]
fn missing_systemctl_is_reported_as_an_inspection_error() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.root.join("bin/systemctl")).unwrap();
    for subcommand in ["doctor", "status"] {
        let output = fixture.command(subcommand).output().unwrap();
        let stdout = stdout(&output);
        assert!(stdout.contains("failed to run systemctl:"), "{stdout}");
        if subcommand == "doctor" {
            assert!(doctor_unit_line(&stdout)
                .ends_with("skipped because systemctl --user is not available"));
        } else {
            assert!(stdout.contains("Service  unknown: failed to run systemctl:"));
        }
        assert!(fixture.calls().is_empty());
    }
}
