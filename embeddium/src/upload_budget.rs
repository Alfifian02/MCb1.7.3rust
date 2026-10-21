//! Sodium's upload budget: `UploadResourceBudget` (`LimitedResourceBudget` / `UnlimitedResourceBudget`) and
//! `UploadDurationEstimator` (`ExpDecayLinear2DEstimator`), as plain CPU bookkeeping. Times are nanoseconds, sizes are bytes.
//!
//! A frame may spend a fraction of its length and a number of bytes on uploading finished meshes; the rest waits for the next
//! frame, so many chunks finishing in the same frame no longer make one long frame. The cost of an upload is not known in advance:
//! [`UploadDurationEstimator`] fits `duration = intercept + slope * size` to what the host measured around its `write_buffer`
//! calls, with the weight of old data decaying exponentially (a straight port of `ExpDecayLinear2DEstimator`'s update).
//!
//! Left out on purpose: `MeshTaskSizeEstimator` and `JobDurationEstimator`. Sodium has to guess a task's size before the task
//! runs (it budgets when it submits the job); here the worker has already built the mesh when the host decides to upload it, so
//! the size is exact and there is nothing to estimate.

/// `RenderSectionManager.FRAME_DURATION_UPLOAD_FRACTION`: the share of the average frame given to uploads.
pub const FRAME_DURATION_UPLOAD_FRACTION: f64 = 0.3;
/// `RenderSectionManager.MIN_UPLOAD_DURATION_BUDGET`: 2 ms, whatever the frame length. (Sodium's source says "2ms" in the comment
/// and writes `10_000_000L`, which is 10 ms; the comment is taken as the intent: 10 ms would be the whole of a 60 Hz frame.)
pub const MIN_UPLOAD_DURATION_BUDGET: u64 = 2_000_000;
/// `RenderSectionManager.FRAME_DURATION_UPDATE_RATIO`: weight of the newest frame in the average frame length.
pub const FRAME_DURATION_UPDATE_RATIO: f64 = 0.05;
/// The average frame length is clamped to this range (Sodium: 1.0001 ms .. 100 ms).
pub const FRAME_DURATION_RANGE: (f64, f64) = (1_000_100.0, 100_000_000.0);

/// `UploadDurationEstimator`'s constants.
pub const NEW_DATA_RATIO: f64 = 0.05;
pub const INITIAL_SAMPLE_TARGET: u32 = 100;
pub const MIN_BATCH_SIZE: usize = 100;
/// What an upload is guessed to cost until the first fit: 100 microseconds.
pub const INITIAL_UPLOAD_TIME_ESTIMATE: u64 = 100_000;

/// `MathUtil.exponentialMovingAverage`.
pub fn exponential_moving_average(old: f64, new: f64, new_contribution: f64) -> f64 {
    new_contribution * new + (1.0 - new_contribution) * old
}

/// The average frame length after a frame of `frame_ns` (`None` = the first frame: it is the average).
pub fn next_average_frame(average: Option<f64>, frame_ns: f64) -> f64 {
    let avg = average.map_or(frame_ns, |a| exponential_moving_average(a, frame_ns, FRAME_DURATION_UPDATE_RATIO));
    avg.clamp(FRAME_DURATION_RANGE.0, FRAME_DURATION_RANGE.1)
}

/// The duration share of a frame's budget for an average frame of `average_frame_ns`.
pub fn frame_duration_budget(average_frame_ns: f64) -> u64 {
    ((average_frame_ns * FRAME_DURATION_UPLOAD_FRACTION) as u64).max(MIN_UPLOAD_DURATION_BUDGET)
}

/// `UploadResourceBudget`: what a frame may still spend on uploads. Spent amounts are subtracted and may overshoot (`consume`),
/// like Sodium's; [`UploadBudget::fits`] is the stricter question the host can ask when it knows the size beforehand.
#[derive(Copy, Clone, Debug)]
pub struct UploadBudget {
    limited: bool,
    duration: i64,
    size: i64,
}

fn clamp_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

impl UploadBudget {
    /// `UnlimitedResourceBudget`: always available, consumes nothing.
    pub fn unlimited() -> Self {
        Self { limited: false, duration: 0, size: 0 }
    }

    /// `LimitedResourceBudget(duration, size)`.
    pub fn limited(duration_ns: u64, size_bytes: u64) -> Self {
        Self { limited: true, duration: clamp_i64(duration_ns), size: clamp_i64(size_bytes) }
    }

    /// `isAvailable`: some duration and some size are left.
    pub fn is_available(&self) -> bool {
        !self.limited || (self.duration > 0 && self.size > 0)
    }

