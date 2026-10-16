use super::super::events::RawEvent;

// These globals describe the contacts forwarded to the virtual device. Physical
// BTN_TOUCH or ABS_X/Y can belong to a swallowed edge contact.
#[derive(Debug, Clone, Default)]
pub(super) struct PointerEmulation {
    contact_count: usize,
    position: Option<(i32, i32)>,
}

impl PointerEmulation {
    pub(super) fn append_changes(
        &mut self,
        contact_count: usize,
        position: Option<(i32, i32)>,
        events: &mut Vec<RawEvent>,
    ) {
        let touch_down = contact_count > 0;
        if (self.contact_count > 0) != touch_down {
            events.push(RawEvent::btn_touch(touch_down));
        }

        if self.contact_count != contact_count {
            if let Some(previous) = tool_event_for_contact_count(self.contact_count, false) {
                events.push(previous);
            }
            if let Some(next) = tool_event_for_contact_count(contact_count, true) {
                events.push(next);
            }
        }

        if let Some((x, y)) = position {
            if self.position.map(|position| position.0) != Some(x) {
                events.push(RawEvent::abs_x(x));
            }
            if self.position.map(|position| position.1) != Some(y) {
                events.push(RawEvent::abs_y(y));
            }
        }

        self.contact_count = contact_count;
        self.position = position;
    }
}

fn tool_event_for_contact_count(count: usize, pressed: bool) -> Option<RawEvent> {
    match count {
        0 => None,
        1 => Some(RawEvent::btn_tool_finger(pressed)),
        2 => Some(RawEvent::btn_tool_doubletap(pressed)),
        3 => Some(RawEvent::btn_tool_tripletap(pressed)),
        4 => Some(RawEvent::btn_tool_quadtap(pressed)),
        _ => Some(RawEvent::btn_tool_quinttap(pressed)),
    }
}
