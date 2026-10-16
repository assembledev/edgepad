use std::path::Path;

use crate::diagnostics::{command_output, CommandOutput};

use super::devices::DeviceAccess;
use super::{CheckStatus, DoctorCheck, DoctorSection};

mod observe;

pub(super) fn check(devices: &DeviceAccess) -> Vec<DoctorCheck> {
    vec![
        check_tags(devices),
        check_acl(devices),
        seat_check(
            devices.both_devices_open,
            command_output("loginctl", &["session-status", "--no-pager"]),
        ),
    ]
}

fn check_tags(devices: &DeviceAccess) -> DoctorCheck {
    let Some(touchpad_path) = devices.touchpad_path.as_deref() else {
        return no_touchpad_check("udev tags");
    };
    let touchpad = observe::udevadm_current_tags(touchpad_path);
    let uinput = observe::udevadm_current_tags(&devices.uinput_path);
    tags_check(
        touchpad_path,
        &devices.uinput_path,
        devices.both_devices_open,
        touchpad,
        uinput,
    )
}

fn tags_check(
    touchpad_path: &Path,
    uinput_path: &Path,
    device_access_ok: bool,
    touchpad: Result<Vec<String>, String>,
    uinput: Result<Vec<String>, String>,
) -> DoctorCheck {
    let (status, detail) = match (touchpad, uinput) {
        (Ok(touchpad_tags), Ok(uinput_tags))
            if has_tag(&touchpad_tags, "uaccess")
                && has_tag(&touchpad_tags, "seat")
                && has_tag(&uinput_tags, "uaccess")
                && has_tag(&uinput_tags, "seat") =>
        {
            (
                CheckStatus::Ok,
                format!(
                    "{} and {} have current udev tags seat,uaccess",
                    touchpad_path.display(),
                    uinput_path.display()
                ),
            )
        }
        (Ok(touchpad_tags), Ok(uinput_tags)) => (
            access_model_gap_status(device_access_ok),
            format!(
                "missing seat/uaccess current tags; touchpad={touchpad_tags:?} uinput={uinput_tags:?}{}",
                access_model_gap_note(device_access_ok),
            ),
        ),
        (Err(err), _) | (_, Err(err)) => (CheckStatus::Fail, err),
    };
    DoctorCheck::new(status, DoctorSection::Access, "udev tags", detail)
}

fn check_acl(devices: &DeviceAccess) -> DoctorCheck {
    let Some(touchpad_path) = devices.touchpad_path.as_deref() else {
        return no_touchpad_check("acl");
    };
    let Some(username) = observe::current_username() else {
        return DoctorCheck::new(
            CheckStatus::Warn,
            DoctorSection::Access,
            "acl",
            "could not determine current username for ACL inspection",
        );
    };
    let touchpad_acl = observe::getfacl_grants_user(touchpad_path, &username);
    let uinput_acl = observe::getfacl_grants_user(&devices.uinput_path, &username);
    acl_check(
        touchpad_path,
        &devices.uinput_path,
        devices.both_devices_open,
        &username,
        touchpad_acl,
        uinput_acl,
    )
}

fn acl_check(
    touchpad_path: &Path,
    uinput_path: &Path,
    device_access_ok: bool,
    username: &str,
    touchpad_acl: Result<bool, String>,
    uinput_acl: Result<bool, String>,
) -> DoctorCheck {
    let (status, detail) = match (touchpad_acl, uinput_acl) {
        (Ok(true), Ok(true)) => (
            CheckStatus::Ok,
            format!(
                "user {username} has rw ACL on {} and {}",
                touchpad_path.display(),
                uinput_path.display()
            ),
        ),
        (Ok(touchpad_ok), Ok(uinput_ok)) => (
            access_model_gap_status(device_access_ok),
            format!(
                "missing rw ACL for user {username}; touchpad_acl={touchpad_ok} uinput_acl={uinput_ok}{}",
                access_model_gap_note(device_access_ok),
            ),
        ),
        (Err(err), _) | (_, Err(err)) => (CheckStatus::Warn, err),
    };
    DoctorCheck::new(status, DoctorSection::Access, "acl", detail)
}

fn no_touchpad_check(label: &'static str) -> DoctorCheck {
    DoctorCheck::new(
        CheckStatus::Fail,
        DoctorSection::Access,
        label,
        "skipped because no touchpad event node is selected",
    )
}

fn seat_check(device_access_ok: bool, observation: Result<CommandOutput, String>) -> DoctorCheck {
    let (status, detail) = match observation {
        Ok(output)
            if output.status_success
                && output.stdout.contains("State: active")
                && output.stdout.contains("Seat: seat") =>
        {
            (
                CheckStatus::Ok,
                "current session is active on a local seat".to_string(),
            )
        }
        Ok(output) if output.status_success => (
            access_model_gap_status(device_access_ok),
            format!(
                "current loginctl session is not active on a local seat{}",
                access_model_gap_note(device_access_ok)
            ),
        ),
        Ok(output) => (
            access_model_gap_status(device_access_ok),
            format!(
                "loginctl session-status failed: {}{}",
                output.stderr_or_stdout(),
                access_model_gap_note(device_access_ok)
            ),
        ),
        Err(err) => (
            access_model_gap_status(device_access_ok),
            format!("{err}{}", access_model_gap_note(device_access_ok)),
        ),
    };
    DoctorCheck::new(status, DoctorSection::Session, "seat", detail)
}

fn has_tag(tags: &[String], wanted: &str) -> bool {
    tags.iter().any(|tag| tag == wanted)
}

// Group membership can grant working access without logind-managed ACLs.
fn access_model_gap_status(device_access_ok: bool) -> CheckStatus {
    if device_access_ok {
        CheckStatus::Warn
    } else {
        CheckStatus::Fail
    }
}

fn access_model_gap_note(device_access_ok: bool) -> &'static str {
    if device_access_ok {
        "; device access is currently functional through another access model"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests;
