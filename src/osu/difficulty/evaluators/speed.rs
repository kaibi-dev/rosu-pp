use crate::{
    any::difficulty::object::IDifficultyObject,
    osu::difficulty::object::OsuDifficultyObject,
    util::difficulty::{bpm_to_milliseconds, milliseconds_to_bpm},
};

pub struct SpeedEvaluator;

impl SpeedEvaluator {
    const MIN_SPEED_BONUS: f64 = 200.0; // * 200 BPM 1/4th
    const SPEED_BALANCING_FACTOR: f64 = 40.0;

    pub fn evaluate_diff_of<'a>(
        curr: &'a OsuDifficultyObject<'a>,
        diff_objects: &'a [OsuDifficultyObject<'a>],
    ) -> f64 {
        if curr.base.is_spinner() {
            return 0.0;
        }

        let osu_curr_obj = curr;

        let mut strain_time = osu_curr_obj.adjusted_delta_time;
        let double_tap_feasibility =
            1.0 - osu_curr_obj.calculate_double_tap_feasibility(curr.next(0, diff_objects));

        // * Cap deltatime to the OD 300 hitwindow.
        // * 0.93 is derived from making sure 260bpm OD8 streams aren't nerfed harshly, whilst 0.92 limits the effect of the cap.
        strain_time /= ((strain_time / osu_curr_obj.hit_window_great) / 0.93).clamp(0.92, 1.0);

        // * speedBonus will be 0.0 for BPM < 200
        let speed_bonus = if milliseconds_to_bpm(strain_time, None) > Self::MIN_SPEED_BONUS {
            // * Add additional scaling bonus for streams/bursts higher than 200bpm
            let base = (bpm_to_milliseconds(Self::MIN_SPEED_BONUS, None) - strain_time)
                / Self::SPEED_BALANCING_FACTOR;

            0.75 * base.powi(2)
        } else {
            0.0
        };

        // * Base difficulty with all bonuses
        let mut speed_difficulty = (1.0 + speed_bonus) * 1000.0 / strain_time;

        speed_difficulty *= high_bpm_bonus(osu_curr_obj.adjusted_delta_time);

        // * Apply penalty if there's doubletappable doubles
        speed_difficulty * double_tap_feasibility
    }
}

fn high_bpm_bonus(ms: f64) -> f64 {
    1.0 / (1.0 - 0.3_f64.powf(ms / 1000.0))
}
