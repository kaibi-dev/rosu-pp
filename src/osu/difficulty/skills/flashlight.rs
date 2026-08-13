use crate::{
    GameMods,
    any::difficulty::{
        object::{HasStartTime, IDifficultyObject},
        skills::{StrainSkill, strain_decay},
    },
    osu::difficulty::{evaluators::FlashlightEvaluator, object::OsuDifficultyObject},
    util::{difficulty::reverse_lerp, traits::IEnumerable},
};

define_skill! {
    pub struct Flashlight: StrainSkill => [OsuDifficultyObject<'a>][OsuDifficultyObject<'a>] {
        current_strain: f64,
        has_flashlight: bool,
        has_hidden: bool,
        has_hidden_bonus: bool,
        has_touch_device: bool,
        has_relax: bool,
        has_autopilot: bool,
        attraction_strength: Option<f64>,
        deflate_start_scale: Option<f64>,
        total_objects: usize,
    }

    pub fn new(mods: &GameMods, total_objects: usize) -> Self {
        Self {
            current_strain: 0.0,
            has_flashlight: mods.fl(),
            has_hidden: mods.hd() && !mods.hd_only_fade_approach_circles().unwrap_or(false),
            has_hidden_bonus: mods.hd(),
            has_touch_device: mods.td(),
            has_relax: mods.rx(),
            has_autopilot: mods.ap(),
            attraction_strength: mods.attraction_strength(),
            deflate_start_scale: mods.deflate_start_scale(),
            total_objects: total_objects,
        }
    }
}

impl Flashlight {
    const SKILL_MULTIPLIER: f64 = 0.058;
    const STRAIN_DECAY_BASE: f64 = 0.15;

    fn calculate_initial_strain(
        &mut self,
        time: f64,
        curr: &OsuDifficultyObject<'_>,
        objects: &[OsuDifficultyObject<'_>],
    ) -> f64 {
        let prev_start_time = curr
            .previous(0, objects)
            .map_or(0.0, HasStartTime::start_time);

        self.current_strain * strain_decay(time - prev_start_time, Self::STRAIN_DECAY_BASE)
    }

    fn strain_value_at(
        &mut self,
        curr: &OsuDifficultyObject<'_>,
        objects: &[OsuDifficultyObject<'_>],
    ) -> f64 {
        if !self.has_flashlight {
            return 0.0;
        }

        self.current_strain *= strain_decay(curr.delta_time, Self::STRAIN_DECAY_BASE);
        self.current_strain +=
            self.calculate_adjusted_difficulty(curr, objects) * Self::SKILL_MULTIPLIER;

        self.current_strain
    }

    fn calculate_adjusted_difficulty(
        &self,
        curr: &OsuDifficultyObject<'_>,
        objects: &[OsuDifficultyObject<'_>],
    ) -> f64 {
        let mut difficulty = FlashlightEvaluator::evaluate_diff_of(
            curr,
            objects,
            self.has_hidden,
            self.has_hidden_bonus,
        );

        if self.has_touch_device {
            difficulty = difficulty.powf(0.9);
        }

        if let Some(magnetised_strength) = self.attraction_strength {
            difficulty *= 1.0 - magnetised_strength;
        }

        if let Some(deflate_initial_scale) = self.deflate_start_scale {
            difficulty *= reverse_lerp(deflate_initial_scale, 11.0, 1.0).clamp(0.1, 1.0);
        }

        if self.has_relax {
            difficulty *= 0.7;
        }

        if self.has_autopilot {
            difficulty *= 0.4;
        }

        difficulty *= 0.985 + curr.overall_difficulty().max(0.0).powi(2) / 4000.0;

        difficulty
    }

    #[expect(
        clippy::needless_pass_by_value,
        reason = "function definition needs to stay in-sync with `StrainSkill::difficulty_value`"
    )]
    fn difficulty_value(current_strain_peaks: Vec<f64>) -> f64 {
        // Length scaling is applied in `into_difficulty_value` / `cloned_difficulty_value`.
        current_strain_peaks.cs_sum()
    }

    pub fn difficulty_to_performance(difficulty: f64) -> f64 {
        25.0 * difficulty.powi(2)
    }

    pub fn process(&mut self, curr: &OsuDifficultyObject<'_>, objects: &[OsuDifficultyObject<'_>]) {
        StrainSkill::process(self, curr, objects);
    }

    pub fn cloned_difficulty_value(&self) -> f64 {
        let sum = Self::difficulty_value(Self::get_current_strain_peaks(
            self.strain_skill_strain_peaks.clone(),
            self.strain_skill_current_section_peak,
        ));

        self.apply_length_bonus(sum)
    }

    fn apply_length_bonus(&self, sum: f64) -> f64 {
        let total_objects = self.total_objects as f64;

        // * Account for shorter maps having a higher ratio of 0 combo/100 combo flashlight radius.
        sum * (0.7
            + 0.1 * (total_objects / 200.0).min(1.0)
            + if total_objects > 200.0 {
                0.2 * ((total_objects - 200.0) / 200.0).min(1.0)
            } else {
                0.0
            })
    }
}
