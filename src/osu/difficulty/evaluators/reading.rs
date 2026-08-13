use std::f64::consts::PI;

use crate::{
    any::difficulty::object::IDifficultyObject,
    osu::difficulty::object::OsuDifficultyObject,
    util::{
        difficulty::{norm, reverse_lerp, smootherstep},
        float_ext::FloatExt,
    },
};

pub struct ReadingEvaluator;

impl ReadingEvaluator {
    const READING_WINDOW_SIZE: f64 = 3000.0; // * 3 seconds
    const DISTANCE_INFLUENCE_THRESHOLD: f64 = OsuDifficultyObject::NORMALIZED_DIAMETER as f64 * 1.5; // * 1.5 circles distance between centers

    pub fn evaluate_diff_of<'a>(
        curr: &'a OsuDifficultyObject<'a>,
        diff_objects: &'a [OsuDifficultyObject<'a>],
        hidden: bool,
    ) -> f64 {
        if curr.base.is_spinner() || curr.idx == 0 {
            return 0.0;
        }

        let curr_obj = curr;
        let next_obj = curr.next(0, diff_objects);

        let velocity = 1.0_f64.max(curr_obj.lazy_jump_dist / curr_obj.adjusted_delta_time); // * Only allow velocity to buff

        let current_visible_object_density =
            retrieve_current_visible_object_density(curr_obj, diff_objects);
        let past_object_difficulty_influence =
            get_past_object_difficulty_influence(curr_obj, diff_objects);

        let constant_angle_nerf_factor = get_constant_angle_nerf_factor(curr_obj, diff_objects);

        let note_density_difficulty = calculate_density_difficulty(
            next_obj,
            velocity,
            constant_angle_nerf_factor,
            past_object_difficulty_influence,
            current_visible_object_density,
        );

        let hidden_difficulty = if hidden {
            calculate_hidden_difficulty(
                curr_obj,
                diff_objects,
                past_object_difficulty_influence,
                current_visible_object_density,
                velocity,
                constant_angle_nerf_factor,
            )
        } else {
            0.0
        };

        let preempt_difficulty =
            calculate_preempt_difficulty(velocity, constant_angle_nerf_factor, curr_obj.preempt);

        let mut reading_difficulty = norm(
            1.5,
            [
                preempt_difficulty,
                hidden_difficulty,
                note_density_difficulty,
            ],
        );

        // * Having less time to process information is harder
        reading_difficulty *= high_bpm_bonus(curr_obj.adjusted_delta_time);

        reading_difficulty
    }
}

fn calculate_density_difficulty(
    next_obj: Option<&OsuDifficultyObject<'_>>,
    velocity: f64,
    constant_angle_nerf_factor: f64,
    past_object_difficulty_influence: f64,
    current_visible_object_density: f64,
) -> f64 {
    const DENSITY_MULTIPLIER: f64 = 2.4;
    const DENSITY_DIFFICULTY_BASE: f64 = 2.5;

    // * Consider future densities too because it can make the path the cursor takes less clear
    let mut future_object_difficulty_influence = current_visible_object_density.sqrt();

    if let Some(next_obj) = next_obj {
        // * Reduce difficulty if movement to next object is small
        future_object_difficulty_influence *= smootherstep(
            next_obj.lazy_jump_dist,
            15.0,
            ReadingEvaluator::DISTANCE_INFLUENCE_THRESHOLD,
        );
    }

    // * Value higher note densities exponentially
    let mut note_density_difficulty =
        (past_object_difficulty_influence + future_object_difficulty_influence).powf(1.7)
            * 0.4
            * constant_angle_nerf_factor
            * velocity;

    // * Award only denser than average maps.
    note_density_difficulty = 0.0_f64.max(note_density_difficulty - DENSITY_DIFFICULTY_BASE);

    // * Apply a soft cap to general density reading to account for partial memorization
    note_density_difficulty.powf(0.45) * DENSITY_MULTIPLIER
}

