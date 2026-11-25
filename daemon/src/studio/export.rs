//! How a film is joined: the filter graph ffmpeg is given for dissolves, fades through black,
//! per-shot volume, the film's own fades and the sound file under it. Nothing here runs anything.
//!
//! Everything is counted in frames at 24 a second, so the pictures and the sound of every shot
//! are exactly as long as each other and a long film does not drift.

const FPS: f64 = 24.0;
/// A dissolve lasts half a second; a fade through black takes the same half second in two halves.
const JOIN_FRAMES: u32 = 12;
/// The film's own fade in and fade out.
const EDGE_SECONDS: f64 = 0.5;
const SOUND: &str = "aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo";

/// How the film goes from one shot into the next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Join {
    Cut,
    Dissolve,
    Black,
}

impl Join {
    pub fn named(name: &str) -> Join {
        match name {
            "dissolve" => Join::Dissolve,
            "black" => Join::Black,
            _ => Join::Cut,
        }
    }
}

/// One made shot as it goes into the film. Input `i` of the ffmpeg command is the clip of piece `i`.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    /// Seconds cut from the start of the clip.
    pub start: f32,
    /// Frames kept after the trims.
    pub frames: u32,
    /// The join from the piece before into this one. Not looked at for the first piece.
    pub join: Join,
    /// 1.0 is the clip as it is; 0 is silence.
    pub volume: f32,
    /// Whether the clip has a sound stream at all.
    pub has_sound: bool,
}

/// What is done to the joined film as a whole.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Finish {
    pub fade_in: bool,
    pub fade_out: bool,
    /// 2 enlarges the picture to twice its width and height; anything else leaves it.
    pub scale: u32,
    /// The ffmpeg input that holds the sound file, and how loud it is (0 to 1).
    pub music: Option<(usize, f32)>,
}

/// A filter graph and where its picture and its sound come out: a label such as `[v]`, or an
/// input stream such as `0:v` when nothing had to be filtered.
#[derive(Debug, Clone, PartialEq)]
pub struct Graph {
    pub filter: String,
    pub video: String,
    pub audio: String,
    pub frames: u32,
}

fn seconds(frames: u32) -> f64 {
    frames as f64 / FPS
}

/// A stream as a filter takes it: `[0:v]` for an input stream, the label itself otherwise.
fn pad(stream: &str) -> String {
    if stream.starts_with('[') {
        stream.to_string()
    } else {
        format!("[{stream}]")
    }
}

/// Frames two pieces are both on screen in a dissolve: never more than half of either.
fn overlap(before: &Piece, after: &Piece) -> u32 {
    JOIN_FRAMES.min(before.frames / 2).min(after.frames / 2)
}

/// Seconds a piece takes to go to black, or to come up from it.
fn dip(piece: &Piece) -> f64 {
    seconds((JOIN_FRAMES / 2).min(piece.frames / 2))
}

fn join_of(pieces: &[Piece], i: usize) -> Join {
    if i == 0 {
        Join::Cut
    } else {
        pieces[i].join
    }
}

/// True when the film is its clips one after another and nothing else: every join a cut, every
/// shot at its own volume. Such a film is joined without a filter graph, which is faster.
pub fn plain(pieces: &[Piece]) -> bool {
    pieces.iter().enumerate().all(|(i, p)| join_of(pieces, i) == Join::Cut && p.volume == 1.0 && p.has_sound)
}

/// Frames of the whole film: every piece, less what dissolves share.
pub fn film_frames(pieces: &[Piece]) -> u32 {
    let shared: u32 = (1..pieces.len()).filter(|i| pieces[*i].join == Join::Dissolve).map(|i| overlap(&pieces[i - 1], &pieces[i])).sum();
    pieces.iter().map(|p| p.frames).sum::<u32>() - shared
}

