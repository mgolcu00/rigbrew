//! How long shots take on this machine: every made shot is a measurement, and the time a film
//! will take is read from the measurements of shots like the ones it still needs.

use serde::{Deserialize, Serialize};

/// One made shot: how many frames were asked for, whether it continued the shot before it
/// (those start from a picture and take longer), and the seconds it took.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub frames: u32,
    #[serde(default)]
    pub continued: bool,
    pub seconds: f32,
}

/// Seconds of work for one second of film when nothing has been measured yet.
pub const PLAIN: f32 = 25.0;
pub const CONTINUED: f32 = 80.0;

/// The newest measurements are kept; a machine changes, and so does what else runs on it.
const KEPT: usize = 60;

pub fn remember(samples: &mut Vec<Sample>, sample: Sample) {
    if sample.frames == 0 || !sample.seconds.is_finite() || sample.seconds <= 0.0 {
        return;
    }
    samples.push(sample);
    if samples.len() > KEPT {
        let extra = samples.len() - KEPT;
        samples.drain(..extra);
    }
}

/// Seconds per frame over a set of measurements.
fn rate<'a>(samples: impl Iterator<Item = &'a Sample>) -> Option<f32> {
    let (seconds, frames) = samples.fold((0.0, 0u32), |(s, f), x| (s + x.seconds, f + x.frames));
    (frames > 0).then(|| seconds / frames as f32)
}

/// About how many seconds a shot of `frames` frames takes to make.
pub fn estimate(samples: &[Sample], frames: u32, continued: bool) -> f32 {
    let kind = || samples.iter().filter(move |s| s.continued == continued);
    // Shots of this very length say it best: the time does not grow evenly with the length.
    let same: Vec<f32> = kind().rev().filter(|s| s.frames == frames).take(5).map(|s| s.seconds).collect();
    if !same.is_empty() {
        return same.iter().sum::<f32>() / same.len() as f32;
    }
    if let Some(per_frame) = rate(kind()) {
        return per_frame * frames as f32;
    }
    // Only the other kind was measured: the usual ratio between the two carries it over.
    if let Some(per_frame) = rate(samples.iter().filter(|s| s.continued != continued)) {
        let ratio = if continued { CONTINUED / PLAIN } else { PLAIN / CONTINUED };
        return per_frame * ratio * frames as f32;
    }
    frames as f32 / 24.0 * if continued { CONTINUED } else { PLAIN }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(frames: u32, continued: bool, seconds: f32) -> Sample {
        Sample { frames, continued, seconds }
    }

    #[test]
    fn nothing_measured_falls_back_to_the_usual_pace() {
        // Five seconds of film: about two minutes plain, more than six when it continues.
        assert_eq!(estimate(&[], 120, false), 125.0);
        assert_eq!(estimate(&[], 120, true), 400.0);
        assert_eq!(estimate(&[], 48, false), 50.0);
    }

    #[test]
    fn shots_of_the_same_length_and_kind_are_the_measure() {
        let seen = [s(49, false, 90.0), s(49, false, 110.0), s(121, false, 300.0), s(49, true, 140.0)];
        assert_eq!(estimate(&seen, 49, false), 100.0);
        assert_eq!(estimate(&seen, 121, false), 300.0);
        assert_eq!(estimate(&seen, 49, true), 140.0);
        // Only the last five of a length count.
        let mut many: Vec<Sample> = (0..10).map(|i| s(49, false, if i < 5 { 1000.0 } else { 100.0 })).collect();
        assert_eq!(estimate(&many, 49, false), 100.0);
        // Old measurements leave when there are too many.
        for _ in 0..100 {
            remember(&mut many, s(121, true, 500.0));
        }
        assert_eq!(many.len(), 60);
        assert!(many.iter().all(|x| x.frames == 121));
        remember(&mut many, s(0, false, 10.0));
        remember(&mut many, s(49, false, f32::NAN));
        assert_eq!(many.len(), 60);
    }

    #[test]
    fn another_length_or_kind_is_scaled() {
        let plain = [s(49, false, 98.0)];
        // Another length of the same kind: by the frame.
        assert_eq!(estimate(&plain, 98, false), 196.0);
        // The other kind only: by the frame and the usual ratio between the two.
        assert!((estimate(&plain, 49, true) - 98.0 * 3.2).abs() < 0.01);
        assert!((estimate(&[s(49, true, 160.0)], 49, false) - 50.0).abs() < 0.01);
    }
}
