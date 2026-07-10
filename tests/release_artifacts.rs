use edgepad::config::{DeviceConfig, EdgepadConfig, GestureActionConfig};
use edgepad::core::{GestureDirection, SliderAxis, Zone};

#[test]
fn release_example_config_parses_and_uses_axis_appropriate_gestures() {
    let config = EdgepadConfig::parse(include_str!("../examples/edgepad.toml.example"))
        .expect("release example config should parse");

    assert_eq!(config.device, DeviceConfig::Auto);
    assert_eq!(config.gestures.len(), 7);
    assert_eq!(config.sliders.len(), 2);

    for slider in &config.sliders {
        assert!(
            matches!(slider.zone, Zone::Left | Zone::Right),
            "release example sliders should use side zones: {slider:?}"
        );
        assert_eq!(slider.axis, SliderAxis::Vertical);
        assert!(!slider.negative.argv.is_empty());
        assert!(!slider.positive.argv.is_empty());
    }

    for binding in &config.gestures {
        match binding.zone {
            Zone::Left | Zone::Right => {
                assert!(
                    matches!(binding.direction, GestureDirection::Tap),
                    "side zones with sliders should only use tap gestures: {binding:?}"
                );
            }
            Zone::Top => {
                assert!(
                    matches!(
                        binding.direction,
                        GestureDirection::Left | GestureDirection::Right | GestureDirection::Tap
                    ),
                    "top examples should use horizontal or tap gestures: {binding:?}"
                );
            }
            Zone::Bottom => {
                assert!(
                    matches!(
                        binding.direction,
                        GestureDirection::Left | GestureDirection::Right
                    ),
                    "bottom examples should use horizontal gestures: {binding:?}"
                );
            }
        }
        assert!(
            matches!(binding.action, GestureActionConfig::Command { .. }),
            "example gestures should run command argv actions: {binding:?}"
        );
    }
}

#[test]
fn release_udev_rules_use_uaccess_for_touchpad_and_uinput() {
    let rules = include_str!("../packaging/70-edgepad.rules");

    assert!(rules.contains(r#"ENV{ID_INPUT_TOUCHPAD}=="1""#));
    assert!(rules.contains(r#"TAG+="uaccess""#));
    assert!(rules.contains(r#"KERNEL=="uinput""#));
    assert!(rules.contains(r#"OPTIONS+="static_node=uinput""#));
}

#[test]
fn release_user_service_runs_installed_user_binary_with_config() {
    let service = include_str!("../packaging/edgepad.service");

    assert!(
        service.contains(
            "ExecStart=%h/.local/bin/edgepad daemon --config %h/.config/edgepad/edgepad.toml"
        ),
        "service was: {service}"
    );
    assert!(service.contains("WantedBy=default.target"));
    assert!(service.contains("Restart=on-failure"));
}

#[test]
fn release_workflow_publishes_required_assets_and_checksums() {
    let workflow = include_str!("../.github/workflows/release.yml");

    for required in [
        "edgepad-x86_64-unknown-linux-musl",
        "70-edgepad.rules",
        "edgepad.service",
        "edgepad.toml.example",
        "checksums",
        "Verify tag matches Cargo version",
        "v$PACKAGE_VERSION",
        "gh release create",
        "--generate-notes",
        "targets: x86_64-unknown-linux-musl",
        "--target x86_64-unknown-linux-musl",
        "readelf -l",
        "INTERP",
    ] {
        assert!(
            workflow.contains(required),
            "release workflow should mention {required}"
        );
    }
}
