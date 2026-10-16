use super::*;
use crate::actions::test_support::{gesture, slider_step};

#[test]
fn action_command_context_includes_gesture_identity() {
    let command = ActionCommand {
        source: ActionSource::Gesture(gesture(Zone::Right, GestureDirection::Up)),
        argv: vec!["notify-send".to_string(), "edgepad".to_string()],
    };

    assert_eq!(
        command.source.to_string(),
        "zone=right direction=up slot=0 tracking_id=42"
    );
}

#[test]
fn gesture_context_includes_gesture_identity() {
    assert_eq!(
        ActionSource::Gesture(gesture(Zone::Top, GestureDirection::Left)).to_string(),
        "zone=top direction=left slot=0 tracking_id=42"
    );
}

#[test]
fn slider_step_context_includes_slider_identity() {
    assert_eq!(
        ActionSource::SliderStep(slider_step(Zone::Right, SliderDirection::Up)).to_string(),
        "zone=right direction=up slot=0 tracking_id=42"
    );
}
