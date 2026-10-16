use super::*;

#[test]
fn gesture_binding_matches_recognized_gesture() {
    let binding = GestureBindingConfig {
        zone: Zone::Top,
        direction: GestureDirection::Down,
        action: GestureActionConfig::Log,
    };
    let gesture = Gesture {
        zone: Zone::Top,
        direction: GestureDirection::Down,
        slot: 0,
        tracking_id: 10,
    };

    assert!(binding.matches(&gesture));
}
