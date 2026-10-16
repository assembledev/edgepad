use crate::diagnostics::{observe_user_service, ServiceObservation, ServiceState};

use super::{StatusLine, StatusSubject};

pub(super) fn status_line(service_name: &str) -> StatusLine {
    line_for_observation(service_name, observe_user_service(service_name))
}

fn line_for_observation(service_name: &str, observation: ServiceObservation) -> StatusLine {
    match observation {
        ServiceObservation::State(state) => {
            if state.load_state == "not-found" {
                StatusLine::fail(
                    StatusSubject::Service,
                    format!("{service_name} is not installed"),
                )
            } else if state.active_state == "active" {
                StatusLine::ok(StatusSubject::Service, service_state_detail(&state))
            } else {
                StatusLine::fail(StatusSubject::Service, service_state_detail(&state))
            }
        }
        ServiceObservation::QueryFailed(detail) => StatusLine::warn(
            StatusSubject::Service,
            format!("unknown: systemctl --user failed: {detail}"),
        ),
        ServiceObservation::Unavailable(detail) => {
            StatusLine::warn(StatusSubject::Service, format!("unknown: {detail}"))
        }
    }
}

fn service_state_detail(state: &ServiceState) -> String {
    let mut detail = format!("{} ({})", state.active_state, state.sub_state);
    if let Some(pid) = &state.main_pid {
        detail.push_str(&format!(", pid {pid}"));
    }
    if let Some(status) = &state.status_text {
        detail.push_str(&format!(", {status}"));
    }
    detail
}

#[cfg(test)]
mod tests;