/// The graph that joins the pieces. Cuts and fades through black are joined end to end; a
/// dissolve lays the start of what follows over the end of what came before.
pub fn joined(pieces: &[Piece]) -> Graph {
    let mut steps: Vec<String> = Vec::new();
    for (i, p) in pieces.iter().enumerate() {
        let length = seconds(p.frames);
        let comes_up = join_of(pieces, i) == Join::Black;
        let goes_down = pieces.get(i + 1).is_some_and(|next| next.join == Join::Black);
        // Pictures: from the trim start, exactly `frames` of them (the last one held if the clip is short).
        let mut v = format!(
            "[{i}:v]trim=start={:.3},setpts=PTS-STARTPTS,fps=24,tpad=stop=-1:stop_mode=clone,trim=end_frame={},format=yuv420p,setsar=1,settb=AVTB",
            p.start, p.frames
        );
        // Sound: the same stretch, padded with silence to the same length. A muted shot is silence.
        let heard = p.has_sound && p.volume > 0.0;
        let mut a = if heard {
            format!("[{i}:a]atrim=start={:.3},asetpts=PTS-STARTPTS,{SOUND},volume={:.2}", p.start, p.volume)
        } else {
            "anullsrc=r=48000:cl=stereo".to_string()
        };
        if comes_up {
            v.push_str(&format!(",fade=t=in:st=0:d={:.4}", dip(p)));
            if heard {
                a.push_str(&format!(",afade=t=in:st=0:d={:.4}", dip(p)));
            }
        }
        if goes_down {
            v.push_str(&format!(",fade=t=out:st={:.4}:d={:.4}", length - dip(p), dip(p)));
            if heard {
                a.push_str(&format!(",afade=t=out:st={:.4}:d={:.4}", length - dip(p), dip(p)));
            }
        }
        steps.push(format!("{v}[v{i}]"));
        steps.push(format!("{a},apad=whole_dur={length:.4},atrim=end={length:.4}[a{i}]"));
    }
    // Runs of pieces with no dissolve between them become one stream each.
    let mut runs: Vec<(String, String, u32, usize, usize)> = Vec::new();
    let mut first = 0;
    while first < pieces.len() {
        let mut last = first;
        while last + 1 < pieces.len() && pieces[last + 1].join != Join::Dissolve {
            last += 1;
        }
        let frames = pieces[first..=last].iter().map(|p| p.frames).sum();
        if first == last {
            runs.push((format!("[v{first}]"), format!("[a{first}]"), frames, first, last));
        } else {
            let inputs: String = (first..=last).map(|i| format!("[v{i}][a{i}]")).collect();
            steps.push(format!("{inputs}concat=n={}:v=1:a=1[rv{first}][ra{first}]", last - first + 1));
            runs.push((format!("[rv{first}]"), format!("[ra{first}]"), frames, first, last));
        }
        first = last + 1;
    }
    // The runs are laid over each other, one dissolve at a time.
    let mut runs = runs.into_iter();
    let Some((mut video, mut audio, mut frames, _, mut end)) = runs.next() else {
        return Graph { filter: String::new(), video: String::new(), audio: String::new(), frames: 0 };
    };
    for (v, a, more, start, last) in runs {
        let shared = overlap(&pieces[end], &pieces[start]);
        // A hair before the frame the dissolve starts on, so rounding cannot move it one frame on.
        let offset = seconds(frames - shared) - 0.002;
        steps.push(format!("{video}{v}xfade=transition=fade:duration={:.4}:offset={offset:.4}[xv{start}]", seconds(shared)));
        steps.push(format!("{audio}{a}acrossfade=d={:.4}:c1=tri:c2=tri[xa{start}]", seconds(shared)));
        (video, audio) = (format!("[xv{start}]"), format!("[xa{start}]"));
        frames = frames + more - shared;
        end = last;
    }
    Graph { filter: steps.join(";"), video, audio, frames }
}

/// The film as one stream, before anything is done to it as a whole: input 0 is all of it.
pub fn as_it_is(frames: u32) -> Graph {
    Graph { filter: String::new(), video: "0:v".into(), audio: "0:a".into(), frames }
}

