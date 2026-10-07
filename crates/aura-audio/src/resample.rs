/// Changes the sample rate of a whole clip by linear interpolation.
///
/// Good enough for speech fixtures in development. Live capture never uses
/// this: the native layer converts with a proper resampler.
pub fn resample(samples: &[i16], from: u32, to: u32) -> Vec<i16> {
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let length = (samples.len() as u64 * u64::from(to) / u64::from(from)) as usize;
    let step = f64::from(from) / f64::from(to);
    (0..length)
        .map(|index| {
            let position = index as f64 * step;
            let left = position.floor() as usize;
            let right = (left + 1).min(samples.len() - 1);
            let fraction = position - left as f64;
            let value = f64::from(samples[left]) * (1.0 - fraction) + f64::from(samples[right]) * fraction;
            value.round() as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_duration_when_changing_rate() {
        let second_at_24k = vec![100i16; 24_000];
        assert_eq!(resample(&second_at_24k, 24_000, 16_000).len(), 16_000);
        assert_eq!(resample(&second_at_24k, 24_000, 48_000).len(), 48_000);
    }

    #[test]
    fn interpolates_between_neighbouring_samples() {
        assert_eq!(resample(&[0, 100], 1, 2), [0, 50, 100, 100]);
        assert_eq!(resample(&[0, 30, 60, 90, 120, 150], 3, 2), [0, 45, 90, 135]);
    }

    #[test]
    fn the_same_rate_and_empty_input_pass_through() {
        assert_eq!(resample(&[1, 2, 3], 8, 8), [1, 2, 3]);
        assert!(resample(&[], 24_000, 16_000).is_empty());
    }
}
