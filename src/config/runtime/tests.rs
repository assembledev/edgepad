use crate::config::*;
use crate::core::EdgeWidths;

#[test]
fn edgepad_config_activates_only_zones_with_gesture_bindings() {
    let config = EdgepadConfig::parse(
        r#"
            edge_width = 0.20

            [[gestures]]
            zone = "right"
            direction = "up"
            action = ["notify-send", "right-up"]

            [[gestures]]
            zone = "top"
            direction = "left"
            action = ["notify-send", "top-left"]
            "#,
    )
    .expect("config should parse");

    assert_eq!(
        config.active_edge_widths(),
        EdgeWidths {
            left: 0.0,
            right: 0.20,
            top: 0.20,
            bottom: 0.0,
        }
    );
}

#[test]
fn edgepad_config_applies_optional_per_edge_widths_with_global_fallback() {
    let config = EdgepadConfig::parse(
        r#"
            edge_width = 0.10
            left_edge_width = 0.08
            top_edge_width = 0.16

            [[gestures]]
            zone = "left"
            direction = "down"
            action = { log = true }

            [[gestures]]
            zone = "right"
            direction = "down"
            action = { log = true }

            [[gestures]]
            zone = "top"
            direction = "right"
            action = { log = true }
            "#,
    )
    .expect("config should parse");

    assert_eq!(
        config.edge_width_overrides,
        EdgeWidthOverrides {
            left: Some(0.08),
            right: None,
            top: Some(0.16),
            bottom: None,
        }
    );
    assert_eq!(
        config.configured_edge_widths(),
        EdgeWidths {
            left: 0.08,
            right: 0.10,
            top: 0.16,
            bottom: 0.10,
        }
    );
    assert_eq!(
        config.active_edge_widths(),
        EdgeWidths {
            left: 0.08,
            right: 0.10,
            top: 0.16,
            bottom: 0.0,
        }
    );
}

#[test]
fn global_edge_width_override_clears_per_edge_widths() {
    let mut config = EdgepadConfig::parse(
        r#"
            edge_width = 0.10
            left_edge_width = 0.08
            right_edge_width = 0.16
            "#,
    )
    .expect("config should parse");

    config.override_edge_width(0.20);

    assert_eq!(config.edge_width, 0.20);
    assert_eq!(config.edge_width_overrides, EdgeWidthOverrides::default());
    assert_eq!(config.configured_edge_widths(), EdgeWidths::all(0.20));
}
