/// Loudness of a buffer for a level meter, from 0.0 (silent) to 1.0 (full
/// scale). RMS on a decibel scale, because a linear meter barely moves for
/// ordinary speech.
pub fn level(samples: &[i16]) -> f32 {
    const FLOOR_DB: f32 = -60.0;
    if samples.is_empty() {
        return 0.0;
    }
    let sum_of_squares: f64 = samples
        .iter()
        .map(|&sample| {
            let normalized = f64::from(sample) / f64::from(i16::MAX);
            normalized * normalized
        })
        .sum();
    let rms = (sum_of_squares / samples.len() as f64).sqrt() as f32;
    if rms <= 0.0 {
        return 0.0;
    }
    let decibels = 20.0 * rms.log10();
    ((decibels - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_and_empty_buffers_read_zero() {
        assert_eq!(level(&[]), 0.0);
        assert_eq!(level(&[0; 480]), 0.0);
    }

    #[test]
    fn full_scale_reads_one() {
        assert!((level(&[i16::MAX; 480]) - 1.0).abs() < 1e-3);
    }

    #[test]
    fn louder_reads_higher_and_speech_levels_are_visible() {
        // Roughly -30 dBFS, a quiet talker: should sit mid-meter, not at zero.
        let quiet = level(&[1_000; 480]);
        let loud = level(&[10_000; 480]);
        assert!(quiet > 0.3 && quiet < 0.7, "quiet = {quiet}");
        assert!(loud > quiet);
    }
}
