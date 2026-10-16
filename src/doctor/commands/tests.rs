use super::*;
use crate::config::DeviceConfig;
use crate::config::{
    EdgeWidthOverrides, GestureBindingConfig, DEFAULT_SWIPE_MIN_DISTANCE,
    DEFAULT_TAP_MIN_DURATION_MS,
};
use crate::core::{GestureDirection, Zone};

#[test]
fn action_executable_check_reports_missing_path_command() {
    let config = EdgepadConfig {
        device: DeviceConfig::Auto,
        edge_width: 0.10,
        edge_width_overrides: EdgeWidthOverrides::default(),
        tap_min_duration_ms: DEFAULT_TAP_MIN_DURATION_MS,
        swipe_min_distance: DEFAULT_SWIPE_MIN_DISTANCE,
        gestures: vec![GestureBindingConfig {
            zone: Zone::Right,
            direction: GestureDirection::Up,
            action: GestureActionConfig::Command {
                argv: vec!["/tmp/edgepad-definitely-missing-command".to_string()],
            },
        }],
        sliders: Vec::new(),
        ..EdgepadConfig::default()
    };

    let checks = check_with_path(&config, None);

    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].status, CheckStatus::Fail);
    assert!(checks[0].detail.contains("not found"));
    assert!(checks[0].detail.contains("right.up"));
}

#[test]
fn find_executable_in_path_finds_executable_file() {
    let dir = crate::doctor::test_support::TestDir::new();
    let program = dir.executable("edgepad-test-tool");
    let found = find_executable_in_path("edgepad-test-tool", Some(dir.path().as_os_str()));
    assert_eq!(found, Some(program));
}

#[test]
fn command_checks_group_gestures_and_both_slider_axes_by_executable() {
    use crate::config::{CommandActionConfig, SliderBindingConfig};

    let dir = crate::doctor::test_support::TestDir::new();
    let alpha = dir.executable("alpha");
    let beta = dir.executable("beta");
    let config = EdgepadConfig {
        gestures: vec![GestureBindingConfig {
            zone: Zone::Right,
            direction: GestureDirection::Tap,
            action: GestureActionConfig::command(["beta", "argument"]).unwrap(),
        }],
        sliders: vec![
            SliderBindingConfig {
                zone: Zone::Left,
                axis: SliderAxis::Vertical,
                step: 0.1,
                negative: CommandActionConfig::new(["alpha"]).unwrap(),
                positive: CommandActionConfig::new(["beta"]).unwrap(),
            },
            SliderBindingConfig {
                zone: Zone::Bottom,
                axis: SliderAxis::Horizontal,
                step: 0.1,
                negative: CommandActionConfig::new(["beta"]).unwrap(),
                positive: CommandActionConfig::new(["alpha"]).unwrap(),
            },
        ],
        ..EdgepadConfig::default()
    };
    let checks = check_with_path(&config, Some(dir.path().as_os_str()));
    assert_eq!(checks.len(), 2);
    assert!(checks.iter().all(|check| check.status == CheckStatus::Ok));
    assert_eq!(
        checks[0].detail,
        format!(
            "alpha: 2 bindings (left.up, bottom.right)\npath: {}",
            alpha.display()
        ),
    );
    assert_eq!(
        checks[1].detail,
        format!(
            "beta: 3 bindings (right.tap, left.down, bottom.left)\npath: {}",
            beta.display()
        ),
    );
}

#[test]
fn explicit_executables_work_without_path_and_path_search_skips_nonexecutables() {
    let dir = crate::doctor::test_support::TestDir::new();
    let blocked = dir.path().join("blocked");
    let bin = dir.path().join("bin");
    fs::create_dir(&blocked).unwrap();
    fs::create_dir(&bin).unwrap();
    fs::write(blocked.join("tool"), "").unwrap();
    fs::set_permissions(blocked.join("tool"), fs::Permissions::from_mode(0o644)).unwrap();
    let executable = bin.join("tool");
    fs::write(&executable, "").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    let search_path = env::join_paths([&blocked, &bin]).unwrap();

    for (program, search_path) in [
        ("tool", Some(search_path.as_os_str())),
        (executable.to_str().unwrap(), None),
    ] {
        let check = command_check(program, &["left.tap".to_string()], search_path);
        assert_eq!(check.status, CheckStatus::Ok);
        assert!(check
            .detail
            .ends_with(&format!("path: {}", executable.display())));
    }
    let missing = command_check("tool", &[], None);
    assert_eq!(missing.status, CheckStatus::Fail);
    assert!(missing.detail.contains("not found in PATH"));
}
