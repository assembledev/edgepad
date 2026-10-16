use std::path::PathBuf;
use std::time::Duration;

use crate::config::*;
use crate::core::{EdgeWidths, GestureDirection, SliderAxis, SliderSpec, Zone};

#[test]
fn edgepad_config_parses_device_edge_width_and_gesture_bindings() {
    let config = EdgepadConfig::parse(
        r#"
            device = "/dev/input/event7"
            edge_width = 0.20
            tap_min_duration_ms = 90
            swipe_min_distance = 0.03

            [[gestures]]
            zone = "left"
            direction = "right"
            action = { log = true }

            [[gestures]]
            zone = "right"
            direction = "down"
            action = ["notify-send", "edgepad", "right-down"]
            "#,
    )
    .expect("config should parse");

    assert_eq!(
        config.device,
        DeviceConfig::Path(PathBuf::from("/dev/input/event7"))
    );
    assert_eq!(config.edge_width, 0.20);
    assert_eq!(config.edge_width_overrides, EdgeWidthOverrides::default());
    assert_eq!(config.tap_min_duration_ms, 90);
    assert_eq!(config.swipe_min_distance, 0.03);
    assert_eq!(
        config.engine_options().tap_min_duration,
        Duration::from_millis(90)
    );
    assert_eq!(config.engine_options().swipe_min_distance, 0.03);
    assert_eq!(
        config.gestures,
        vec![
            GestureBindingConfig {
                zone: Zone::Left,
                direction: GestureDirection::Right,
                action: GestureActionConfig::Log,
            },
            GestureBindingConfig {
                zone: Zone::Right,
                direction: GestureDirection::Down,
                action: GestureActionConfig::Command {
                    argv: vec![
                        "notify-send".to_string(),
                        "edgepad".to_string(),
                        "right-down".to_string(),
                    ],
                },
            },
        ]
    );
}

#[test]
fn palm_rejection_defaults_on_and_can_be_disabled() {
    let defaults = EdgepadConfig::parse("").unwrap();
    assert!(defaults.palm_rejection);
    assert!(defaults.engine_options().palm_rejection);
    let enabled = EdgepadConfig::parse("palm_rejection = true").unwrap();
    assert!(enabled.engine_options().palm_rejection);
    let disabled = EdgepadConfig::parse("palm_rejection = false").unwrap();
    assert!(!disabled.palm_rejection);
    assert!(!disabled.engine_options().palm_rejection);
    assert!(EdgepadConfig::parse("palm_rejection = \"false\"").is_err());
}

#[test]
fn edgepad_config_defaults_tap_sequence_thresholds() {
    let config = EdgepadConfig::parse(
        r#"
            [[gestures]]
            zone = "left"
            direction = "tap"
            action = ["pamixer", "-t"]
            "#,
    )
    .expect("config should parse");

    assert_eq!(config.tap_min_duration_ms, DEFAULT_TAP_MIN_DURATION_MS);
    assert_eq!(config.tap_max_duration_ms, DEFAULT_TAP_MAX_DURATION_MS);
    assert_eq!(config.double_tap_timeout_ms, DEFAULT_DOUBLE_TAP_TIMEOUT_MS);
    assert_eq!(
        config.double_tap_max_distance,
        DEFAULT_DOUBLE_TAP_MAX_DISTANCE
    );
    assert_eq!(config.swipe_min_distance, DEFAULT_SWIPE_MIN_DISTANCE);
}

#[test]
fn edgepad_config_parses_double_tap_binding_and_sequence_thresholds() {
    let config = EdgepadConfig::parse(
        r#"
            tap_min_duration_ms = 30
            tap_max_duration_ms = 220
            double_tap_timeout_ms = 360
            double_tap_max_distance = 0.05

            [[gestures]]
            zone = "top"
            direction = "tap"
            action = ["playerctl", "play-pause"]

            [[gestures]]
            zone = "top"
            direction = "double-tap"
            action = ["playerctl", "stop"]
            "#,
    )
    .expect("double-tap config should parse");

    let options = config.engine_options();
    assert_eq!(options.tap_min_duration, Duration::from_millis(30));
    assert_eq!(options.tap_max_duration, Duration::from_millis(220));
    assert_eq!(options.double_tap_timeout, Duration::from_millis(360));
    assert_eq!(options.double_tap_max_distance, 0.05);
    assert!(options.double_tap_zones.contains(Zone::Top));
    assert!(options.single_tap_zones.contains(Zone::Top));
    assert_eq!(config.gestures[1].direction, GestureDirection::DoubleTap);
}

