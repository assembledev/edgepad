use super::*;
use clap::{error::ErrorKind, CommandFactory};

#[test]
fn command_schema_is_valid() {
    Cli::command().debug_assert();
}

#[test]
fn doctor_and_status_defaults_match_their_runtime_configs() {
    let Command::Doctor(args) = Cli::try_parse_from(["edgepad", "doctor"]).unwrap().command else {
        panic!("doctor command expected");
    };
    assert_eq!(DoctorConfig::from(args), DoctorConfig::default());

    let Command::Status(args) = Cli::try_parse_from(["edgepad", "status"]).unwrap().command else {
        panic!("status command expected");
    };
    assert_eq!(StatusConfig::from(args), StatusConfig::default());
}

#[test]
fn doctor_args_accept_overrides() {
    let Command::Doctor(args) = Cli::try_parse_from([
        "edgepad",
        "doctor",
        "--config",
        "/tmp/edgepad.toml",
        "--device",
        "/tmp/input/event7",
        "--input-root",
        "/tmp/input",
        "--uinput",
        "/tmp/uinput",
        "--service",
        "custom-edgepad.service",
    ])
    .unwrap()
    .command
    else {
        panic!("doctor command expected");
    };
    let args = DoctorConfig::from(args);
    assert_eq!(args.config_path, Some(PathBuf::from("/tmp/edgepad.toml")));
    assert_eq!(
        args.device_override,
        Some(DeviceConfig::Path(PathBuf::from("/tmp/input/event7")))
    );
    assert_eq!(args.input_root, PathBuf::from("/tmp/input"));
    assert_eq!(args.uinput_path, PathBuf::from("/tmp/uinput"));
    assert_eq!(args.service_name, "custom-edgepad.service");
}

#[test]
fn doctor_args_reject_unknown_option() {
    let error = Cli::try_parse_from(["edgepad", "doctor", "--wat"]).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::UnknownArgument);
}

#[test]
fn proxy_accepts_only_the_two_explicit_modes() {
    for flags in 0..8 {
        let mut args = vec!["edgepad", "proxy", "--frames", "2", "--built-in-defaults"];
        if flags & 1 != 0 {
            args.push("--dry-run");
        }
        if flags & 2 != 0 {
            args.push("--uinput");
        }
        if flags & 4 != 0 {
            args.push("--grab");
        }
        let parsed = Cli::try_parse_from(args);
        match flags {
            1 | 6 => {
                let Command::Proxy(args) = parsed.unwrap().command else {
                    panic!("proxy command expected");
                };
                let expected = if flags == 1 {
                    ProxyMode::DryRun
                } else {
                    ProxyMode::UinputGrab
                };
                assert_eq!(args.mode(), expected);
            }
            _ => assert!(parsed.is_err(), "unsafe/incomplete mode {flags} must fail"),
        }
    }
}

#[test]
fn profile_sources_conflict_in_all_profile_commands() {
    for command in ["proxy", "replay", "replay-raw"] {
        let mut args = vec![
            "edgepad",
            command,
            "--config",
            "missing.toml",
            "--built-in-defaults",
        ];
        if command == "proxy" {
            args.extend(["--frames", "1", "--dry-run"]);
        } else {
            args.push("capture.ev");
        }
        let error = Cli::try_parse_from(args).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict, "{command}");
    }
}

#[test]
fn repeated_options_keep_the_last_value_and_devices_keeps_root_alias() {
    let Command::Devices(args) = Cli::try_parse_from([
        "edgepad",
        "devices",
        "--input-root",
        "/first",
        "--root",
        "/last",
        "--all",
        "--all",
    ])
    .unwrap()
    .command
    else {
        panic!("devices command expected");
    };
    assert_eq!(args.root, PathBuf::from("/last"));
    assert!(args.show_all);

    let Command::Proxy(args) = Cli::try_parse_from([
        "edgepad",
        "proxy",
        "--frames",
        "1",
        "--frames",
        "2",
        "--dry-run",
        "--dry-run",
    ])
    .unwrap()
    .command
    else {
        panic!("proxy command expected");
    };
    assert_eq!(args.frames, 2);
    assert_eq!(args.mode(), ProxyMode::DryRun);
}

#[test]
fn help_never_loads_daemon_config() {
    let error = Cli::try_parse_from([
        "edgepad",
        "daemon",
        "--config",
        "/missing/config.toml",
        "--help",
    ])
    .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::DisplayHelp);
}

#[test]
fn validation_rejects_zero_frames_and_non_finite_widths() {
    for value in ["0", "-1", "bad"] {
        assert!(Cli::try_parse_from(["edgepad", "proxy", "--frames", value, "--dry-run"]).is_err());
    }
    for value in ["0", "0.5", "NaN", "inf", "-0.1"] {
        assert!(Cli::try_parse_from(["edgepad", "daemon", "--edge-width", value]).is_err());
    }
}
