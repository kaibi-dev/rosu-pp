use crate::{
    any::difficulty::object::IDifficultyObject, osu::difficulty::object::OsuDifficultyObject,
};

pub struct AgilityEvaluator;

impl AgilityEvaluator {
    pub fn evaluate_diff_of<'a>(
        curr: &'a OsuDifficultyObject<'a>,
        diff_objects: &'a [OsuDifficultyObject<'a>],
    ) -> f64 {
        if curr.base.is_spinner() {
            return 0.0;
        }

        const DISTANCE_CAP: f64 = OsuDifficultyObject::NORMALIZED_DIAMETER as f64 * 1.2; // * 1.2 circles distance between centers

        let osu_curr_obj = curr;
        let osu_prev_obj = if curr.idx > 0 {
            curr.previous(0, diff_objects)
        } else {
            None
        };

        let travel_distance = osu_prev_obj.map_or(0.0, |obj| obj.lazy_travel_dist);
        let distance = travel_distance + osu_curr_obj.lazy_jump_dist;

        let distance_scaled = distance.min(DISTANCE_CAP) / DISTANCE_CAP;

        let mut agility_difficulty = distance_scaled * 1000.0 / osu_curr_obj.adjusted_delta_time;

        agility_difficulty *= osu_curr_obj.small_circle_bonus.powf(1.5);

        agility_difficulty *= high_bpm_bonus(osu_curr_obj.adjusted_delta_time);

        agility_difficulty
    }
}

fn high_bpm_bonus(ms: f64) -> f64 {
    1.0 / (1.0 - 0.2_f64.powf(ms / 1000.0))
}
