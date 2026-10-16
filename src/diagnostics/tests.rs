use super::*;

#[test]
fn property_values_preserve_empty_values_and_embedded_equals() {
    let properties = "OtherKey=ignored\nStatusText=waiting=seat\nEmpty=\nStatusText=second\n";
    assert_eq!(
        property_value(properties, "StatusText"),
        Some("waiting=seat")
    );
    assert_eq!(property_value(properties, "Empty"), Some(""));
    assert_eq!(property_value(properties, "Key"), None);
    assert_eq!(property_value(properties, "Missing"), None);
}

#[test]
fn missing_service_properties_remain_unknown() {
    let state = ServiceState::parse("Unrecognized=value\n");
    assert_eq!(state.load_state, "unknown");
    assert_eq!(state.active_state, "unknown");
    assert_eq!(state.sub_state, "unknown");
    assert_eq!(state.main_pid, None);
    assert_eq!(state.status_text, None);
}

#[test]
fn service_state_preserves_details_and_omits_empty_pid_and_status() {
    let state = ServiceState::parse(
        "LoadState=loaded\nActiveState=activating\nSubState=start\nMainPID=1234\nStatusText=waiting=seat\n",
    );
    assert_eq!(state.load_state, "loaded");
    assert_eq!(state.active_state, "activating");
    assert_eq!(state.sub_state, "start");
    assert_eq!(state.main_pid.as_deref(), Some("1234"));
    assert_eq!(state.status_text.as_deref(), Some("waiting=seat"));
    for pid in ["", "0"] {
        let state = ServiceState::parse(&format!("MainPID={pid}\nStatusText=\n"));
        assert!(state.main_pid.is_none());
        assert!(state.status_text.is_none());
    }
}

#[test]
fn command_diagnostics_prefer_stderr_and_fall_back_to_stdout() {
    for (stderr, stdout, expected) in [
        (" stderr\n", "stdout", "stderr"),
        (" \n", " stdout\n", "stdout"),
        ("", "", ""),
    ] {
        let output = CommandOutput {
            status_success: false,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        };
        assert_eq!(output.stderr_or_stdout(), expected);
    }
}
