use edgepad::core::{AxisRange, Capabilities};
use edgepad::raw::{RawEvent, RawOutputSink};
use edgepad::uinput::{build_virtual_touchpad, UinputRawOutputSink, VirtualTouchpadSpec};
use std::thread;
use std::time::Duration;

fn live_test_capabilities() -> Capabilities {
    Capabilities {
        slot_min: 0,
        slot_max: 4,
        x: AxisRange { min: 0, max: 1000 },
        y: AxisRange { min: 0, max: 700 },
    }
}

fn emit_frame(
    sink: &mut UinputRawOutputSink<evdev::uinput::VirtualDevice>,
    events: impl IntoIterator<Item = RawEvent>,
) -> Result<(), String> {
    for event in events {
        sink.emit(event)
            .map_err(|err| format!("failed to buffer raw event: {err:?}"))?;
    }
    sink.sync()
        .map_err(|err| format!("failed to emit uinput frame: {err:?}"))
}

#[test]
#[ignore = "requires /dev/uinput and permission to create virtual input devices"]
fn creates_virtual_touchpad_and_emits_center_contact() -> Result<(), String> {
    let spec = VirtualTouchpadSpec::named(live_test_capabilities(), "edgepad live test touchpad");
    let device = build_virtual_touchpad(&spec).map_err(|err| {
        format!(
            "failed to create virtual touchpad via /dev/uinput; load uinput and check permissions: {err}"
        )
    })?;
    let mut sink = UinputRawOutputSink::new(device);

    thread::sleep(Duration::from_millis(50));

    emit_frame(
        &mut sink,
        [
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(1),
            RawEvent::abs_mt_position_x(500),
            RawEvent::abs_mt_position_y(300),
            RawEvent::btn_touch(true),
            RawEvent::btn_tool_finger(true),
            RawEvent::abs_x(500),
            RawEvent::abs_y(300),
        ],
    )?;

    emit_frame(
        &mut sink,
        [
            RawEvent::abs_mt_slot(0),
            RawEvent::abs_mt_tracking_id(-1),
            RawEvent::btn_touch(false),
            RawEvent::btn_tool_finger(false),
        ],
    )
}
