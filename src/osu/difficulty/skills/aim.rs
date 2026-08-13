use crate::{
    any::difficulty::object::{HasStartTime, IDifficultyObject},
    model::mods::GameMods,
    osu::difficulty::{
        evaluators::{AgilityEvaluator, FlowAimEvaluator, SnapAimEvaluator},
        object::OsuDifficultyObject,
        skills::variable_length::{StrainPeak, VariableLengthStrainSkill},
    },
    util::{
        difficulty::{logistic, logistic_exp, norm},
        float_ext::FloatExt,
    },
};

#[derive(Clone)]
pub struct Aim {
    pub include_sliders: bool,
    inner: VariableLengthStrainSkill,
    current_strain: f64,
    slider_strains: Vec<f64>,
    has_autopilot: bool,
    has_touch_device: bool,
    has_relax: bool,
    attraction_strength: Option<f64>,
}

impl Aim {
    const STRAIN_DECAY_BASE: f64 = 0.2;

    pub fn new(mods: &GameMods, include_sliders: bool) -> Self {
        Self {
            include_sliders,
            inner: VariableLengthStrainSkill::new(0.9, 400),
            current_strain: 0.0,
            slider_strains: Vec::with_capacity(64),
            has_autopilot: mods.ap(),
            has_touch_device: mods.td(),
            has_relax: mods.rx(),
            attraction_strength: mods.attraction_strength(),
        }
    }