    /// An upload of `size_bytes` that is estimated to take `duration_ns` is available and does not exceed what is left.
    pub fn fits(&self, duration_ns: u64, size_bytes: u64) -> bool {
        !self.limited || (self.is_available() && clamp_i64(duration_ns) <= self.duration && clamp_i64(size_bytes) <= self.size)
    }

    /// `consume`.
    pub fn consume(&mut self, duration_ns: u64, size_bytes: u64) {
        if self.limited {
            self.duration = self.duration.saturating_sub(clamp_i64(duration_ns));
            self.size = self.size.saturating_sub(clamp_i64(size_bytes));
        }
    }

    pub fn remaining(&self) -> (i64, i64) {
        (self.duration, self.size)
    }
}

/// `UploadDurationEstimator`: a linear fit of upload duration (ns) over upload size (bytes), updated in batches.
///
/// `add_data` collects `(size, duration)` pairs; `update_models` folds them into the fit once at least [`MIN_BATCH_SIZE`] are
/// waiting (fewer are kept for the next call, like `ClearingLinearRegressionBatch`). The first [`INITIAL_SAMPLE_TARGET`] samples
/// weigh equally; after that a batch weighs `NEW_DATA_RATIO` of the total, so the fit follows a device that changes (thermal
/// throttling) without jumping on one slow frame. A negative slope is clamped to a flat line.
#[derive(Clone, Debug)]
pub struct UploadDurationEstimator {
    batch: Vec<(u64, u64)>,
    gathered: u32,
    x_mean: f64,
    y_mean: f64,
    covariance: f64,
    variance: f64,
    slope: f64,
    intercept: f64,
}

impl Default for UploadDurationEstimator {
    fn default() -> Self {
        Self { batch: Vec::new(), gathered: 0, x_mean: 0.0, y_mean: 0.0, covariance: 0.0, variance: 0.0, slope: 0.0, intercept: 0.0 }
    }
}

impl UploadDurationEstimator {
    pub fn new() -> Self {
        Self::default()
    }

    /// `estimateUploadDuration`: the predicted duration (ns) of uploading `size` bytes.
    pub fn estimate(&self, size: u64) -> u64 {
        if self.gathered == 0 {
            return INITIAL_UPLOAD_TIME_ESTIMATE;
        }
        (self.intercept + self.slope * size as f64).max(0.0) as u64
    }

    /// `addData(new UploadDuration(duration, size))`.
    pub fn add_data(&mut self, size: u64, duration_ns: u64) {
        self.batch.push((size, duration_ns));
    }

    /// Samples waiting for `update_models`.
    pub fn pending(&self) -> usize {
        self.batch.len()
    }

    /// `updateModels`.
    pub fn update_models(&mut self) {
        if self.batch.len() < MIN_BATCH_SIZE {
            return; // empty, or too small: keep collecting (the batch is not cleared)
        }
        self.fit();
        self.batch.clear();
    }

    fn fit(&mut self) {
        let n = self.batch.len() as f64;
        let total_samples = self.gathered + self.batch.len() as u32;
        let (old_weight, total_weight);
        if total_samples <= INITIAL_SAMPLE_TARGET {
            total_weight = total_samples as f64;
            old_weight = self.gathered as f64;
            self.gathered = total_samples;
        } else {
            old_weight = n / NEW_DATA_RATIO - n;
            total_weight = old_weight + n;
        }
        let inv = 1.0 / total_weight;

        let (mut x_sum, mut y_sum) = (0.0f64, 0.0f64);
        for &(x, y) in &self.batch {
            x_sum += x as f64;
            y_sum += y as f64;
        }
        let x_mean = (self.x_mean * old_weight + x_sum) * inv;
        let y_mean = (self.y_mean * old_weight + y_sum) * inv;

        let (mut covariance_sum, mut variance_sum) = (0.0f64, 0.0f64);
        for &(x, y) in &self.batch {
            let (dx, dy) = (x as f64 - x_mean, y as f64 - y_mean);
            covariance_sum += dx * dy;
            variance_sum += dx * dx;
        }

        if variance_sum.abs() <= f64::MIN_POSITIVE {
            // Every size alike: no slope to find. Sodium returns here with the line still at 0 (a prediction of 0 ns once a
            // sample was counted); a flat line through the mean is the honest fit.
            self.slope = 0.0;
            self.intercept = y_mean;
            self.x_mean = x_mean;
            self.y_mean = y_mean;
            return;
        }

        covariance_sum += self.covariance * old_weight;
        variance_sum += self.variance * old_weight;

        self.slope = (covariance_sum / variance_sum).max(0.0);
        self.intercept = y_mean - self.slope * x_mean;

        self.x_mean = x_mean;
        self.y_mean = y_mean;
        self.covariance = covariance_sum * inv;
        self.variance = variance_sum * inv;
    }

