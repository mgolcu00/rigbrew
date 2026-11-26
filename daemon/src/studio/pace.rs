//! How long shots take on this machine: every made shot is a measurement, and the time a film
//! will take is read from the measurements of shots like the ones it still needs.

use serde::{Deserialize, Serialize};

/// One made shot: how many frames were asked for, how many points each picture had, whether it
/// continued the shot before it (those start from a picture and take longer), and the seconds it took.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub frames: u32,
    #[serde(default)]
    pub continued: bool,
    pub seconds: f32,
    /// Width times height; 0 for a shot measured before sizes were kept, which was a draft.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pixels: u32,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

/// Seconds of work for one second of film when nothing has been measured yet, at the draft size.
pub const PLAIN: f32 = 25.0;
pub const CONTINUED: f32 = 80.0;

/// The points of a draft picture (640 x 384): what a shot measured before sizes were kept had.
pub const DRAFT_PIXELS: u32 = 640 * 384;

/// How the work of a shot grows with its points (frames times width times height): more than
/// evenly, because every point of a clip looks at every other. Measured on an M5 Max with the
/// video model at three sizes (see the lab's results).
pub const GROWTH: f64 = 1.3;

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

fn pixels_of(s: &Sample) -> u32 {
    if s.pixels == 0 {
        DRAFT_PIXELS
    } else {
        s.pixels
    }
}

/// The shot work is counted against: five seconds of a draft.
const FIVE: f64 = 121.0;

/// The work of a shot, counted so that a five-second draft is 121: about one a frame for drafts
/// of the usual lengths, more for each frame of a larger picture.
pub fn work(frames: u32, pixels: u32) -> f64 {
    FIVE * (frames as f64 * pixels.max(1) as f64 / (FIVE * DRAFT_PIXELS as f64)).powf(GROWTH)
}

/// Seconds per unit of work over a set of measurements.
fn rate<'a>(samples: impl Iterator<Item = &'a Sample>) -> Option<f64> {
    let (seconds, units) = samples.fold((0.0, 0.0), |(s, u), x| (s + x.seconds as f64, u + work(x.frames, pixels_of(x))));
    (units > 0.0).then(|| seconds / units)
}

/// About how many seconds a shot of `frames` frames of `pixels` points each takes to make.
pub fn estimate(samples: &[Sample], frames: u32, pixels: u32, continued: bool) -> f32 {
    let pixels = if pixels == 0 { DRAFT_PIXELS } else { pixels };
    let kind = || samples.iter().filter(move |s| s.continued == continued);
    // Shots of this very length and size say it best.
    let same: Vec<f32> = kind().rev().filter(|s| s.frames == frames && pixels_of(s) == pixels).take(5).map(|s| s.seconds).collect();
    if !same.is_empty() {
        return same.iter().sum::<f32>() / same.len() as f32;
    }
    let units = work(frames, pixels);
    if let Some(per_unit) = rate(kind()) {
        return (per_unit * units) as f32;
    }
    // Only the other kind was measured: the usual ratio between the two carries it over.
    if let Some(per_unit) = rate(samples.iter().filter(|s| s.continued != continued)) {
        let ratio = if continued { CONTINUED / PLAIN } else { PLAIN / CONTINUED } as f64;
        return (per_unit * ratio * units) as f32;
    }
    // Nothing measured: the usual pace of a draft, grown as the work grows.
    let usual = if continued { CONTINUED } else { PLAIN } as f64;
    (usual / 24.0 * units) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(frames: u32, continued: bool, seconds: f32) -> Sample {
        Sample { frames, continued, seconds, pixels: 0 }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01 * b.abs().max(1.0)
    }

    #[test]
    fn nothing_measured_falls_back_to_the_usual_pace() {
        // Five seconds of a draft film: about two minutes plain, more than six when it continues.
        assert!(close(estimate(&[], 121, DRAFT_PIXELS, false), 126.0));
        assert!(close(estimate(&[], 121, 0, true), 403.3));
        assert!(close(estimate(&[], 120, DRAFT_PIXELS, false), 125.0));
        // Twice the points is more than twice the work.
        assert!(estimate(&[], 121, 2 * DRAFT_PIXELS, false) > 2.0 * estimate(&[], 121, DRAFT_PIXELS, false));
        assert!(close(work(121, DRAFT_PIXELS) as f32, 121.0));
    }

    #[test]
    fn shots_of_the_same_length_size_and_kind_are_the_measure() {
        let seen = [s(49, false, 90.0), s(49, false, 110.0), s(121, false, 300.0), s(49, true, 140.0)];
        assert_eq!(estimate(&seen, 49, DRAFT_PIXELS, false), 100.0);
        assert_eq!(estimate(&seen, 121, DRAFT_PIXELS, false), 300.0);
        assert_eq!(estimate(&seen, 49, DRAFT_PIXELS, true), 140.0);
        // Only the last five of a length count.
        let mut many: Vec<Sample> = (0..10).map(|i| s(49, false, if i < 5 { 1000.0 } else { 100.0 })).collect();
        assert_eq!(estimate(&many, 49, 0, false), 100.0);
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
    fn another_length_size_or_kind_is_scaled_by_its_work() {
        let plain = [s(49, false, 98.0)];
        // Another length of the same kind and size: by the work.
        assert!(close(estimate(&plain, 98, DRAFT_PIXELS, false), 98.0 * 2f32.powf(GROWTH as f32)));
        // Another size: the same.
        let standard = 512 * 896;
        let grown = (standard as f32 / DRAFT_PIXELS as f32).powf(GROWTH as f32);
        assert!(close(estimate(&plain, 49, standard, false), 98.0 * grown));
        // A measured shot of that size is the measure for it.
        let both = [s(49, false, 98.0), Sample { frames: 49, continued: false, seconds: 400.0, pixels: standard }];
        assert_eq!(estimate(&both, 49, standard, false), 400.0);
        // The other kind only: by the work and the usual ratio between the two.
        assert!(close(estimate(&plain, 49, DRAFT_PIXELS, true), 98.0 * 3.2));
        assert!(close(estimate(&[s(49, true, 160.0)], 49, DRAFT_PIXELS, false), 50.0));
    }

    #[test]
    fn a_sample_from_before_sizes_is_a_draft() {
        let old: Sample = serde_json::from_str(r#"{"frames":121,"continued":false,"seconds":250.0}"#).unwrap();
        assert_eq!((old.pixels, pixels_of(&old)), (0, DRAFT_PIXELS));
        assert_eq!(estimate(&[old], 121, 384 * 640, false), 250.0);
        // A draft is written as before, so an older app reads the file as it did.
        assert_eq!(serde_json::to_string(&s(121, false, 250.0)).unwrap(), r#"{"frames":121,"continued":false,"seconds":250.0}"#);
    }
}