/// Adds what is done to the whole film: enlarging, its fade in and fade out, the sound file.
pub fn finished(film: Graph, finish: &Finish) -> Graph {
    let total = seconds(film.frames);
    let edge = EDGE_SECONDS.min(total / 2.0);
    let mut steps: Vec<String> = if film.filter.is_empty() { Vec::new() } else { vec![film.filter.clone()] };
    let mut look: Vec<String> = Vec::new();
    let mut hear: Vec<String> = Vec::new();
    if finish.scale == 2 {
        look.push("scale=iw*2:ih*2:flags=lanczos".into());
    }
    if finish.fade_in {
        look.push(format!("fade=t=in:st=0:d={edge:.4}"));
        hear.push(format!("afade=t=in:st=0:d={edge:.4}"));
    }
    if finish.fade_out {
        look.push(format!("fade=t=out:st={:.4}:d={edge:.4}", total - edge));
        hear.push(format!("afade=t=out:st={:.4}:d={edge:.4}", total - edge));
    }
    let mut out = film.clone();
    if !look.is_empty() {
        steps.push(format!("{}{}[v]", pad(&film.video), look.join(",")));
        out.video = "[v]".into();
    }
    if let Some((input, volume)) = finish.music {
        // The sound file runs under the shots' own sound, as long as the film.
        steps.push(format!("[{input}:a]{SOUND},volume={volume:.2}[m]"));
        hear.insert(0, "amix=inputs=2:duration=first:dropout_transition=0:normalize=0".into());
        steps.push(format!("{}[m]{}[a]", pad(&film.audio), hear.join(",")));
        out.audio = "[a]".into();
    } else if !hear.is_empty() {
        steps.push(format!("{}{}[a]", pad(&film.audio), hear.join(",")));
        out.audio = "[a]".into();
    }
    out.filter = steps.join(";");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn piece(frames: u32, join: Join) -> Piece {
        Piece { start: 0.0, frames, join, volume: 1.0, has_sound: true }
    }

    const PICTURE: &str = "setpts=PTS-STARTPTS,fps=24,tpad=stop=-1:stop_mode=clone";
    const LOOK: &str = "format=yuv420p,setsar=1,settb=AVTB";

    #[test]
    fn a_film_of_cuts_needs_no_graph() {
        let film = vec![piece(48, Join::Cut), piece(48, Join::Cut), piece(120, Join::Cut)];
        assert!(plain(&film));
        assert!(plain(&film[..1]));
        // The join of the first piece is never looked at.
        assert!(plain(&[piece(48, Join::Dissolve), piece(48, Join::Cut)]));
        assert!(!plain(&[piece(48, Join::Cut), piece(48, Join::Dissolve)]));
        assert!(!plain(&[piece(48, Join::Cut), piece(48, Join::Black)]));
        let mut quiet = film.clone();
        quiet[1].volume = 0.5;
        assert!(!plain(&quiet));
        let mut silent = film.clone();
        silent[2].has_sound = false;
        assert!(!plain(&silent));
        assert_eq!(film_frames(&film), 216);
        // Nothing to do to the whole film: the joined clips go to the file as they are.
        let whole = finished(as_it_is(216), &Finish::default());
        assert_eq!((whole.filter.as_str(), whole.video.as_str(), whole.audio.as_str()), ("", "0:v", "0:a"));
    }

    #[test]
    fn one_shot() {
        let mut only = piece(56, Join::Dissolve);
        only.start = 0.5;
        only.volume = 1.5;
        let g = joined(&[only]);
        assert_eq!(
            g.filter,
            format!(
                "[0:v]trim=start=0.500,{PICTURE},trim=end_frame=56,{LOOK}[v0];\
                 [0:a]atrim=start=0.500,asetpts=PTS-STARTPTS,{SOUND},volume=1.50,apad=whole_dur=2.3333,atrim=end=2.3333[a0]"
            )
        );
        assert_eq!((g.video.as_str(), g.audio.as_str(), g.frames), ("[v0]", "[a0]", 56));
    }

    #[test]
    fn two_shots_dissolve() {
        let g = joined(&[piece(48, Join::Cut), piece(48, Join::Dissolve)]);
        let steps: Vec<&str> = g.filter.split(';').collect();
        assert_eq!(steps.len(), 6);
        // The second shot starts half a second before the first ends.
        assert_eq!(steps[4], "[v0][v1]xfade=transition=fade:duration=0.5000:offset=1.4980[xv1]");
        assert_eq!(steps[5], "[a0][a1]acrossfade=d=0.5000:c1=tri:c2=tri[xa1]");
        assert_eq!((g.video.as_str(), g.audio.as_str(), g.frames), ("[xv1]", "[xa1]", 84));
        assert_eq!(film_frames(&[piece(48, Join::Cut), piece(48, Join::Dissolve)]), 84);
    }

    #[test]
    fn two_shots_through_black() {
        let g = joined(&[piece(48, Join::Cut), piece(48, Join::Black)]);
        let steps: Vec<&str> = g.filter.split(';').collect();
        assert_eq!(steps.len(), 5);
        assert!(steps[0].ends_with(",fade=t=out:st=1.7500:d=0.2500[v0]"));
        assert!(steps[1].contains(",afade=t=out:st=1.7500:d=0.2500,apad="));
        assert!(steps[2].ends_with(",fade=t=in:st=0:d=0.2500[v1]"));
        assert!(steps[3].contains(",afade=t=in:st=0:d=0.2500,apad="));
        assert_eq!(steps[4], "[v0][a0][v1][a1]concat=n=2:v=1:a=1[rv0][ra0]");
        assert_eq!((g.video.as_str(), g.frames), ("[rv0]", 96));
    }

    #[test]
    fn three_shots_mixed_with_trims_and_volumes() {
        let mut second = piece(26, Join::Black);
        second.start = 0.5;
        second.volume = 0.0;
        let mut third = piece(56, Join::Dissolve);
        third.has_sound = false;
        let film = [piece(56, Join::Cut), second, third];
        let g = joined(&film);
        let steps: Vec<&str> = g.filter.split(';').collect();
        assert_eq!(steps.len(), 9);
        assert!(steps[2].starts_with("[1:v]trim=start=0.500,") && steps[2].contains("trim=end_frame=26,"));
        // A muted shot and a clip without sound are both silence of the shot's length.
        assert_eq!(steps[3], "anullsrc=r=48000:cl=stereo,apad=whole_dur=1.0833,atrim=end=1.0833[a1]");
        assert_eq!(steps[5], "anullsrc=r=48000:cl=stereo,apad=whole_dur=2.3333,atrim=end=2.3333[a2]");
        // The first two join through black into one stream; the third dissolves over its end.
        assert_eq!(steps[6], "[v0][a0][v1][a1]concat=n=2:v=1:a=1[rv0][ra0]");
        assert_eq!(steps[7], "[rv0][v2]xfade=transition=fade:duration=0.5000:offset=2.9147[xv2]");
        assert_eq!(steps[8], "[ra0][a2]acrossfade=d=0.5000:c1=tri:c2=tri[xa2]");
        assert_eq!(g.frames, 56 + 26 + 56 - 12);
        assert_eq!(film_frames(&film), g.frames);
    }

    #[test]
    fn many_shots() {
        // Sixty shots: every third join a dissolve, every fifth through black, the rest cuts.
        let film: Vec<Piece> = (0..60)
            .map(|i| {
                piece(
                    120,
                    if i % 3 == 0 {
                        Join::Dissolve
                    } else if i % 5 == 0 {
                        Join::Black
                    } else {
                        Join::Cut
                    },
                )
            })
            .collect();
        let g = joined(&film);
        let dissolves = (1..60).filter(|i| i % 3 == 0).count();
        assert_eq!(g.filter.matches("xfade=").count(), dissolves);
        assert_eq!(g.filter.matches("acrossfade=").count(), dissolves);
        assert_eq!(g.filter.matches("fade=t=in:st=0").count(), 2 * (1..60).filter(|i| i % 5 == 0 && i % 3 != 0).count());
        assert_eq!(g.frames, 60 * 120 - 12 * dissolves as u32);
        assert_eq!(g.frames, film_frames(&film));
        // Every clip is read once for pictures and once for sound, and every label made is used.
        for i in 0..60 {
            assert_eq!(g.filter.matches(&format!("[{i}:v]")).count(), 1);
            assert_eq!(g.filter.matches(&format!("[{i}:a]")).count(), 1);
            assert_eq!(g.filter.matches(&format!("[v{i}]")).count(), 2);
            assert_eq!(g.filter.matches(&format!("[a{i}]")).count(), 2);
        }
        // The last dissolve starts where the film so far ends, less the half second they share.
        let before_last: u32 = 57 * 120 - 12 * 18;
        assert!(g.filter.contains(&format!("offset={:.4}[xv57]", (before_last - 12) as f64 / 24.0 - 0.002)));
        assert_eq!((g.video.as_str(), g.audio.as_str()), ("[xv57]", "[xa57]"));
    }

    #[test]
    fn a_join_is_never_longer_than_half_a_shot() {
        let g = joined(&[piece(120, Join::Cut), piece(12, Join::Dissolve), piece(10, Join::Black)]);
        // Six frames of dissolve into the short shot, and five frames each way through black.
        assert!(g.filter.contains("xfade=transition=fade:duration=0.2500:offset=4.7480"));
        assert!(g.filter.contains("fade=t=out:st=0.2500:d=0.2500[v1]"));
        assert!(g.filter.contains("fade=t=in:st=0:d=0.2083[v2]"));
        assert_eq!(g.frames, 120 + 12 + 10 - 6);
    }

    #[test]
    fn the_whole_film_fades_and_takes_the_sound_file() {
        let film = [piece(48, Join::Cut), piece(48, Join::Dissolve)];
        let all = Finish { fade_in: true, fade_out: true, scale: 2, music: Some((2, 0.4)) };
        let g = finished(joined(&film), &all);
        let steps: Vec<&str> = g.filter.split(';').collect();
        assert_eq!(steps[6], "[xv1]scale=iw*2:ih*2:flags=lanczos,fade=t=in:st=0:d=0.5000,fade=t=out:st=3.0000:d=0.5000[v]");
        assert_eq!(steps[7], format!("[2:a]{SOUND},volume=0.40[m]"));
        assert_eq!(
            steps[8],
            "[xa1][m]amix=inputs=2:duration=first:dropout_transition=0:normalize=0,afade=t=in:st=0:d=0.5000,afade=t=out:st=3.0000:d=0.5000[a]"
        );
        assert_eq!((g.video.as_str(), g.audio.as_str(), g.frames), ("[v]", "[a]", 84));
        // A film of cuts stays one input; only what is asked for is added.
        let cuts = finished(as_it_is(96), &Finish { fade_out: true, ..Finish::default() });
        assert_eq!(cuts.filter, "[0:v]fade=t=out:st=3.5000:d=0.5000[v];[0:a]afade=t=out:st=3.5000:d=0.5000[a]");
        let music = finished(as_it_is(96), &Finish { music: Some((1, 1.0)), ..Finish::default() });
        assert_eq!(
            music.filter,
            format!("[1:a]{SOUND},volume=1.00[m];[0:a][m]amix=inputs=2:duration=first:dropout_transition=0:normalize=0[a]")
        );
        assert_eq!((music.video.as_str(), music.audio.as_str()), ("0:v", "[a]"));
        // A film shorter than a second fades for half of itself each way.
        let short = finished(as_it_is(12), &Finish { fade_in: true, fade_out: true, ..Finish::default() });
        assert!(short.filter.contains("fade=t=in:st=0:d=0.2500,fade=t=out:st=0.2500:d=0.2500[v]"));
    }
}
