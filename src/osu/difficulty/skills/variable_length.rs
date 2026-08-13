use std::cmp::Ordering;

use crate::{
    osu::difficulty::object::OsuDifficultyObject,
    util::{difficulty::logistic, float_ext::FloatExt},
};

/// Port of lazer `VariableLengthStrainSkill.StrainPeak`.
#[derive(Copy, Clone, Debug)]
pub struct StrainPeak {
    pub value: f64,
    pub section_length: f64,
}

impl StrainPeak {
    pub fn new(value: f64, section_length: f64) -> Self {
        Self {
            value,
            section_length: section_length.round(),
        }
    }
}

impl PartialEq for StrainPeak {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value && self.section_length == other.section_length
    }
}

impl Eq for StrainPeak {}

/// Port of lazer `VariableLengthStrainSkill`.
#[derive(Clone)]
pub struct VariableLengthStrainSkill {
    pub decay_weight: f64,
    pub max_section_length: i32,
    max_stored_length: f64,
    current_section_peak: f64,
    current_section_begin: f64,
    current_section_end: f64,
    strain_peaks: Vec<StrainPeak>,
    total_length: f64,
    queued_strains: Vec<(f64, f64)>,
    final_peak: Option<StrainPeak>,
    pub object_difficulties: Vec<f64>,
}

impl VariableLengthStrainSkill {
    pub fn new(decay_weight: f64, max_section_length: i32) -> Self {
        Self {
            decay_weight,
            max_section_length,
            max_stored_length: 11.0 / (1.0 - decay_weight),
            current_section_peak: 0.0,
            current_section_begin: 0.0,
            current_section_end: 0.0,
            strain_peaks: Vec::with_capacity(256),
            total_length: 0.0,
            queued_strains: Vec::new(),
            final_peak: None,
            object_difficulties: Vec::with_capacity(256),
        }
    }

