use crate::util::{difficulty::logistic, float_ext::FloatExt};

/// Port of lazer `HarmonicSkill`.
#[derive(Clone)]
pub struct HarmonicSkill {
    pub object_difficulties: Vec<f64>,
    pub object_weight_sum: f64,
    harmonic_scale: f64,
    decay_exponent: f64,
}

impl HarmonicSkill {
    pub fn new(harmonic_scale: f64, decay_exponent: f64) -> Self {
        Self {
            object_difficulties: Vec::with_capacity(256),
            object_weight_sum: 0.0,
            harmonic_scale,
            decay_exponent,
        }
    }

    pub fn process(&mut self, difficulty: f64) {
        self.object_difficulties.push(difficulty);
    }

    pub fn difficulty_value(&mut self) -> f64 {
        self.difficulty_value_of(self.object_difficulties.clone())
    }

    pub fn difficulty_value_of(&mut self, difficulties: Vec<f64>) -> f64 {
        self.object_weight_sum = 0.0;

        if difficulties.is_empty() {
            return 0.0;
        }

        let mut difficulty = 0.0;
        let mut index = 0i32;

        // * Objects with 0 difficulty are excluded to avoid worst-case time complexity of the following sort (e.g. /b/2351871).
        // * These objects will not contribute to the difficulty.
        let mut sorted: Vec<f64> = difficulties.into_iter().filter(|&v| v > 0.0).collect();
        sorted.sort_by(|a, b| b.total_cmp(a));

        for obj in sorted {
            // * Use a harmonic sum that considers each object of the map according to a predefined weight.
            let harmonic_term = self.harmonic_scale / (1.0 + f64::from(index));
            let weight = (1.0 + harmonic_term)
                / (f64::from(index).powf(self.decay_exponent) + 1.0 + harmonic_term);

            self.object_weight_sum += weight;

            difficulty += obj * weight;
            index += 1;
        }

        difficulty
    }

    pub fn count_top_weighted_object_difficulties(&self, difficulty_value: f64) -> f64 {
        if self.object_difficulties.is_empty() {
            return 0.0;
        }

        if FloatExt::eq(self.object_weight_sum, 0.0) {
            return 0.0;
        }

        // * What would the top difficulty be if all object difficulties were identical
        let consistent_top_object = difficulty_value / self.object_weight_sum;

        if FloatExt::eq(consistent_top_object, 0.0) {
            return 0.0;
        }

        self.object_difficulties
            .iter()
            .map(|d| logistic(*d / consistent_top_object, 0.88, 10.0, Some(1.1)))
            .sum()
    }

    pub fn difficulty_to_performance(difficulty: f64) -> f64 {
        4.0 * difficulty * difficulty * difficulty
    }
}
