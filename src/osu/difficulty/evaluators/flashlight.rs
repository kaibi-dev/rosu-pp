use std::cmp;

use crate::{
    any::difficulty::object::IDifficultyObject,
    osu::{difficulty::object::OsuDifficultyObject, object::OsuObjectKind},
};

pub struct FlashlightEvaluator;

impl FlashlightEvaluator {
    const MAX_OPACITY_BONUS: f64 = 0.4;
    const HIDDEN_BONUS: f64 = 0.2;

    const MIN_VELOCITY: f64 = 0.5;
    const SLIDER_MULTIPLIER: f64 = 1.3;

    const MIN_ANGLE_MULTIPLIER: f64 = 0.2;

    pub fn evaluate_diff_of<'a>(
        curr: &'a OsuDifficultyObject<'a>,
        diff_objects: &'a [OsuDifficultyObject<'a>],
        hidden: bool,
        hidden_bonus: bool,
    ) -> f64 {
        if curr.base.is_spinner() {
            return 0.0;
        }

        let osu_current = curr;
        let osu_hit_obj = curr.base;

        let scaling_factor = 52.0 / osu_current.radius;
        let mut small_dist_nerf = 1.0;
        let mut cumulative_strain_time = 0.0;

        let mut flashlight_difficulty = 0.0;

        let mut last_obj = osu_current;

        let mut angle_repeat_count = 0.0;

        // * This is iterating backwards in time from the current object.
        for i in 0..cmp::min(curr.idx, 10) {
            let Some(current_obj) = curr.previous(i, diff_objects) else {
                break;
            };

            let current_hit_object = current_obj.base;

            cumulative_strain_time += last_obj.adjusted_delta_time;

            if !current_obj.base.is_spinner() {
                let jump_distance = f64::from(
                    (osu_hit_obj.stacked_pos() - current_hit_object.stacked_end_pos()).length(),
                );

                // * We want to nerf objects that can be easily seen within the Flashlight circle radius.
                if i == 0 {
                    small_dist_nerf = (jump_distance / 75.0).min(1.0);
                }

                // * We also want to nerf stacks so that only the first object of the stack is accounted for.
                let stack_nerf = ((current_obj.lazy_jump_dist / scaling_factor) / 25.0).min(1.0);

                // * Bonus based on how visible the object is.
                let opacity_bonus = 1.0
                    + Self::MAX_OPACITY_BONUS
                        * (1.0 - osu_current.opacity_at(current_hit_object.start_time, hidden));

                flashlight_difficulty +=
                    stack_nerf * opacity_bonus * scaling_factor * jump_distance
                        / cumulative_strain_time;

                if let Some((current_obj_angle, osu_curr_angle)) =
                    current_obj.angle.zip(osu_current.angle)
                {
                    // * Objects further back in time should count less for the nerf.
                    if (current_obj_angle - osu_curr_angle).abs() < 0.02 {
                        angle_repeat_count += (1.0 - 0.1 * i as f64).max(0.0);
                    }
                }
            }

            last_obj = current_obj;
        }

        flashlight_difficulty = (small_dist_nerf * flashlight_difficulty).powi(2);

        // * Additional bonus for Hidden due to there being no approach circles.
        if hidden_bonus {
            flashlight_difficulty *= 1.0 + Self::HIDDEN_BONUS;
        }

        // * Nerf patterns with repeated angles.
        flashlight_difficulty *= Self::MIN_ANGLE_MULTIPLIER
            + (1.0 - Self::MIN_ANGLE_MULTIPLIER) / (angle_repeat_count + 1.0);

        let mut slider_bonus = 0.0;

        if let OsuObjectKind::Slider(slider) = &osu_current.base.kind {
            // * Invert the scaling factor to determine the true travel distance independent of circle size.
            let pixel_travel_distance = osu_current.lazy_travel_dist / scaling_factor;

            // * Reward sliders based on velocity.
            slider_bonus = ((pixel_travel_distance / osu_current.travel_time - Self::MIN_VELOCITY)
                .max(0.0))
            .powf(0.5);

            // * Longer sliders require more memorisation.
            slider_bonus *= pixel_travel_distance;

            // * Nerf sliders with repeats, as less memorisation is required.
            let repeat_count = slider.repeat_count();

            if repeat_count > 0 {
                slider_bonus /= (repeat_count + 1) as f64;
            }
        }

        flashlight_difficulty += slider_bonus * Self::SLIDER_MULTIPLIER;

        flashlight_difficulty
    }
}
