use std::cmp;

use crate::{
    any::difficulty::object::IDifficultyObject,
    osu::difficulty::object::OsuDifficultyObject,
    util::difficulty::{logistic, reverse_lerp, smoothstep_bell_curve_unit},
};

pub struct RhythmEvaluator;

impl RhythmEvaluator {
    const HISTORY_TIME_MAX: u32 = 5 * 1000; // * 5 seconds
    const HISTORY_OBJECTS_MAX: usize = 32;
    const RHYTHM_OVERALL_MULTIPLIER: f64 = 0.95;

    #[expect(clippy::too_many_lines, reason = "staying in-sync with lazer")]
    pub fn evaluate_diff_of<'a>(
        curr: &'a OsuDifficultyObject<'a>,
        diff_objects: &'a [OsuDifficultyObject<'a>],
    ) -> f64 {
        if curr.base.is_spinner() {
            return 0.0;
        }

        let mut rhythm_complexity_sum = 0.0;

        let delta_difference_epsilon = curr.hit_window_great * 0.3;

        let mut island = Island::new(i32::MAX);
        let mut previous_island = Island::new(i32::MAX);

        let mut islands = Vec::<Island>::new();

        let mut start_difficulty = 0.0; // * store the difficulty of the current start of an island to buff for tighter rhythms

        let mut first_delta_switch = false;

        let historical_note_count = cmp::min(curr.idx, Self::HISTORY_OBJECTS_MAX);

        let mut rhythm_start = 0;

        while curr
            .previous(rhythm_start, diff_objects)
            .filter(|prev| {
                rhythm_start + 2 < historical_note_count
                    && curr.start_time - prev.start_time < f64::from(Self::HISTORY_TIME_MAX)
            })
            .is_some()
        {
            rhythm_start += 1;
        }

        let Some((mut prev_obj, mut prev_prev_obj)) = curr
            .previous(rhythm_start, diff_objects)
            .zip(curr.previous(rhythm_start + 1, diff_objects))
        else {
            return (4.0 + rhythm_complexity_sum * Self::RHYTHM_OVERALL_MULTIPLIER).sqrt() / 2.0;
        };

        // * we go from the furthest object back to the current one
        for i in (1..=rhythm_start).rev() {
            let Some(curr_obj) = curr.previous(i - 1, diff_objects) else {
                break;
            };

            if curr_obj.base.is_spinner() {
                continue;
            }

            // * scales note 0 to 1 from history to now
            let time_decay = (f64::from(Self::HISTORY_TIME_MAX)
                - (curr.start_time - curr_obj.start_time))
                / f64::from(Self::HISTORY_TIME_MAX);
            let note_decay = (historical_note_count - i) as f64 / historical_note_count as f64;

            // * either we're limited by time or limited by object count.
            let curr_historical_decay = note_decay.min(time_decay);

            // * Use custom cap value to ensure that at this point delta time is actually zero
            const DELTA_MIN_VALUE: f64 = 1e-7;

            let curr_delta = curr_obj.delta_time.max(DELTA_MIN_VALUE);
            let prev_delta = prev_obj.delta_time.max(DELTA_MIN_VALUE);

            let delta_difference = (prev_delta - curr_delta).abs();

            // * Make sure to always have the current island initialised - if we don't do it here it will only initialise on the next rhythm change
            if island.delta == i32::MAX {
                island = Island::new(curr_delta as i32);
            }

            // * calculate how much current delta difference deserves a rhythm bonus
            // * this function is meant to reduce rhythm bonus for deltas that are multiples of each other (i.e 100 and 200)
            let delta_difference_ratio = prev_delta.max(curr_delta) / prev_delta.min(curr_delta);

            // * reduce ratio bonus if delta difference is too big
            let difference_multiplier = (2.0 - delta_difference_ratio / 8.0).clamp(0.0, 1.0);

            let window_penalty = ((delta_difference - delta_difference_epsilon)
                / delta_difference_epsilon)
                .clamp(0.0, 1.0);

            let mut effective_difficulty = get_effective_difficulty(delta_difference_ratio)
                * window_penalty
                * difference_multiplier;

            // * if previous object is a slider it might be easier to tap since you don't have to do a whole tapping motion
            // * while a full deltatime might end up some weird ratio the "unpress->tap" motion might be simple
            // * for example a slider-circle-circle pattern should be evaluated as a regular triple and not as a single->double
            if prev_obj.base.is_slider() {
                let slider_lazy_end_delta = curr_obj.min_jump_time;
                let slider_lazy_delta_difference_ratio =
                    slider_lazy_end_delta.max(curr_delta) / slider_lazy_end_delta.min(curr_delta);

                let slider_real_end_delta = curr_obj.last_object_end_delta_time;
                let slider_real_delta_difference_ratio =
                    slider_real_end_delta.max(curr_delta) / slider_real_end_delta.min(curr_delta);

                let slider_effective_difficulty =
                    get_effective_difficulty(slider_lazy_delta_difference_ratio)
                        .min(get_effective_difficulty(slider_real_delta_difference_ratio));
                effective_difficulty = slider_effective_difficulty.min(effective_difficulty);
            }

            if delta_difference < delta_difference_epsilon {
                // * island is still progressing
                island.add_delta(curr_delta as i32);
            }

            if first_delta_switch {
                if delta_difference > delta_difference_epsilon {
                    // * bpm change is into slider, this is easy acc window
                    if curr_obj.base.is_slider() {
                        effective_difficulty *= 0.5;
                    }

                    // * repeated island polarity (2 -> 4, 3 -> 5)
                    if island.is_similar_polarity(&previous_island, delta_difference_epsilon) {
                        effective_difficulty *= 0.5;
                    }

                    // * previous increase happened a note ago, 1/1->1/2-1/4, dont want to buff this.
                    if prev_prev_obj.delta_time.max(DELTA_MIN_VALUE)
                        > prev_delta + delta_difference_epsilon
                        && prev_delta > curr_delta + delta_difference_epsilon
                    {
                        effective_difficulty *= 0.125;
                    }

                    // * repeated island size (ex: triplet -> triplet)
                    // * TODO: remove this nerf since its staying here only for balancing purposes because of the flawed ratio calculation
                    if previous_island.delta_count == island.delta_count {
                        effective_difficulty *= 0.5;
                    }

                    let is_speeding_up = prev_delta > curr_delta + delta_difference_epsilon;

                    if is_speeding_up {
                        effective_difficulty *= 0.65;
                    }

                    let mut found = false;

                    for existing_island in &mut islands {
                        if existing_island.almost_equals(&island, delta_difference_epsilon) {
                            // * only increase island occurrences if they're going one after another
                            if previous_island.almost_equals(&island, delta_difference_epsilon) {
                                existing_island.occurrences += 1;
                            }

                            // * repeated island (ex: triplet -> triplet)
                            let power = logistic(f64::from(island.delta), 58.33, 0.24, Some(2.75));
                            effective_difficulty *= (3.0 / f64::from(existing_island.occurrences))
                                .min((1.0 / f64::from(existing_island.occurrences)).powf(power));

                            found = true;
                            break;
                        }
                    }

                    if !found && island.delta_count > 0 {
                        islands.push(island);
                    }

                    // * scale down the difficulty if the object is double-tappable
                    effective_difficulty *=
                        1.0 - prev_obj.calculate_double_tap_feasibility(Some(curr_obj)) * 0.75;

                    if island.delta_count > 1 {
                        rhythm_complexity_sum += (effective_difficulty * start_difficulty).sqrt()
                            * curr_historical_decay;
                    } else {
                        // * constant difficulty for single-note islands
                        rhythm_complexity_sum += 0.7 * curr_historical_decay;
                    }

                    start_difficulty = effective_difficulty;

                    if prev_delta + delta_difference_epsilon < curr_delta {
                        // * we're slowing down, stop counting
                        first_delta_switch = false; // * if we're speeding up, this stays true and we keep counting island size.
                    }

                    previous_island = island;
                    island = Island::new(curr_delta as i32);
                }
            } else if prev_delta > curr_delta + delta_difference_epsilon {
                // * we're speeding up
                // * Begin counting island until we change speed again.
                first_delta_switch = true;

                // * bpm change is into slider, this is easy acc window
                if curr_obj.base.is_slider() {
                    effective_difficulty *= 0.6;
                }

                // * bpm change was from a slider, this is easier typically than circle -> circle
                // * unintentional side effect is that bursts with kicksliders at the ends might have lower difficulty than bursts without sliders
                if prev_obj.base.is_slider() {
                    effective_difficulty *= 0.6;
                }

                start_difficulty = effective_difficulty;

                island = Island::new(curr_delta as i32);
            }

            prev_prev_obj = prev_obj;
            prev_obj = curr_obj;
        }

        // * If the current island is long we don't want the sum to have as big of an effect
        rhythm_complexity_sum *= reverse_lerp(f64::from(island.delta_count), 22.0, 3.0);

        // * produces multiplier that can be applied to strain. range [1, infinity) (not really though)
        (4.0 + rhythm_complexity_sum * Self::RHYTHM_OVERALL_MULTIPLIER).sqrt() / 2.0
    }
}