    pub fn process(
        &mut self,
        curr: &OsuDifficultyObject<'_>,
        objects: &[OsuDifficultyObject<'_>],
        mut strain_value_at: impl FnMut(&OsuDifficultyObject<'_>, &[OsuDifficultyObject<'_>]) -> f64,
        mut calculate_initial_strain: impl FnMut(
            f64,
            &OsuDifficultyObject<'_>,
            &[OsuDifficultyObject<'_>],
        ) -> f64,
    ) {
        let difficulty_value = self.process_internal(
            curr,
            objects,
            &mut strain_value_at,
            &mut calculate_initial_strain,
        );
        self.object_difficulties.push(difficulty_value);
    }

    fn process_internal(
        &mut self,
        curr: &OsuDifficultyObject<'_>,
        objects: &[OsuDifficultyObject<'_>],
        strain_value_at: &mut impl FnMut(&OsuDifficultyObject<'_>, &[OsuDifficultyObject<'_>]) -> f64,
        calculate_initial_strain: &mut impl FnMut(
            f64,
            &OsuDifficultyObject<'_>,
            &[OsuDifficultyObject<'_>],
        ) -> f64,
    ) -> f64 {
        let max_section_length = f64::from(self.max_section_length);

        // * If we're on the first object, set up the first section to end `MaxSectionLength` after it.
        if curr.idx == 0 {
            self.current_section_begin = curr.start_time;
            self.current_section_end = self.current_section_begin + max_section_length;

            // * No work is required for first object after calculating difficulty
            self.current_section_peak = strain_value_at(curr, objects);

            return self.current_section_peak;
        }

        self.backfill_peaks(curr, objects, calculate_initial_strain);

        let current_strain = strain_value_at(curr, objects);

        // * If the current strain is larger than the current peak, begin a new peak
        // * Otherwise, add the current strain to the queue
        if current_strain > self.current_section_peak {
            // * Clear the queue since none of the strains inside of it will be contributing to the difficulty.
            self.queued_strains.clear();

            // * End the current section with the new peak
            self.save_current_peak(curr.start_time - self.current_section_begin);

            // * Set up the new section to start at the current object with the current strain
            self.current_section_begin = curr.start_time;
            self.current_section_end = self.current_section_begin + max_section_length;
            self.current_section_peak = current_strain;
        } else {
            // * Empty the queue of smaller elements as they won't be relevant to difficulty
            while self
                .queued_strains
                .last()
                .is_some_and(|&(strain, _)| strain < current_strain)
            {
                self.queued_strains.pop();
            }

            self.queued_strains.push((current_strain, curr.start_time));
        }

        current_strain
    }

    fn backfill_peaks(
        &mut self,
        curr: &OsuDifficultyObject<'_>,
        objects: &[OsuDifficultyObject<'_>],
        calculate_initial_strain: &mut impl FnMut(
            f64,
            &OsuDifficultyObject<'_>,
            &[OsuDifficultyObject<'_>],
        ) -> f64,
    ) {
        let max_section_length = f64::from(self.max_section_length);

        // * If the current object starts after the current section ends
        // * then we want to start a new section without any harsh drop-off.
        // * If we have previous strains that influence the current difficulty we will prioritise those first.
        // * Otherwise, start with the current object's initial strain.
        while curr.start_time > self.current_section_end {
            // * Save the current peak, marking the end of the section.
            self.save_current_peak(self.current_section_end - self.current_section_begin);
            self.current_section_begin = self.current_section_end;

            // * If we have any strains queued, then we will use those until the object falls into the new section.
            if !self.queued_strains.is_empty() {
                let (strain, start_time) = self.queued_strains.remove(0);

                // * We want the section to end `MaxSectionLength` after the strain we're using as an influence.
                // * This effectively means the queued strain will exist in its own section if the gap between the queued strain and current object is large enough.
                // * This is required to make sure there's no harsh difficulty difference between 2 sections if there was a large gap.
                self.current_section_end = start_time + max_section_length;
                self.start_new_section_from(
                    self.current_section_begin,
                    curr,
                    objects,
                    calculate_initial_strain,
                );

                // * If the current object's peak was higher, we don't want to override it with a lower strain.
                // * Only use the queued strain if it contributes more difficulty.
                self.current_section_peak = self.current_section_peak.max(strain);
            } else {
                // * We don't have any prior strains to take as a reference, so end the new section `MaxSectionLength` after it starts.
                self.current_section_end = self.current_section_begin + max_section_length;
                self.start_new_section_from(
                    self.current_section_begin,
                    curr,
                    objects,
                    calculate_initial_strain,
                );
            }
        }
    }

    fn save_current_peak(&mut self, section_length: f64) {
        if let Some(final_peak) = self.final_peak.take() {
            if let Some(idx) = self.strain_peaks.iter().position(|p| p == &final_peak) {
                self.strain_peaks.remove(idx);
            }
        }

        let peak = StrainPeak::new(self.current_section_peak, section_length);

        add_in_place(&mut self.strain_peaks, peak);
        self.total_length += section_length;

        // * Remove from the back of our strain peaks if there's any which are too deep to contribute to difficulty.
        // * `maxStoredLength` dictates for us how many sections will preserve at least 99.999% of the difficulty value.
        let max_total = self.max_stored_length * f64::from(self.max_section_length);

        while self.total_length > max_total {
            if let Some(removed) = self.strain_peaks.pop() {
                self.total_length -= removed.section_length;
            } else {
                break;
            }
        }
    }

    fn start_new_section_from(
        &mut self,
        time: f64,
        curr: &OsuDifficultyObject<'_>,
        objects: &[OsuDifficultyObject<'_>],
        calculate_initial_strain: &mut impl FnMut(
            f64,
            &OsuDifficultyObject<'_>,
            &[OsuDifficultyObject<'_>],
        ) -> f64,
    ) {
        // * The maximum strain of the new section is not zero by default
        // * This means we need to capture the strain level at the beginning of the new section, and use that as the initial peak level.
        self.current_section_peak = calculate_initial_strain(time, curr, objects);
    }

    pub fn current_strain_peaks(&self) -> Vec<StrainPeak> {
        let mut peaks = self.strain_peaks.clone();

        if self.final_peak.is_none() {
            let peak = StrainPeak::new(
                self.current_section_peak,
                self.current_section_end - self.current_section_begin,
            );
            add_in_place(&mut peaks, peak);
        }

        peaks
    }

    pub fn into_current_strain_peaks(self) -> Vec<f64> {
        self.current_strain_peaks()
            .into_iter()
            .map(|p| p.value)
            .collect()
    }

    pub fn count_top_weighted_strains(&self, difficulty_value: f64) -> f64 {
        if self.object_difficulties.is_empty() {
            return 0.0;
        }

        // * What would the top strain be if all strain values were identical
        let consistent_top_strain = difficulty_value * (1.0 - self.decay_weight);

        if FloatExt::eq(consistent_top_strain, 0.0) {
            return self.object_difficulties.len() as f64;
        }

        // * Use a weighted sum of all strains. Constants are arbitrary and give nice values
        self.object_difficulties
            .iter()
            .map(|s| logistic(*s / consistent_top_strain, 0.88, 10.0, Some(1.1)))
            .sum()
    }
}

/// C# `List.AddInPlace` using `IComparable` + .NET `List.BinarySearch`.
fn add_in_place(list: &mut Vec<StrainPeak>, item: StrainPeak) {
    let mut lo = 0i32;
    let mut hi = list.len() as i32 - 1;

    while lo <= hi {
        let i = lo + ((hi - lo) >> 1);
        // * StrainPeak.CompareTo is reverse (highest first): other.Value.CompareTo(Value)
        // * BinarySearch compares list[i] to item => item.Value.CompareTo(list[i].Value)
        match item.value.total_cmp(&list[i as usize].value) {
            Ordering::Equal => {
                list.insert(i as usize, item);
                return;
            }
            Ordering::Less => lo = i + 1,
            Ordering::Greater => hi = i - 1,
        }
    }

    list.insert(lo as usize, item);
}
