use super::*;
use crate::diagnostics::ServiceState;
use crate::doctor::DoctorReport;

#[test]
fn missing_stopped_and_failed_units_are_problems_while_starting_is_pending() {
    for (properties, expected_status, expected_detail) in [
        (
            "LoadState=not-found\nActiveState=inactive\nSubState=dead\n",
            CheckStatus::Fail,
            "custom.service is not installed in the user manager",
        ),
        (
            "LoadState=loaded\nActiveState=inactive\nSubState=dead\n",
            CheckStatus::Fail,
            "custom.service is inactive (dead)",
        ),
        (
            "LoadState=loaded\nActiveState=failed\nSubState=failed\n",
            CheckStatus::Fail,
            "custom.service is failed (failed)",
        ),
        (
            "LoadState=loaded\nActiveState=activating\nSubState=start\n",
            CheckStatus::Warn,
            "custom.service is loaded=loaded active=activating sub=start",
        ),
        (
            "LoadState=loaded\nActiveState=active\nSubState=running\n",
            CheckStatus::Ok,
            "custom.service is active (running)",
        ),
        (
            "",
            CheckStatus::Warn,
            "custom.service is loaded=unknown active=unknown sub=unknown",
        ),
    ] {
        let check = unit_check(
            "custom.service",
            ServiceObservation::State(ServiceState::parse(properties)),
        );
        assert_eq!(check.status, expected_status);
        assert_eq!(check.section, DoctorSection::Service);
        assert_eq!(check.detail, expected_detail);
        assert_eq!(
            DoctorReport {
                checks: vec![check]
            }
            .has_failures(),
            expected_status == CheckStatus::Fail,
        );
    }
}

#[test]
fn service_inspection_errors_are_warnings_with_command_context() {
    for (observation, expected_detail) in [
        (
            ServiceObservation::QueryFailed("bus unavailable".to_string()),
            "could not inspect custom.service: bus unavailable",
        ),
        (
            ServiceObservation::Unavailable("failed to run systemctl: missing".to_string()),
            "failed to run systemctl: missing",
        ),
    ] {
        let check = unit_check("custom.service", observation);
        assert_eq!(check.status, CheckStatus::Warn);
        assert_eq!(check.detail, expected_detail);
    }
}