fn calculate_preempt_difficulty(
    velocity: f64,
    constant_angle_nerf_factor: f64,
    preempt: f64,
) -> f64 {
    const PREEMPT_BALANCING_FACTOR: f64 = 140_000.0;
    const PREEMPT_STARTING_POINT: f64 = 500.0; // * AR 9.66 in milliseconds

    // * Arbitrary curve for the base value preempt difficulty should have as approach rate increases.
    // * https://www.desmos.com/calculator/c175335a71
    let mut preempt_difficulty =
        ((PREEMPT_STARTING_POINT - preempt + (preempt - PREEMPT_STARTING_POINT).abs()) / 2.0)
            .powf(2.5)
            / PREEMPT_BALANCING_FACTOR;

    preempt_difficulty *= constant_angle_nerf_factor * velocity;

    preempt_difficulty
}

fn calculate_hidden_difficulty(
    curr_obj: &OsuDifficultyObject<'_>,
    diff_objects: &[OsuDifficultyObject<'_>],
    past_object_difficulty_influence: f64,
    current_visible_object_density: f64,
    velocity: f64,
    constant_angle_nerf_factor: f64,
) -> f64 {
    const HIDDEN_MULTIPLIER: f64 = 0.28;

    // * Higher preempt means that time spent invisible is higher too, we want to reward that
    let preempt_factor = curr_obj.preempt.powf(2.2) * 0.01;

    // * Account for both past and current densities
    let density_factor =
        (current_visible_object_density + past_object_difficulty_influence).powf(3.3) * 3.0;

    let mut hidden_difficulty =
        (preempt_factor + density_factor) * constant_angle_nerf_factor * velocity * 0.01;

    // * Apply a soft cap to general HD reading to account for partial memorization
    hidden_difficulty = hidden_difficulty.powf(0.4) * HIDDEN_MULTIPLIER;

    if let Some(previous_obj) = curr_obj.previous(0, diff_objects) {
        // * Buff perfect stacks only if current note is completely invisible at the time you click the previous note.
        if curr_obj.lazy_jump_dist == 0.0
            && curr_obj.opacity_at(previous_obj.base.start_time, true) == 0.0
            && previous_obj.start_time > curr_obj.start_time - curr_obj.preempt
        {
            hidden_difficulty +=
                HIDDEN_MULTIPLIER * 2500.0 / curr_obj.adjusted_delta_time.powf(1.5);
            // * Perfect stacks are harder the less time between notes
        }
    }

    hidden_difficulty
}

fn get_past_object_difficulty_influence(
    curr_obj: &OsuDifficultyObject<'_>,
    diff_objects: &[OsuDifficultyObject<'_>],
) -> f64 {
    let mut past_object_difficulty_influence = 0.0;

    for i in 0..curr_obj.idx {
        let Some(loop_obj) = curr_obj.previous(i, diff_objects) else {
            break;
        };

        if curr_obj.start_time - loop_obj.start_time > ReadingEvaluator::READING_WINDOW_SIZE
            || loop_obj.start_time < curr_obj.start_time - curr_obj.preempt
        {
            // * Current object not visible at the time object needs to be clicked
            break;
        }

        let mut loop_difficulty = curr_obj.opacity_at(loop_obj.base.start_time, false);

        // * When aiming an object small distances mean previous objects may be cheesed, so it doesn't matter whether they were arranged confusingly.
        loop_difficulty *= smootherstep(
            loop_obj.lazy_jump_dist,
            15.0,
            ReadingEvaluator::DISTANCE_INFLUENCE_THRESHOLD,
        );

        // * Account less for objects close to the max reading window
        let time_between_curr_and_loop_obj = curr_obj.start_time - loop_obj.start_time;
        let time_nerf_factor = get_time_nerf_factor(time_between_curr_and_loop_obj);

        loop_difficulty *= time_nerf_factor;
        past_object_difficulty_influence += loop_difficulty;
    }

    past_object_difficulty_influence
}

fn retrieve_current_visible_object_density(
    current: &OsuDifficultyObject<'_>,
    diff_objects: &[OsuDifficultyObject<'_>],
) -> f64 {
    let mut visible_object_count = 0.0;

    let mut hit_object = current.next(0, diff_objects);

    while let Some(obj) = hit_object {
        if obj.start_time - current.start_time > ReadingEvaluator::READING_WINDOW_SIZE
            || current.start_time < obj.start_time - obj.preempt
        {
            // * Object not visible at the time current object needs to be clicked.
            break;
        }

        let time_between_curr_and_loop_obj = obj.start_time - current.start_time;
        let time_nerf_factor = get_time_nerf_factor(time_between_curr_and_loop_obj);

        visible_object_count += obj.opacity_at(current.base.start_time, false) * time_nerf_factor;

        hit_object = obj.next(0, diff_objects);
    }

    visible_object_count
}

fn get_constant_angle_nerf_factor(
    current: &OsuDifficultyObject<'_>,
    diff_objects: &[OsuDifficultyObject<'_>],
) -> f64 {
    const MINIMUM_ANGLE_RELEVANCY_TIME: f64 = 2000.0; // * 2 seconds
    const MAXIMUM_ANGLE_RELEVANCY_TIME: f64 = 200.0;

    let mut constant_angle_count = 0.0;
    let mut index = 0usize;
    let mut current_time_gap = 0.0;

    let mut loop_obj_prev0 = current;
    let mut loop_obj_prev1: Option<&OsuDifficultyObject<'_>> = None;
    let mut loop_obj_prev2: Option<&OsuDifficultyObject<'_>> = None;

    while current_time_gap < MINIMUM_ANGLE_RELEVANCY_TIME {
        let Some(loop_obj) = current.previous(index, diff_objects) else {
            break;
        };

        // * Account less for objects that are close to the time limit.
        let long_interval_factor = 1.0
            - reverse_lerp(
                loop_obj.adjusted_delta_time,
                MAXIMUM_ANGLE_RELEVANCY_TIME,
                MINIMUM_ANGLE_RELEVANCY_TIME,
            );

        if let Some((loop_angle, current_angle)) = loop_obj.angle.zip(current.angle) {
            let angle_difference = (current_angle - loop_angle).abs();
            let mut angle_difference_alternating = PI;

            if let (Some(prev0_angle), Some(prev1), Some(prev2)) =
                (loop_obj_prev0.angle, loop_obj_prev1, loop_obj_prev2)
                && let (Some(prev1_angle), Some(prev2_angle)) = (prev1.angle, prev2.angle)
            {
                angle_difference_alternating = (prev1_angle - loop_angle).abs();
                angle_difference_alternating += (prev2_angle - prev0_angle).abs();

                let mut weight = 1.0;

                // * Be sure that one of the angles is very sharp, when other is wide
                weight *= reverse_lerp(loop_angle.min(prev0_angle) * 180.0 / PI, 20.0, 5.0);
                weight *= reverse_lerp(loop_angle.max(prev0_angle) * 180.0 / PI, 60.0, 120.0);

                // * Lerp between max angle difference and rescaled alternating difference, with more harsh scaling compared to normal difference
                angle_difference_alternating =
                    f64::lerp(PI, 0.1 * angle_difference_alternating, weight);
            }

            let stack_factor = smootherstep(
                loop_obj.lazy_jump_dist,
                0.0,
                f64::from(OsuDifficultyObject::NORMALIZED_RADIUS),
            );

            constant_angle_count += f64::cos(
                3.0 * f64::to_radians(30.0)
                    .min(angle_difference.min(angle_difference_alternating) * stack_factor),
            ) * long_interval_factor;
        }

        current_time_gap = current.start_time - loop_obj.start_time;
        index += 1;

        loop_obj_prev2 = loop_obj_prev1;
        loop_obj_prev1 = Some(loop_obj_prev0);
        loop_obj_prev0 = loop_obj;
    }

    (2.0 / constant_angle_count).clamp(0.2, 1.0)
}

fn get_time_nerf_factor(delta_time: f64) -> f64 {
    (2.0 - delta_time / (ReadingEvaluator::READING_WINDOW_SIZE / 2.0)).clamp(0.0, 1.0)
}

fn high_bpm_bonus(ms: f64) -> f64 {
    1.0 / (1.0 - 0.8_f64.powf(ms / 1000.0))
}
