use super::*;

#[test]
fn access_model_gap_status_warns_when_device_access_is_functional() {
    assert_eq!(access_model_gap_status(true), CheckStatus::Warn);
    assert_eq!(access_model_gap_status(false), CheckStatus::Fail);
}

#[test]
fn access_model_gap_note_explains_functional_non_uaccess_access() {
    assert!(access_model_gap_note(true).contains("another access model"));
    assert_eq!(access_model_gap_note(false), "");
}

fn output(success: bool, stdout: &str, stderr: &str) -> Result<CommandOutput, String> {
    Ok(CommandOutput {
        status_success: success,
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
    })
}

#[test]
fn missing_uaccess_metadata_is_a_warning_only_when_both_devices_opened() {
    for (devices_open, expected) in [(true, CheckStatus::Warn), (false, CheckStatus::Fail)] {
        let checks = [
            tags_check(
                Path::new("/input/event0"),
                Path::new("/uinput"),
                devices_open,
                Ok(vec![]),
                Ok(vec![]),
            ),
            acl_check(
                Path::new("/input/event0"),
                Path::new("/uinput"),
                devices_open,
                "tester",
                Ok(false),
                Ok(true),
            ),
            seat_check(devices_open, output(true, "State: active\n", "")),
        ];
        for check in checks {
            assert_eq!(check.status, expected, "{}", check.detail);
            assert_eq!(check.detail.contains("another access model"), devices_open);
        }
    }
}

#[test]
fn successful_access_metadata_remains_visible_when_a_device_open_fails() {
    for devices_open in [true, false] {
        let tags = || Ok(vec!["seat".to_string(), "uaccess".to_string()]);
        let checks = [
            tags_check(
                Path::new("/input/event0"),
                Path::new("/uinput"),
                devices_open,
                tags(),
                tags(),
            ),
            acl_check(
                Path::new("/input/event0"),
                Path::new("/uinput"),
                devices_open,
                "tester",
                Ok(true),
                Ok(true),
            ),
            seat_check(
                devices_open,
                output(true, "State: active\nSeat: seat0\n", ""),
            ),
        ];
        assert!(checks.iter().all(|check| check.status == CheckStatus::Ok));
    }
}

#[test]
fn command_failures_keep_their_details_and_each_checks_severity() {
    for devices_open in [true, false] {
        let tags = tags_check(
            Path::new("/input/event0"),
            Path::new("/uinput"),
            devices_open,
            Err("udev failed".to_string()),
            Ok(vec![]),
        );
        assert_eq!(tags.status, CheckStatus::Fail);
        assert_eq!(tags.detail, "udev failed");
        let acl = acl_check(
            Path::new("/input/event0"),
            Path::new("/uinput"),
            devices_open,
            "tester",
            Ok(true),
            Err("getfacl failed".to_string()),
        );
        assert_eq!(acl.status, CheckStatus::Warn);
        assert_eq!(acl.detail, "getfacl failed");
        for observation in [
            output(false, "fallback", "seat unavailable"),
            output(false, "seat unavailable", ""),
            Err("seat unavailable".to_string()),
        ] {
            let seat = seat_check(devices_open, observation);
            assert_eq!(
                seat.status,
                if devices_open {
                    CheckStatus::Warn
                } else {
                    CheckStatus::Fail
                }
            );
            assert!(seat.detail.contains("seat unavailable"), "{}", seat.detail);
        }
    }
}
