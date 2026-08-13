use crate::{
    model::mods::GameMods,
    osu::difficulty::{
        evaluators::{RhythmEvaluator, SpeedEvaluator},
        object::OsuDifficultyObject,
        skills::harmonic::HarmonicSkill,
    },
    util::{difficulty::logistic, float_ext::FloatExt},
};

#[derive(Clone)]
pub struct Speed {
    inner: HarmonicSkill,
    current_strain: f64,
    slider_strains: Vec<f64>,
    has_relax: bool,
    has_autopilot: bool,
}

impl Speed {
    const SKILL_MULTIPLIER: f64 = 1.16;
    const STRAIN_DECAY_BASE: f64 = 0.3;

    pub fn new(mods: &GameMods) -> Self {
        Self {
            inner: HarmonicSkill::new(20.0, 0.9),
            current_strain: 0.0,
            slider_strains: Vec::with_capacity(64),
            has_relax: mods.rx(),
            has_autopilot: mods.ap(),
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
        if self.has_relax {
            return 0.0;
        }

        let decay = crate::any::difficulty::skills::strain_decay(
            curr.adjusted_delta_time,
            Self::STRAIN_DECAY_BASE,
        );

        self.current_strain *= decay;
        self.current_strain += self.calculate_adjusted_difficulty(curr, objects)
            * (1.0 - decay)
            * Self::SKILL_MULTIPLIER;

        let current_rhythm = RhythmEvaluator::evaluate_diff_of(curr, objects);

        let total_strain = self.current_strain * current_rhythm;

        if curr.base.is_slider() {
            self.slider_strains.push(total_strain);
        }

        total_strain
    }

    fn calculate_adjusted_difficulty(
        &self,
        curr: &OsuDifficultyObject<'_>,
        objects: &[OsuDifficultyObject<'_>],
    ) -> f64 {
        let mut difficulty = SpeedEvaluator::evaluate_diff_of(curr, objects);

        if self.has_autopilot {
            difficulty *= 0.5;
        }

        difficulty
    }

    pub fn relevant_object_count(&self) -> f64 {
        if self.inner.object_difficulties.is_empty() {
            return 0.0;
        }

        let max_strain = self
            .inner
            .object_difficulties
            .iter()
            .copied()
            .fold(0.0, f64::max);

        if FloatExt::eq(max_strain, 0.0) {
            return 0.0;
        }

        self.inner
            .object_difficulties
            .iter()
            .map(|strain| logistic(*strain / max_strain, 0.5, 12.0, None))
            .sum()
    }

    pub fn count_top_weighted_sliders(&self, difficulty_value: f64) -> f64 {
        if self.slider_strains.is_empty() {
            return 0.0;
        }

        if FloatExt::eq(self.inner.object_weight_sum, 0.0) {
            return 0.0;
        }

        // * What would the top note be if all note values were identical
        let consistent_top_object = difficulty_value / self.inner.object_weight_sum;

        if FloatExt::eq(consistent_top_object, 0.0) {
            return 0.0;
        }

        // * Use a weighted sum of all notes. Constants are arbitrary and give nice values
        self.slider_strains
            .iter()
            .map(|s| logistic(*s / consistent_top_object, 0.88, 10.0, Some(1.1)))
            .sum()
    }

    pub fn count_top_weighted_object_difficulties(&self, difficulty_value: f64) -> f64 {
        self.inner
            .count_top_weighted_object_difficulties(difficulty_value)
    }

    pub fn cloned_difficulty_value(&mut self) -> f64 {
        self.inner.difficulty_value()
    }

    pub fn into_current_strain_peaks(self) -> Vec<f64> {
        self.inner.object_difficulties
    }
}
