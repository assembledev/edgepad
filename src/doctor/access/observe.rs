use crate::diagnostics::{command_output, property_value};
use std::env;
use std::path::Path;

pub(super) fn udevadm_current_tags(path: &Path) -> Result<Vec<String>, String> {
    let output = command_output(
        "udevadm",
        &[
            "info",
            "--query=property",
            "--name",
            &path.display().to_string(),
        ],
    )?;
    if !output.status_success {
        return Err(format!(
            "udevadm info failed for {}: {}",
            path.display(),
            output.stderr_or_stdout()
        ));
    }

    Ok(parse_udev_current_tags(&output.stdout))
}

pub(super) fn current_username() -> Option<String> {
    env::var("USER")
        .ok()
        .filter(|user| !user.is_empty())
        .or_else(|| {
            let output = command_output("id", &["-un"]).ok()?;
            output
                .status_success
                .then(|| output.stdout.trim().to_string())
        })
        .filter(|user| !user.is_empty())
}

pub(super) fn getfacl_grants_user(path: &Path, username: &str) -> Result<bool, String> {
    let output = command_output("getfacl", &["-cp", &path.display().to_string()])?;
    if !output.status_success {
        return Err(format!(
            "getfacl failed for {}: {}",
            path.display(),
            output.stderr_or_stdout()
        ));
    }

    Ok(acl_output_grants_user(&output.stdout, username))
}

fn parse_udev_current_tags(output: &str) -> Vec<String> {
    property_value(output, "CURRENT_TAGS")
        .or_else(|| property_value(output, "TAGS"))
        .map(parse_colon_tags)
        .unwrap_or_default()
}

fn parse_colon_tags(raw: &str) -> Vec<String> {
    raw.split(':')
        .filter(|tag| !tag.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn acl_output_grants_user(output: &str, username: &str) -> bool {
    let prefix = format!("user:{username}:");
    output.lines().any(|line| {
        let Some(perms) = line.strip_prefix(&prefix) else {
            return false;
        };
        perms.as_bytes().first() == Some(&b'r') && perms.as_bytes().get(1) == Some(&b'w')
    })
}

#[cfg(test)]
mod tests;