    /// `toString`: `s=<slope>,y=<intercept>` (for a debug line).
    pub fn describe(&self) -> String {
        format!("s={:.2},y={:.0}", self.slope, self.intercept)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_is_always_available_and_never_runs_out() {
        let mut b = UploadBudget::unlimited();
        b.consume(u64::MAX, u64::MAX);
        assert!(b.is_available() && b.fits(u64::MAX, u64::MAX));
    }

    #[test]
    fn limited_runs_out_of_either_resource() {
        let mut b = UploadBudget::limited(1_000, 10_000);
        assert!(b.is_available());
        b.consume(400, 4_000);
        assert!(b.is_available() && b.fits(600, 6_000) && !b.fits(601, 1) && !b.fits(1, 6_001));
        b.consume(600, 100); // duration exactly spent
        assert!(!b.is_available());
        let mut c = UploadBudget::limited(1_000, 10_000);
        c.consume(1, 10_000); // size exactly spent
        assert!(!c.is_available());
    }

    #[test]
    fn consume_may_overshoot_like_sodium() {
        let mut b = UploadBudget::limited(100, 100);
        b.consume(500, 500);
        assert_eq!(b.remaining(), (-400, -400));
        assert!(!b.is_available());
    }

    #[test]
    fn frame_budget_is_a_third_of_the_frame_with_a_floor() {
        assert_eq!(frame_duration_budget(16_666_667.0), 5_000_000);
        assert_eq!(frame_duration_budget(4_000_000.0), MIN_UPLOAD_DURATION_BUDGET);
    }

    #[test]
    fn average_frame_starts_at_the_first_frame_and_stays_clamped() {
        assert_eq!(next_average_frame(None, 16_000_000.0), 16_000_000.0);
        assert_eq!(next_average_frame(None, 1.0), FRAME_DURATION_RANGE.0);
        assert_eq!(next_average_frame(Some(16_000_000.0), 1e12), FRAME_DURATION_RANGE.1);
        let ema = next_average_frame(Some(20_000_000.0), 10_000_000.0);
        assert!((ema - 19_500_000.0).abs() < 1.0, "{ema}");
    }

    #[test]
    fn estimator_guesses_before_it_has_data() {
        let mut e = UploadDurationEstimator::new();
        assert_eq!(e.estimate(123_456), INITIAL_UPLOAD_TIME_ESTIMATE);
        for i in 0..(MIN_BATCH_SIZE - 1) as u64 {
            e.add_data(1_000 + i, 5_000);
        }
        e.update_models(); // one short of a batch: nothing is fitted, nothing is dropped
        assert_eq!(e.estimate(5_000), INITIAL_UPLOAD_TIME_ESTIMATE);
        assert_eq!(e.pending(), MIN_BATCH_SIZE - 1);
    }

    #[test]
    fn estimator_learns_a_line() {
        let mut e = UploadDurationEstimator::new();
        for i in 0..MIN_BATCH_SIZE as u64 {
            let size = 10_000 + i * 700;
            e.add_data(size, 2_000 + size * 3 / 2); // 2 us + 1.5 ns per byte
        }
        e.update_models();
        assert_eq!(e.pending(), 0);
        let got = e.estimate(100_000) as f64;
        let want = 2_000.0 + 150_000.0;
        assert!((got - want).abs() / want < 0.01, "got {got}, want {want}");
    }

    #[test]
    fn estimator_follows_a_change_slowly() {
        let mut e = UploadDurationEstimator::new();
        let batch = |e: &mut UploadDurationEstimator, per_byte: u64| {
            for i in 0..MIN_BATCH_SIZE as u64 {
                let size = 10_000 + i * 700;
                e.add_data(size, size * per_byte);
            }
            e.update_models();
        };
        batch(&mut e, 1);
        let before = e.estimate(50_000);
        batch(&mut e, 4); // a device that became 4x slower: past the initial sample target, the new batch weighs 5%
        let after = e.estimate(50_000);
        assert!(after > before && after < before * 2, "{before} -> {after}");
    }

    #[test]
    fn a_falling_trend_is_a_flat_line_not_a_negative_cost() {
        let mut e = UploadDurationEstimator::new();
        for i in 0..MIN_BATCH_SIZE as u64 {
            e.add_data(10_000 + i * 100, 900_000 - i * 1_000); // bigger uploads measured faster: noise
        }
        e.update_models();
        assert_eq!(e.estimate(0), e.estimate(1_000_000));
    }

    #[test]
    fn equal_sizes_predict_the_mean_not_zero() {
        let mut e = UploadDurationEstimator::new();
        for _ in 0..MIN_BATCH_SIZE {
            e.add_data(4_096, 30_000);
        }
        e.update_models();
        assert_eq!(e.estimate(4_096), 30_000);
    }
}
