//! Continuous slider steps and fractional movement retention.

use super::{
    Capabilities, Contact, Engine, FrameOutput, Ownership, SliderAxis, SliderDirection, SliderSpec,
    SliderStep, SlotError, Zone,
};

impl Engine {
    pub(super) fn emit_slider_steps_if_ready(
        &mut self,
        output: &mut FrameOutput,
    ) -> Result<(), SlotError> {
        let slot = self.current_slot;
        let (zone, spec, position, tracking_id) = {
            let Some(contact) = &self.slot(slot)?.contact else {
                return Ok(());
            };
            if contact.gesture_rejected {
                return Ok(());
            }
            let Ownership::Claimed(zone) = contact.ownership else {
                return Ok(());
            };
            let Some(spec) = self.slider_for_zone(zone) else {
                return Ok(());
            };
            let Some(position) = slider_position(self.caps, spec.axis, contact) else {
                return Ok(());
            };
            (zone, spec, position, contact.tracking_id)
        };

        let contact = self
            .slot_mut(slot)?
            .contact
            .as_mut()
            .expect("slider contact remains active");
        let Some(mut anchor) = contact.slider_anchor else {
            contact.slider_anchor = Some(position);
            return Ok(());
        };

        let moved_by_step = (position - anchor).abs() >= spec.step;

        // Advance by whole steps, retaining the remainder. Setting the
        // anchor to the latest position would lose small movements and make
        // sensitivity depend on the device's event rate.
        while position - anchor >= spec.step {
            push_slider_step(
                contact,
                output,
                SliderStep {
                    zone,
                    direction: positive_slider_direction(spec.axis),
                    slot,
                    tracking_id,
                },
            );
            anchor += spec.step;
        }

        while anchor - position >= spec.step {
            push_slider_step(
                contact,
                output,
                SliderStep {
                    zone,
                    direction: negative_slider_direction(spec.axis),
                    slot,
                    tracking_id,
                },
            );
            anchor -= spec.step;
        }

        contact.slider_anchor = Some(anchor);
        if moved_by_step {
            self.flush_pending_tap(output);
        }
        Ok(())
    }

    pub(super) fn slider_for_zone(&self, zone: Zone) -> Option<SliderSpec> {
        self.sliders
            .iter()
            .copied()
            .find(|slider| slider.zone == zone)
    }
}

fn push_slider_step(contact: &mut Contact, output: &mut FrameOutput, step: SliderStep) {
    // Even a step smaller than tap slop commits the contact to the slider.
    // Lifting or returning to the start must not also fire a tap.
    contact.consume_by_slider();
    output.slider_steps.push(step);
}

pub(super) fn validate_slider_specs(sliders: &[SliderSpec]) {
    for slider in sliders {
        assert!(
            slider.step.is_finite() && slider.step > 0.0 && slider.step <= 1.0,
            "invalid slider step for {:?}: {}",
            slider.zone,
            slider.step
        );
    }
}

pub(super) fn slider_position(
    caps: Capabilities,
    axis: SliderAxis,
    state: &Contact,
) -> Option<f32> {
    match axis {
        SliderAxis::Horizontal => state.current_x.map(|x| caps.x.normalize(x)),
        SliderAxis::Vertical => state.current_y.map(|y| caps.y.normalize(y)),
    }
}

fn positive_slider_direction(axis: SliderAxis) -> SliderDirection {
    match axis {
        SliderAxis::Horizontal => SliderDirection::Right,
        SliderAxis::Vertical => SliderDirection::Down,
    }
}

fn negative_slider_direction(axis: SliderAxis) -> SliderDirection {
    match axis {
        SliderAxis::Horizontal => SliderDirection::Left,
        SliderAxis::Vertical => SliderDirection::Up,
    }
}