#[test]
fn invalid_gesture_direction_lists_double_tap_as_an_allowed_value() {
    let result = EdgepadConfig::parse(
        r#"
            [[gestures]]
            zone = "top"
            direction = "doubletap"
            action = { log = true }
            "#,
    );

    assert_eq!(
        result.as_ref().err().map(String::as_str),
        Some("gestures[0].direction must be one of: up, down, left, right, tap, double-tap")
    );
}

#[test]
fn edgepad_config_rejects_invalid_per_edge_width() {
    let err = EdgepadConfig::parse(
        r#"
            edge_width = 0.10
            right_edge_width = 0.50
            "#,
    )
    .expect_err("invalid per-edge width should fail");

    assert_eq!(err, "right_edge_width must be > 0 and < 0.5");
}

#[test]
fn edgepad_config_parses_vertical_slider_with_default_step() {
    let config = EdgepadConfig::parse(
        r#"
            edge_width = 0.20

            [[sliders]]
            zone = "left"
            up = ["pamixer", "-d", "3"]
            down = ["pamixer", "-i", "3"]
            "#,
    )
    .expect("config should parse");

    assert_eq!(config.sliders.len(), 1);
    assert_eq!(config.sliders[0].zone, Zone::Left);
    assert_eq!(config.sliders[0].axis, SliderAxis::Vertical);
    assert_eq!(config.sliders[0].step, DEFAULT_SLIDER_STEP);
    assert_eq!(
        config.sliders[0].negative.argv,
        vec!["pamixer".to_string(), "-d".to_string(), "3".to_string()]
    );
    assert_eq!(
        config.sliders[0].positive.argv,
        vec!["pamixer".to_string(), "-i".to_string(), "3".to_string()]
    );
    assert_eq!(
        config.active_edge_widths(),
        EdgeWidths {
            left: 0.20,
            right: 0.0,
            top: 0.0,
            bottom: 0.0,
        }
    );
    assert_eq!(
        config.slider_specs(),
        vec![SliderSpec {
            zone: Zone::Left,
            axis: SliderAxis::Vertical,
            step: DEFAULT_SLIDER_STEP,
        }]
    );
}

#[test]
fn edgepad_config_parses_horizontal_slider_with_custom_step() {
    let config = EdgepadConfig::parse(
        r#"
            [[sliders]]
            zone = "top"
            step = 0.08
            left = ["playerctl", "previous"]
            right = ["playerctl", "next"]
            "#,
    )
    .expect("config should parse");

    assert_eq!(config.sliders.len(), 1);
    assert_eq!(config.sliders[0].zone, Zone::Top);
    assert_eq!(config.sliders[0].axis, SliderAxis::Horizontal);
    assert_eq!(config.sliders[0].step, 0.08);
}

#[test]
fn edgepad_config_rejects_excessive_tap_min_duration() {
    let result = EdgepadConfig::parse(
        r#"
            tap_min_duration_ms = 10001

            [[gestures]]
            zone = "left"
            direction = "tap"
            action = ["pamixer", "-t"]
            "#,
    );

    assert_eq!(
        result.as_ref().err().map(String::as_str),
        Some("tap_min_duration_ms must be <= 10000")
    );
}

#[test]
fn edgepad_config_rejects_empty_tap_duration_range() {
    let result = EdgepadConfig::parse(
        r#"
            tap_min_duration_ms = 180
            tap_max_duration_ms = 180
            "#,
    );

    assert_eq!(
        result.as_ref().err().map(String::as_str),
        Some("tap_min_duration_ms must be < tap_max_duration_ms")
    );
}

