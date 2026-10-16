use super::*;
use crate::status::{status_result, StatusResult, StatusSeverity};

#[test]
fn active_service_includes_pid_and_status_text() {
    let state = ServiceState::parse(
        "LoadState=loaded\nActiveState=active\nSubState=running\nMainPID=1234\nStatusText=edgepad ready\n",
    );
    let line = line_for_observation("custom.service", ServiceObservation::State(state));
    assert_eq!(line.severity, StatusSeverity::Ok);
    assert_eq!(line.detail, "active (running), pid 1234, edgepad ready");
    assert_eq!(status_result(&[line]), StatusResult::Running);
}

#[test]
fn only_an_active_unit_is_reported_as_running() {
    for (properties, expected_detail) in [
        ("LoadState=not-found\nActiveState=inactive\nSubState=dead\n", "custom.service is not installed"),
        ("LoadState=loaded\nActiveState=inactive\nSubState=dead\nMainPID=0\nStatusText=\n", "inactive (dead)"),
        ("LoadState=loaded\nActiveState=failed\nSubState=failed\n", "failed (failed)"),
        ("LoadState=loaded\nActiveState=activating\nSubState=start\nMainPID=4321\nStatusText=waiting for touchpad access\n", "activating (start), pid 4321, waiting for touchpad access"),
        ("", "unknown (unknown)"),
    ] {
        let line = line_for_observation("custom.service", ServiceObservation::State(ServiceState::parse(properties)));
        assert_eq!(line.severity, StatusSeverity::Fail);
        assert_eq!(line.detail, expected_detail);
        let result = status_result(&[line]);
        assert_eq!(result, StatusResult::NotRunning);
        assert_eq!(result.exit_code(), 2);
    }
}

#[test]
fn failed_queries_are_unknown_rather_than_an_observed_inactive_unit() {
    for (observation, expected_detail) in [
        (
            ServiceObservation::QueryFailed("bus unavailable".to_string()),
            "unknown: systemctl --user failed: bus unavailable",
        ),
        (
            ServiceObservation::Unavailable("failed to run systemctl: missing".to_string()),
            "unknown: failed to run systemctl: missing",
        ),
    ] {
        let line = line_for_observation("custom.service", observation);
        assert_eq!(line.severity, StatusSeverity::Warn);
        assert_eq!(line.detail, expected_detail);
        assert_eq!(status_result(&[line]), StatusResult::NeedsAttention);
    }
}
