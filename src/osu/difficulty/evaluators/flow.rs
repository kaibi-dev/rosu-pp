use crate::{
    any::difficulty::object::IDifficultyObject,
    osu::difficulty::{evaluators::snap::SnapAimEvaluator, object::OsuDifficultyObject},
    util::difficulty::{smootherstep, smoothstep},
};

pub struct FlowAimEvaluator;

impl FlowAimEvaluator {
    const VELOCITY_CHANGE_MULTIPLIER: f64 = 0.52;

    pub fn evaluate_diff_of<'a>(
        curr: &'a OsuDifficultyObject<'a>,
        diff_objects: &'a [OsuDifficultyObject<'a>],
        with_slider_travel_dist: bool,
    ) -> f64 {
        let osu_curr_obj = curr;

        let Some(osu_last_obj) = curr.previous(0, diff_objects) else {
            return 0.0;
        };

        if curr.base.is_spinner() || curr.idx <= 1 || osu_last_obj.base.is_spinner() {
            return 0.0;
        }

        let osu_last_last_obj = curr.previous(1, diff_objects);

        let curr_distance = if with_slider_travel_dist {
            osu_curr_obj.lazy_jump_dist
        } else {
            osu_curr_obj.jump_distance
        };
        let prev_distance = if with_slider_travel_dist {
            osu_last_obj.lazy_jump_dist
        } else {
            osu_last_obj.jump_distance
        };

        let mut curr_velocity = curr_distance / osu_curr_obj.adjusted_delta_time;

        if osu_last_obj.base.is_slider() && with_slider_travel_dist {
            // * If the last object is a slider, then we extend the travel velocity through the slider into the current object.
            let slider_distance = osu_last_obj.lazy_travel_dist + osu_curr_obj.lazy_jump_dist;
            curr_velocity = curr_velocity.max(slider_distance / osu_curr_obj.adjusted_delta_time);
        }

        let prev_velocity = prev_distance / osu_last_obj.adjusted_delta_time;

        let mut flow_difficulty = curr_velocity;

        // * Apply high circle size bonus to the base velocity.
        // * We use reduced CS bonus here because the bonus was made for an evaluator with a different d/t scaling
        flow_difficulty *= osu_curr_obj.small_circle_bonus.sqrt();

        // * Rhythm changes are harder to flow
        flow_difficulty *= 1.0
            + f64::min(
                0.25,
                ((osu_curr_obj
                    .adjusted_delta_time
                    .max(osu_last_obj.adjusted_delta_time)
                    - osu_curr_obj
                        .adjusted_delta_time
                        .min(osu_last_obj.adjusted_delta_time))
                    / 50.0)
                    .powi(4),
            );

        if let Some((curr_angle, last_angle)) = osu_curr_obj.angle.zip(osu_last_obj.angle) {
            let angle_difference = (curr_angle - last_angle).abs();
            let angle_difference_adjusted = (angle_difference / 2.0).sin() * 180.0;
            let angular_velocity =
                angle_difference_adjusted / (osu_curr_obj.adjusted_delta_time * 0.1);

            // * Low angular velocity flow (angles are consistent) is easier to follow than erratic flow
            flow_difficulty *= 0.8 + (angular_velocity / 270.0).sqrt();
        }

        // * If all three notes are overlapping - don't reward bonuses as you don't have to do additional movement
        let mut overlapped_notes_weight = 1.0;

        if curr.idx > 2
            && let Some(osu_last_last_obj) = osu_last_last_obj
        {
            let o1 = calculate_overlap_factor(osu_curr_obj, osu_last_obj);
            let o2 = calculate_overlap_factor(osu_curr_obj, osu_last_last_obj);
            let o3 = calculate_overlap_factor(osu_last_obj, osu_last_last_obj);

            overlapped_notes_weight = 1.0 - o1 * o2 * o3;
        }

        if let Some(curr_angle) = osu_curr_obj.angle {
            // * Acute angles are also hard to flow
            flow_difficulty += curr_velocity
                * SnapAimEvaluator::calc_angle_acuteness(curr_angle)
                * overlapped_notes_weight;
        }

        if prev_velocity.max(curr_velocity) != 0.0 {
            let curr_velocity = if with_slider_travel_dist {
                curr_distance / osu_curr_obj.adjusted_delta_time
            } else {
                curr_velocity
            };

            // * Scale with ratio of difference compared to 0.5 * max dist.
            let dist_ratio = smoothstep(
                (prev_velocity - curr_velocity).abs() / prev_velocity.max(curr_velocity),
                0.0,
                1.0,
            );

            // * Reward for % distance up to 125 / strainTime for overlaps where velocity is still changing.
            let overlap_velocity_buff = (f64::from(OsuDifficultyObject::NORMALIZED_DIAMETER)
                * 1.25
                / osu_curr_obj
                    .adjusted_delta_time
                    .min(osu_last_obj.adjusted_delta_time))
            .min((prev_velocity - curr_velocity).abs());

            flow_difficulty += overlap_velocity_buff
                * dist_ratio
                * overlapped_notes_weight
                * Self::VELOCITY_CHANGE_MULTIPLIER;
        }

        if osu_curr_obj.base.is_slider() && with_slider_travel_dist {
            // * Include slider velocity to make velocity more consistent with snap
            flow_difficulty += osu_curr_obj.travel_dist / osu_curr_obj.travel_time;
        }

        // * Final velocity is being raised to a power because flow difficulty scales harder with both high distance and time, and we want to account for that
        flow_difficulty = flow_difficulty.powf(1.45);

        // * Reduce difficulty for low spacing since spacing below radius is always to be flowed
        flow_difficulty
            * smootherstep(
                curr_distance,
                0.0,
                f64::from(OsuDifficultyObject::NORMALIZED_RADIUS),
            )
    }
}

fn calculate_overlap_factor(
    first: &OsuDifficultyObject<'_>,
    second: &OsuDifficultyObject<'_>,
) -> f64 {
    let object_radius = first.radius;

    let distance = f64::from((first.base.stacked_pos() - second.base.stacked_pos()).length());

    (1.0 - ((distance - object_radius).max(0.0) / object_radius).powi(2)).clamp(0.0, 1.0)
}
