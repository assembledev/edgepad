use edgepad::core::{AxisRange, Capabilities};
use edgepad::raw::{
    ABS_MT_POSITION_X, ABS_MT_POSITION_Y, ABS_MT_SLOT, ABS_MT_TRACKING_ID, ABS_X, ABS_Y, BTN_LEFT,
    BTN_RIGHT, BTN_TOOL_DOUBLETAP, BTN_TOOL_FINGER, BTN_TOOL_QUADTAP, BTN_TOOL_QUINTTAP,
    BTN_TOOL_TRIPLETAP, BTN_TOUCH,
};
use edgepad::uinput::{PhysicalTouchpadAbsInfo, VirtualAbsAxis, VirtualTouchpadSpec};
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

#[test]
fn virtual_touchpad_spec_preserves_physical_buttonpad_capabilities() {
    let spec = VirtualTouchpadSpec::from_physical_device_info(
        PhysicalTouchpadAbsInfo::from_capabilities(test_capabilities()),
        vec![PropType::POINTER.0, PropType::BUTTONPAD.0],
        vec![BTN_LEFT, BTN_RIGHT, 30],
        "edgepad mirrored buttonpad",
    );

    assert_eq!(
        spec.properties,
        vec![PropType::POINTER.0, PropType::BUTTONPAD.0]
    );
    assert!(spec.keys.contains(&BTN_LEFT));
    assert!(spec.keys.contains(&BTN_RIGHT));
    assert!(
        !spec.keys.contains(&30),
        "keyboard keys must not be mirrored"
    );
}

#[test]
fn virtual_touchpad_spec_preserves_physical_abs_resolution_for_pointer_speed() {
    let spec = VirtualTouchpadSpec::from_physical_abs_info(
        PhysicalTouchpadAbsInfo {
            abs_x: Some(VirtualAbsAxis {
                code: ABS_X,
                value: 400,
                min: 10,
                max: 1210,
                fuzz: 1,
                flat: 2,
                resolution: 31,
            }),
            abs_y: Some(VirtualAbsAxis {
                code: ABS_Y,
                value: 300,
                min: 20,
                max: 820,
                fuzz: 3,
                flat: 4,
                resolution: 32,
            }),
            mt_slot: VirtualAbsAxis {
                code: ABS_MT_SLOT,
                value: 0,
                min: 0,
                max: 4,
                fuzz: 0,
                flat: 0,
                resolution: 0,
            },
            mt_tracking_id: Some(VirtualAbsAxis {
                code: ABS_MT_TRACKING_ID,
                value: 0,
                min: 0,
                max: 65535,
                fuzz: 0,
                flat: 0,
                resolution: 0,
            }),
            mt_position_x: VirtualAbsAxis {
                code: ABS_MT_POSITION_X,
                value: 400,
                min: 10,
                max: 1210,
                fuzz: 5,
                flat: 6,
                resolution: 41,
            },
            mt_position_y: VirtualAbsAxis {
                code: ABS_MT_POSITION_Y,
                value: 300,
                min: 20,
                max: 820,
                fuzz: 7,
                flat: 8,
                resolution: 42,
            },
        },
        "edgepad mirrored touchpad",
    );

    assert_eq!(spec.name, "edgepad mirrored touchpad");
    assert_eq!(
        spec.absolute_axes,
        vec![
            VirtualAbsAxis {
                code: ABS_X,
                value: 400,
                min: 10,
                max: 1210,
                fuzz: 1,
                flat: 2,
                resolution: 31,
            },
            VirtualAbsAxis {
                code: ABS_Y,
                value: 300,
                min: 20,
                max: 820,
                fuzz: 3,
                flat: 4,
                resolution: 32,
            },
            VirtualAbsAxis::new(ABS_MT_SLOT, 0, 4),
            VirtualAbsAxis::new(ABS_MT_TRACKING_ID, 0, 65535),
            VirtualAbsAxis {
                code: ABS_MT_POSITION_X,
                value: 400,
                min: 10,
                max: 1210,
                fuzz: 5,
                flat: 6,
                resolution: 41,
            },
            VirtualAbsAxis {
                code: ABS_MT_POSITION_Y,
                value: 300,
                min: 20,
                max: 820,
                fuzz: 7,
                flat: 8,
                resolution: 42,
            },
        ]
    );
}
