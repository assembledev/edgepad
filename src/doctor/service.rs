use crate::diagnostics::{command_output, observe_user_service, ServiceObservation};

use super::{CheckStatus, DoctorCheck, DoctorSection};

pub(super) fn check(service_name: &str) -> Vec<DoctorCheck> {
    let manager = check_systemd_user();
    let unit = if manager.status == CheckStatus::Ok {
        unit_check(service_name, observe_user_service(service_name))
    } else {
        DoctorCheck::new(
            CheckStatus::Warn,
            DoctorSection::Service,
            "unit",
            "skipped because systemctl --user is not available",
        )
    };
    vec![manager, unit]
}

fn unit_check(service_name: &str, observation: ServiceObservation) -> DoctorCheck {
    let (status, detail) = match observation {
        ServiceObservation::State(state) => {
            if state.load_state == "not-found" {
                (
                    CheckStatus::Fail,
                    format!("{service_name} is not installed in the user manager"),
                )
            } else if state.active_state == "active" {
                (
                    CheckStatus::Ok,
                    format!("{service_name} is active ({})", state.sub_state),
                )
            } else if state.active_state == "inactive" {
                (
                    CheckStatus::Fail,
                    format!("{service_name} is inactive ({})", state.sub_state),
                )
            } else if state.active_state == "failed" {
                (
                    CheckStatus::Fail,
                    format!("{service_name} is failed ({})", state.sub_state),
                )
            } else {
                (
                    CheckStatus::Warn,
                    format!(
                        "{service_name} is loaded={} active={} sub={}",
                        state.load_state, state.active_state, state.sub_state,
                    ),
                )
            }
        }
        ServiceObservation::QueryFailed(detail) => (
            CheckStatus::Warn,
            format!("could not inspect {service_name}: {detail}"),
        ),
        ServiceObservation::Unavailable(detail) => (CheckStatus::Warn, detail),
    };
    DoctorCheck::new(status, DoctorSection::Service, "unit", detail)
}

fn check_systemd_user() -> DoctorCheck {
    let (status, detail) = match command_output("systemctl", &["--user", "show-environment"]) {
        Ok(output) if output.status_success => {
            (CheckStatus::Ok, "systemctl --user is available".to_string())
        }
        Ok(output) => (
            CheckStatus::Fail,
            format!("systemctl --user failed: {}", output.stderr_or_stdout()),
        ),
        Err(err) => (CheckStatus::Fail, err),
    };
    DoctorCheck::new(status, DoctorSection::Session, "systemd", detail)
}

#[cfg(test)]
mod tests;
