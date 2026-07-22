//! Sample-format conversion and channel downmix (pure functions).

/// Convert an `f32` sample in `[-1.0, 1.0]` to `i16`, saturating.
#[inline]
pub fn f32_to_i16(s: f32) -> i16 {
    let clamped = s.clamp(-1.0, 1.0);
    // Scale into i16 range; round-half-away keeps symmetry around zero.
    (clamped * 32767.0).round() as i16
}

/// Convert an `i16` sample to `f32` in `[-1.0, 1.0]`.
#[inline]
pub fn i16_to_f32(s: i16) -> f32 {
    f32::from(s) / 32768.0
}

/// Convert a `u16` (offset-binary, cpal `SampleFormat::U16`) sample to `i16`.
#[inline]
pub fn u16_to_i16(s: u16) -> i16 {
    (i32::from(s) - 32768) as i16
}

/// Downmix interleaved frames to mono by averaging channels.
///
/// `channels` must be ≥ 1; a trailing partial frame (fewer samples than
/// `channels`) is dropped. For `channels == 1` this is a copy.
pub fn downmix_to_mono_i16(interleaved: &[i16], channels: usize) -> Vec<i16> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|&s| i32::from(s)).sum();
            // channels ≤ 32 in practice; i32 cannot overflow here.
            (sum / channels as i32) as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f32_conversion_saturates_and_round_trips() {
        assert_eq!(f32_to_i16(2.0), 32767);
        assert_eq!(f32_to_i16(-2.0), -32767);
        assert_eq!(f32_to_i16(0.0), 0);
        let x = 12345i16;
        let rt = f32_to_i16(i16_to_f32(x));
        assert!((i32::from(rt) - i32::from(x)).abs() <= 1, "rt={rt}");
    }

    #[test]
    fn u16_offset_binary_maps_midpoint_to_zero() {
        assert_eq!(u16_to_i16(32768), 0);
        assert_eq!(u16_to_i16(0), i16::MIN);
        assert_eq!(u16_to_i16(65535), i16::MAX);
    }

    #[test]
    fn stereo_downmix_averages_and_drops_partial_frame() {
        let stereo = [100i16, 200, -100, -200, 7]; // trailing lone sample dropped
        assert_eq!(downmix_to_mono_i16(&stereo, 2), vec![150, -150]);
        let mono = [1i16, 2, 3];
        assert_eq!(downmix_to_mono_i16(&mono, 1), vec![1, 2, 3]);
    }
}
