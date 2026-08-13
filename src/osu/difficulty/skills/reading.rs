use crate::{
    model::mods::GameMods,
    osu::difficulty::{
        evaluators::ReadingEvaluator, object::OsuDifficultyObject, skills::harmonic::HarmonicSkill,
    },
};

#[derive(Clone)]
pub struct Reading {
    inner: HarmonicSkill,
    current_strain: f64,
    has_hidden_mod: bool,
    has_touch_device: bool,
    has_relax: bool,
    has_autopilot: bool,
    attraction_strength: Option<f64>,
    reduced_note_count: f64,
    reduced_duration: Option<f64>,
}

impl Reading {
    const SKILL_MULTIPLIER: f64 = 2.5;
    const STRAIN_DECAY_BASE: f64 = 0.8;
    const REDUCED_DIFFICULTY_DURATION: f64 = 60.0 * 1000.0;

    pub fn new(mods: &GameMods) -> Self {
        Self {
            inner: HarmonicSkill::new(1.0, 0.9),
            current_strain: 0.0,
            has_hidden_mod: mods.hd() && !mods.hd_only_fade_approach_circles().unwrap_or(false),
            has_touch_device: mods.td(),
            has_relax: mods.rx(),
            has_autopilot: mods.ap(),
            attraction_strength: mods.attraction_strength(),
            reduced_note_count: 0.0,
            reduced_duration: None,
        }
    }

    pub fn process(&mut self, curr: &OsuDifficultyObject<'_>, objects: &[OsuDifficultyObject<'_>]) {
        let difficulty = self.object_difficulty_of(curr, objects);
        self.inner.process(difficulty);
    }

    fn object_difficulty_of(
        &mut self,
        curr: &OsuDifficultyObject<'_>,
        objects: &[OsuDifficultyObject<'_>],
    ) -> f64 {
        let decay =
            crate::any::difficulty::skills::strain_decay(curr.delta_time, Self::STRAIN_DECAY_BASE);

        self.current_strain *= decay;
        self.current_strain += self.calculate_adjusted_difficulty(curr, objects)
            * (1.0 - decay)
            * Self::SKILL_MULTIPLIER;

        // * This currently operates under the assumption that `ObjectDifficultyOf` is called once per object, and in order.
        // * Under that assumption, we can trust that `current.StartTime` refers to the start time of the first object in the case that `reducedDuration` is yet to be set.
        if self.reduced_duration.is_none() {
            self.reduced_duration = Some(curr.start_time + Self::REDUCED_DIFFICULTY_DURATION);
        }

        // * This relies on the same assumption, as calling in order means that we can safely increase the note count until we reach the first object after the reduced duration.
        if curr.start_time <= self.reduced_duration.unwrap_or(0.0) {
            self.reduced_note_count += 1.0;
        }

        self.current_strain
    }

    fn calculate_adjusted_difficulty(
        &self,
        curr: &OsuDifficultyObject<'_>,
        objects: &[OsuDifficultyObject<'_>],
    ) -> f64 {
        let mut difficulty = ReadingEvaluator::evaluate_diff_of(curr, objects, self.has_hidden_mod);

        if self.has_touch_device {
            difficulty = difficulty.powf(0.89);
        }

        if let Some(magnetised_strength) = self.attraction_strength {
            difficulty *= 1.0 - magnetised_strength;
        }

        if self.has_relax {
            difficulty *= 0.4;
        }

        if self.has_autopilot {
            difficulty *= 0.1;
        }

        difficulty *= 0.825 + curr.overall_difficulty().max(0.0).powf(2.2) / 1125.0;

        difficulty
    }

    fn get_transformed_difficulties(&self) -> Vec<f64> {
        let mut difficulties: Vec<f64> = self
            .inner
            .object_difficulties
            .iter()
            .copied()
            .filter(|&v| v > 0.0)
            .collect();

        const REDUCED_DIFFICULTY_BASE_LINE: f64 = 0.0; // * Assume the first seconds are completely memorised

        let mut i = 0usize;

        while (i as f64) < (difficulties.len() as f64).min(self.reduced_note_count) {
            let scale = f64::log10(lerp(
                1.0,
                10.0,
                (i as f64 / self.reduced_note_count).clamp(0.0, 1.0),
            ));
            difficulties[i] *= lerp(REDUCED_DIFFICULTY_BASE_LINE, 1.0, scale);
            i += 1;
        }

        difficulties
    }

    pub fn count_top_weighted_object_difficulties(&self, difficulty_value: f64) -> f64 {
        if self.inner.object_difficulties.is_empty() {
            return 0.0;
        }

        if crate::util::float_ext::FloatExt::eq(self.inner.object_weight_sum, 0.0) {
            return 0.0;
        }

        // * What would the top difficulty be if all object difficulties were identical
        let consistent_top_note = difficulty_value / self.inner.object_weight_sum;

        if crate::util::float_ext::FloatExt::eq(consistent_top_note, 0.0) {
            return 0.0;
        }

        self.inner
            .object_difficulties
            .iter()
            .map(|d| {
                crate::util::difficulty::logistic(*d / consistent_top_note, 1.15, 5.0, Some(1.1))
            })
            .sum()
    }

    pub fn cloned_difficulty_value(&mut self) -> f64 {
        let transformed = self.get_transformed_difficulties();
        self.inner.difficulty_value_of(transformed)
    }

    pub fn into_current_strain_peaks(self) -> Vec<f64> {
        self.inner.object_difficulties
    }
}

const fn lerp(start: f64, end: f64, amount: f64) -> f64 {
    start + (end - start) * amount
}