fn get_effective_difficulty(delta_difference_ratio: f64) -> f64 {
    const RHYTHM_RATIO_DIFFICULTY_MULTIPLIER: f64 = 26.0;

    // * Take only the fractional part of the value since we're only interested in punishing multiples
    let delta_difference_fraction = delta_difference_ratio - delta_difference_ratio.trunc();

    1.0 + RHYTHM_RATIO_DIFFICULTY_MULTIPLIER
        * 0.5_f64.min(smoothstep_bell_curve_unit(delta_difference_fraction))
}

/// An island is a group of consecutive objects with the same delta time.
#[derive(Copy, Clone)]
struct Island {
    delta: i32,
    delta_count: i32,
    occurrences: i32,
}

const MIN_DELTA_TIME: i32 = 25;

const _: [(); 0 - !{ MIN_DELTA_TIME - OsuDifficultyObject::MIN_DELTA_TIME as i32 == 0 } as usize] =
    [];

impl Island {
    fn new(delta: i32) -> Self {
        Self {
            delta: cmp::max(delta, MIN_DELTA_TIME),
            delta_count: 1,
            occurrences: 1,
        }
    }

    fn add_delta(&mut self, delta: i32) {
        if self.delta == i32::MAX {
            self.delta = cmp::max(delta, MIN_DELTA_TIME);
        }

        self.delta_count += 1;
    }

    fn is_similar_polarity(&self, other: &Self, epsilon: f64) -> bool {
        // * single delta islands shouldn't be compared
        if self.delta_count <= 1 || other.delta_count <= 1 {
            return false;
        }

        f64::from((self.delta - other.delta).abs()) < epsilon
            && self.delta_count % 2 == other.delta_count % 2
    }

    fn almost_equals(&self, other: &Self, epsilon: f64) -> bool {
        f64::from((self.delta - other.delta).abs()) < epsilon
            && self.delta_count == other.delta_count
    }
}
