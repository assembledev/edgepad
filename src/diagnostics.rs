use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandOutput {
    pub status_success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn stderr_or_stdout(&self) -> &str {
        let stderr = self.stderr.trim();
        if stderr.is_empty() {
            self.stdout.trim()
        } else {
            stderr
        }
    }
}

pub(crate) fn command_output(program: &str, args: &[&str]) -> Result<CommandOutput, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|err| format!("failed to run {program}: {err}"))?;
    Ok(CommandOutput {
        status_success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

pub(crate) fn property_value<'a>(output: &'a str, key: &str) -> Option<&'a str> {
    output.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name == key).then_some(value)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServiceState {
    pub load_state: String,
    pub active_state: String,
    pub sub_state: String,
    pub main_pid: Option<String>,
    pub status_text: Option<String>,
}

impl ServiceState {
    pub fn parse(output: &str) -> Self {
        Self {
            load_state: property_value(output, "LoadState")
                .unwrap_or("unknown")
                .to_string(),
            active_state: property_value(output, "ActiveState")
                .unwrap_or("unknown")
                .to_string(),
            sub_state: property_value(output, "SubState")
                .unwrap_or("unknown")
                .to_string(),
            main_pid: property_value(output, "MainPID")
                .filter(|pid| !pid.is_empty() && *pid != "0")
                .map(ToString::to_string),
            status_text: property_value(output, "StatusText")
                .filter(|text| !text.is_empty())
                .map(ToString::to_string),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ServiceObservation {
    State(ServiceState),
    QueryFailed(String),
    Unavailable(String),
}

pub(crate) fn observe_user_service(service_name: &str) -> ServiceObservation {
    match command_output(
        "systemctl",
        &[
            "--user",
            "show",
            service_name,
            "-p",
            "LoadState",
            "-p",
            "ActiveState",
            "-p",
            "SubState",
            "-p",
            "MainPID",
            "-p",
            "StatusText",
        ],
    ) {
        Ok(output) if output.status_success => {
            ServiceObservation::State(ServiceState::parse(&output.stdout))
        }
        Ok(output) => ServiceObservation::QueryFailed(output.stderr_or_stdout().to_string()),
        Err(err) => ServiceObservation::Unavailable(err),
    }
}

#[cfg(test)]
mod tests;
