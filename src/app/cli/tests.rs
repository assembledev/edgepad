use super::*;

#[test]
fn doctor_args_default_to_auto_device_and_system_paths() {
    let args = parse_doctor_args(Vec::<String>::new().into_iter()).expect("doctor args parse");

    assert_eq!(args.config_path, None);
    assert_eq!(args.device_override, None);
    assert_eq!(args.input_root, PathBuf::from("/dev/input"));
    assert_eq!(args.uinput_path, PathBuf::from("/dev/uinput"));
    assert_eq!(args.service_name, "edgepad.service");
}

#[test]
fn doctor_args_accept_overrides() {
    let args = parse_doctor_args(
        [
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
        ]
        .into_iter()
        .map(String::from),
    )
    .expect("doctor args parse");

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
    let err = parse_doctor_args(["--wat"].into_iter().map(String::from))
        .expect_err("unknown option should fail");

    assert_eq!(err, "unknown doctor option --wat");
}