    pub fn process(&mut self, curr: &OsuDifficultyObject<'_>, objects: &[OsuDifficultyObject<'_>]) {
        let include_sliders = self.include_sliders;
        let has_autopilot = self.has_autopilot;
        let has_touch_device = self.has_touch_device;
        let has_relax = self.has_relax;
        let attraction_strength = self.attraction_strength;

        let Self {
            inner,
            current_strain,
            slider_strains,
            ..
        } = self;

        let current_strain_for_initial = *current_strain;

        inner.process(
            curr,
            objects,
            |curr, objects| {
                Self::strain_value_at(
                    current_strain,
                    slider_strains,
                    include_sliders,
                    has_autopilot,
                    has_touch_device,
                    has_relax,
                    attraction_strength,
                    curr,
                    objects,
                )
            },
            |time, curr, objects| {
                Self::calculate_initial_strain(current_strain_for_initial, time, curr, objects)
            },
        );
    }

    fn calculate_initial_strain(
        current_strain: f64,
        time: f64,
        curr: &OsuDifficultyObject<'_>,
        objects: &[OsuDifficultyObject<'_>],
    ) -> f64 {
        let prev_start_time = curr
            .previous(0, objects)
            .map_or(0.0, HasStartTime::start_time);

        current_strain
            * crate::any::difficulty::skills::strain_decay(
                time - prev_start_time,
                Self::STRAIN_DECAY_BASE,
            )
    }

    #[expect(clippy::too_many_arguments, reason = "staying in-sync with lazer")]
    fn strain_value_at(
        current_strain: &mut f64,
        slider_strains: &mut Vec<f64>,
        include_sliders: bool,
        has_autopilot: bool,
        has_touch_device: bool,
        has_relax: bool,
        attraction_strength: Option<f64>,
        curr: &OsuDifficultyObject<'_>,
        objects: &[OsuDifficultyObject<'_>],
    ) -> f64 {
        if has_autopilot {
            return 0.0;
        }

        let decay = crate::any::difficulty::skills::strain_decay(
            curr.adjusted_delta_time,
            Self::STRAIN_DECAY_BASE,
        );

        *current_strain *= decay;
        *current_strain += calculate_adjusted_difficulty(
            curr,
            objects,
            include_sliders,
            has_touch_device,
            has_relax,
            attraction_strength,
        ) * (1.0 - decay);

        if curr.base.is_slider() {
            slider_strains.push(*current_strain);
        }

        *current_strain
    }

    pub fn get_difficult_sliders(&self) -> f64 {
        if self.slider_strains.is_empty() {
            return 0.0;
        }

        let max_slider_strain = self.slider_strains.iter().copied().fold(0.0, f64::max);

        if FloatExt::eq(max_slider_strain, 0.0) {
            return 0.0;
        }

        self.slider_strains
            .iter()
            .copied()
            .map(|strain| logistic(strain / max_slider_strain, 0.5, 12.0, None))
            .sum()
    }

    pub fn count_top_weighted_sliders(&self, difficulty_value: f64) -> f64 {
        if self.slider_strains.is_empty() {
            return 0.0;
        }

        // * What would the top strain be if all strain values were identical
        let consistent_top_strain = difficulty_value * (1.0 - self.inner.decay_weight);

        if FloatExt::eq(consistent_top_strain, 0.0) {
            return 0.0;
        }

        // * Use a weighted sum of all strains. Constants are arbitrary and give nice values
        self.slider_strains
            .iter()
            .map(|s| logistic(*s / consistent_top_strain, 0.88, 10.0, Some(1.1)))
            .sum()
    }

    pub fn count_top_weighted_strains(&self, difficulty_value: f64) -> f64 {
        self.inner.count_top_weighted_strains(difficulty_value)
    }

    pub fn cloned_difficulty_value(&self) -> f64 {
        self.difficulty_value_from(self.inner.current_strain_peaks())
    }

    pub fn into_current_strain_peaks(self) -> Vec<f64> {
        self.inner.into_current_strain_peaks()
    }

    pub fn difficulty_to_performance(difficulty: f64) -> f64 {
        4.0 * difficulty * difficulty * difficulty
    }

    fn difficulty_value_from(&self, peaks: Vec<StrainPeak>) -> f64 {
        let mut difficulty = 0.0;
        let mut time = 0.0;

        let strains = get_reduced_strain_peaks(peaks);

        // * Difficulty is a continuous weighted sum of the sorted strains
        for strain in strains {
            /* Weighting function can be thought of as:
                    b
                    ∫ DecayWeight^x dx
                    a
                where a = startTime and b = endTime

                Technically, the function below has been slightly modified from the equation above.
                The real function would be
                    double weight = DiffUtils.Pow(DecayWeight, startTime) - DiffUtils.Pow(DecayWeight, endTime);
                    ...
                    return difficulty / Math.Log(1 / DecayWeight);
                E.g. for a DecayWeight of 0.9, we're multiplying by 10 instead of 9.49122...

                This change makes it so that a map composed solely of MaxSectionLength chunks will have the exact same value when summed in this class and StrainSkill.
                Doing this ensures the relationship between strain values and difficulty values remains the same between the two classes.
            */
            let start_time = time;
            let end_time = time + strain.section_length / f64::from(self.inner.max_section_length);

            let weight =
                self.inner.decay_weight.powf(start_time) - self.inner.decay_weight.powf(end_time);

            difficulty += strain.value * weight;
            time = end_time;
        }

        difficulty / (1.0 - self.inner.decay_weight)
    }
}

fn calculate_adjusted_difficulty(
    curr: &OsuDifficultyObject<'_>,
    objects: &[OsuDifficultyObject<'_>],
    include_sliders: bool,
    has_touch_device: bool,
    has_relax: bool,
    attraction_strength: Option<f64>,
) -> f64 {
    const SKILL_MULTIPLIER_SNAP: f64 = 70.9;
    const SKILL_MULTIPLIER_AGILITY: f64 = 2.35;
    const SKILL_MULTIPLIER_FLOW: f64 = 242.0;

    let snap_difficulty =
        SnapAimEvaluator::evaluate_diff_of(curr, objects, include_sliders) * SKILL_MULTIPLIER_SNAP;
    let agility_difficulty =
        AgilityEvaluator::evaluate_diff_of(curr, objects) * SKILL_MULTIPLIER_AGILITY;
    let flow_difficulty =
        FlowAimEvaluator::evaluate_diff_of(curr, objects, include_sliders) * SKILL_MULTIPLIER_FLOW;

    let mut total_difficulty = calculate_total_value(
        snap_difficulty,
        agility_difficulty,
        flow_difficulty,
        has_touch_device,
        has_relax,
    );

    if let Some(magnetised_strength) = attraction_strength {
        total_difficulty *= 1.0 - magnetised_strength;
    }

    total_difficulty *= 0.985 + curr.overall_difficulty().max(0.0).powi(2) / 4000.0;

    total_difficulty
}

fn calculate_total_value(
    mut snap_difficulty: f64,
    agility_difficulty: f64,
    mut flow_difficulty: f64,
    has_touch_device: bool,
    has_relax: bool,
) -> f64 {
    const SKILL_MULTIPLIER_TOTAL: f64 = 1.12;
    const COMBINED_SNAP_NORM_EXPONENT: f64 = 1.2;

    // * We compare flow to combined snap and agility because snap by itself doesn't have enough difficulty to be above flow on streams
    // * Agility on the other hand is supposed to measure the rate of cursor velocity changes while snapping
    // * So snapping every circle on a stream requires an enormous amount of agility at which point it's easier to flow
    let mut combined_snap_difficulty = norm(
        COMBINED_SNAP_NORM_EXPONENT,
        [snap_difficulty, agility_difficulty],
    );

    let p_snap = calculate_snap_flow_probability(flow_difficulty / combined_snap_difficulty);
    let p_flow = 1.0 - p_snap;

    if has_touch_device {
        // * we don't adjust agility here since agility represents TD difficulty in a decent enough way
        snap_difficulty = snap_difficulty.powf(0.89);
        combined_snap_difficulty = norm(
            COMBINED_SNAP_NORM_EXPONENT,
            [snap_difficulty, agility_difficulty],
        );
    }

    if has_relax {
        combined_snap_difficulty *= 0.75;
        flow_difficulty *= 0.6;
    }

    let total_difficulty = combined_snap_difficulty * p_snap + flow_difficulty * p_flow;

    total_difficulty * SKILL_MULTIPLIER_TOTAL
}

// * A function that turns the ratio of snap : flow into the probability of snapping/flowing
// * It has the constraints:
// * P(snap) + P(flow) = 1 (the object is always either snapped or flowed)
// * P(snap) = f(snap/flow), P(flow) = f(flow/snap) (ie snap and flow are symmetric and reversible)
// * Therefore: f(x) + f(1/x) = 1
// * 0 <= f(x) <= 1 (cannot have negative or greater than 100% probability of snapping or flowing)
// * This logistic function is a solution, which fits nicely with the general idea of interpolation and provides a tuneable constant
fn calculate_snap_flow_probability(ratio: f64) -> f64 {
    const K: f64 = 7.27;

    if ratio == 0.0 {
        return 0.0;
    }

    if ratio.is_nan() {
        return 1.0;
    }

    logistic_exp(-K * ratio.ln(), None)
}

fn get_reduced_strain_peaks(peaks: Vec<StrainPeak>) -> Vec<StrainPeak> {
    const REDUCED_SECTION_TIME: f64 = 4000.0;
    const REDUCED_STRAIN_BASELINE: f64 = 0.727;

    let mut strains: Vec<StrainPeak> = peaks.into_iter().filter(|p| p.value > 0.0).collect();

    const CHUNK_SIZE: f64 = 20.0;
    let mut time = 0.0;
    let mut skip_count = 0usize;

    // * We are reducing the highest strains first to account for extreme difficulty spikes
    // * Strains are split into 20ms chunks to try to mitigate inconsistencies caused by reducing strains
    while skip_count < strains.len() && time < REDUCED_SECTION_TIME {
        let strain = strains[skip_count];

        let mut added_time = 0.0;

        while added_time < strain.section_length {
            let scale = f64::log10(lerp(
                1.0,
                10.0,
                ((time + added_time) / REDUCED_SECTION_TIME).clamp(0.0, 1.0),
            ));

            // * intentionally add at end and sort afterwards, should be cheaper.
            strains.push(StrainPeak::new(
                strain.value * lerp(REDUCED_STRAIN_BASELINE, 1.0, scale),
                CHUNK_SIZE.min(strain.section_length - added_time),
            ));

            added_time += CHUNK_SIZE;
        }

        time += strain.section_length;
        skip_count += 1;
    }

    let mut remaining: Vec<StrainPeak> = strains.into_iter().skip(skip_count).collect();
    remaining.sort_by(|a, b| b.value.total_cmp(&a.value));

    remaining
}

const fn lerp(start: f64, end: f64, amount: f64) -> f64 {
    start + (end - start) * amount
}