#[test]
fn edgepad_config_rejects_invalid_double_tap_thresholds() {
    let timeout = EdgepadConfig::parse("double_tap_timeout_ms = 0");
    assert_eq!(
        timeout.as_ref().err().map(String::as_str),
        Some("double_tap_timeout_ms must be > 0 and <= 10000")
    );

    let distance = EdgepadConfig::parse("double_tap_max_distance = 0.0");
    assert_eq!(
        distance.as_ref().err().map(String::as_str),
        Some("double_tap_max_distance must be > 0 and <= 1")
    );
}

#[test]
fn edgepad_config_rejects_invalid_swipe_min_distance() {
    let result = EdgepadConfig::parse(
        r#"
            swipe_min_distance = 0.0

            [[gestures]]
            zone = "left"
            direction = "tap"
            action = ["pamixer", "-t"]
            "#,
    );

    assert_eq!(
        result.as_ref().err().map(String::as_str),
        Some("swipe_min_distance must be > 0 and <= 1")
    );
}

#[test]
fn edgepad_config_rejects_duplicate_gesture_binding() {
    let result = EdgepadConfig::parse(
        r#"
            [[gestures]]
            zone = "left"
            direction = "right"
            action = { log = true }

            [[gestures]]
            zone = "left"
            direction = "right"
            action = ["notify-send", "duplicate"]
            "#,
    );

    assert_eq!(
        result.as_ref().err().map(String::as_str),
        Some("duplicate gesture binding left.right")
    );
}

#[test]
fn edgepad_config_rejects_duplicate_slider_zone() {
    let result = EdgepadConfig::parse(
        r#"
            [[sliders]]
            zone = "right"
            up = ["brightnessctl", "set", "5%-"]
            down = ["brightnessctl", "set", "+5%"]

            [[sliders]]
            zone = "right"
            up = ["brightnessctl", "set", "5%-"]
            down = ["brightnessctl", "set", "+5%"]
            "#,
    );

    assert_eq!(
        result.as_ref().err().map(String::as_str),
        Some("duplicate slider binding right")
    );
}

#[test]
fn edgepad_config_rejects_directional_gesture_on_slider_zone() {
    let result = EdgepadConfig::parse(
        r#"
            [[gestures]]
            zone = "left"
            direction = "up"
            action = ["notify-send", "left-up"]

            [[sliders]]
            zone = "left"
            up = ["pamixer", "-d", "3"]
            down = ["pamixer", "-i", "3"]
            "#,
    );

    assert_eq!(
            result.as_ref().err().map(String::as_str),
            Some("gestures[0].direction=up conflicts with slider left; slider zones only allow tap or double-tap gestures")
        );
}

#[test]
fn edgepad_config_allows_tap_gesture_on_slider_zone() {
    let config = EdgepadConfig::parse(
        r#"
            [[gestures]]
            zone = "left"
            direction = "tap"
            action = ["pamixer", "-t"]

            [[sliders]]
            zone = "left"
            up = ["pamixer", "-d", "3"]
            down = ["pamixer", "-i", "3"]
            "#,
    )
    .expect("tap and slider can share a zone");

    assert_eq!(config.gestures.len(), 1);
    assert_eq!(config.sliders.len(), 1);
}

#[test]
fn edgepad_config_allows_double_tap_gesture_on_slider_zone() {
    let config = EdgepadConfig::parse(
        r#"
            [[gestures]]
            zone = "left"
            direction = "double-tap"
            action = ["pamixer", "-t"]

            [[sliders]]
            zone = "left"
            up = ["pamixer", "-d", "3"]
            down = ["pamixer", "-i", "3"]
            "#,
    )
    .expect("double-tap and slider can share a zone");

    assert_eq!(config.gestures.len(), 1);
    assert_eq!(config.sliders.len(), 1);
}

#[test]
fn edgepad_config_rejects_empty_command_action_array() {
    let result = EdgepadConfig::parse(
        r#"
            [[gestures]]
            zone = "top"
            direction = "right"
            action = []
            "#,
    );

    assert_eq!(
        result.as_ref().err().map(String::as_str),
        Some("gestures[0].action: command action requires at least a program")
    );
}
