//! Continuous slider steps and fractional movement retention.

use super::{
    Capabilities, Engine, FrameOutput, Ownership, SliderAxis, SliderDirection, SliderSpec,
    SliderStep, SlotError, SlotState, Zone,
};

impl Engine {
    pub(super) fn emit_slider_steps_if_ready(
        &mut self,
        output: &mut FrameOutput,
    ) -> Result<(), SlotError> {
        let slot = self.current_slot;
        let (zone, spec, position, tracking_id) = {
            let slot_state = self.slot(slot)?;
            if slot_state.gesture_rejected {
                return Ok(());
            }
            let Ownership::Claimed(zone) = slot_state.ownership else {
                return Ok(());
            };
            let Some(spec) = self.slider_for_zone(zone) else {
                return Ok(());
            };
            let Some(position) = slider_position(self.caps, spec.axis, slot_state) else {
                return Ok(());
            };
            let Some(tracking_id) = slot_state.tracking_id else {
                return Ok(());
            };
            (zone, spec, position, tracking_id)
        };

        let slot_state = self.slot_mut(slot)?;
        let Some(mut anchor) = slot_state.slider_anchor else {
            slot_state.slider_anchor = Some(position);
            return Ok(());
        };

        if (position - anchor).abs() >= spec.step {
            self.flush_pending_tap(output);
        }

        let slot_state = self.slot_mut(slot)?;

        // Advance by whole steps, retaining the remainder. Setting the
        // anchor to the latest position would lose small movements and make
        // sensitivity depend on the device's event rate.
        while position - anchor >= spec.step {
            push_slider_step(
                slot_state,
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
                slot_state,
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

        slot_state.slider_anchor = Some(anchor);
        Ok(())
    }

    pub(super) fn slider_for_zone(&self, zone: Zone) -> Option<SliderSpec> {
        self.sliders
            .iter()
            .copied()
            .find(|slider| slider.zone == zone)
    }
}

fn push_slider_step(slot_state: &mut SlotState, output: &mut FrameOutput, step: SliderStep) {
    slot_state.consume_by_slider();
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
    state: &SlotState,
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
