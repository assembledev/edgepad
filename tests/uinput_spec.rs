use edgepad::core::{AxisRange, Capabilities};
use edgepad::raw::{
    ABS_MT_POSITION_X, ABS_MT_POSITION_Y, ABS_MT_SLOT, ABS_MT_TRACKING_ID, ABS_X, ABS_Y,
    BTN_TOOL_DOUBLETAP, BTN_TOOL_FINGER, BTN_TOOL_QUADTAP, BTN_TOOL_QUINTTAP, BTN_TOOL_TRIPLETAP,
    BTN_TOUCH,
};
use edgepad::uinput::{VirtualAbsAxis, VirtualTouchpadSpec};
use evdev::PropType;

fn test_capabilities() -> Capabilities {
    Capabilities {
        slot_min: 0,
        slot_max: 4,
        x: AxisRange { min: 10, max: 1210 },
        y: AxisRange { min: 20, max: 820 },
    }
}

#[test]
fn virtual_touchpad_spec_mirrors_composer_output_capabilities() {
    let spec = VirtualTouchpadSpec::from_capabilities(test_capabilities());

    assert_eq!(spec.name, "edgepad virtual touchpad");
    assert_eq!(spec.properties, vec![PropType::POINTER.0]);
    assert_eq!(
        spec.keys,
        vec![
            BTN_TOUCH,
            BTN_TOOL_FINGER,
            BTN_TOOL_DOUBLETAP,
            BTN_TOOL_TRIPLETAP,
            BTN_TOOL_QUADTAP,
            BTN_TOOL_QUINTTAP,
        ]
    );
    assert_eq!(
        spec.absolute_axes,
        vec![
            VirtualAbsAxis::new(ABS_X, 10, 1210),
            VirtualAbsAxis::new(ABS_Y, 20, 820),
            VirtualAbsAxis::new(ABS_MT_SLOT, 0, 4),
            VirtualAbsAxis::new(ABS_MT_TRACKING_ID, 0, 65_535),
            VirtualAbsAxis::new(ABS_MT_POSITION_X, 10, 1210),
            VirtualAbsAxis::new(ABS_MT_POSITION_Y, 20, 820),
        ]
    );
    assert!(spec.misc.is_empty());
}

#[test]
fn virtual_touchpad_spec_can_use_a_custom_public_name() {
    let spec = VirtualTouchpadSpec::named(test_capabilities(), "edgepad test device");

    assert_eq!(spec.name, "edgepad test device");
}
