//! How a film is joined: the filter graph ffmpeg is given for dissolves, fades through black,
//! per-shot volume, the film's own fades, the sound file under it and the sound tracks mixed
//! into it. Nothing here runs anything.
//!
//! Everything is counted in frames at 24 a second, so the pictures and the sound of every shot
//! are exactly as long as each other and a long film does not drift.

const FPS: f64 = 24.0;
/// Samples a second of everything that is mixed.
const RATE: f64 = 48000.0;
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
    /// Frames the join into this piece lasts: a dissolve for all of them, a fade through black in two halves.
    pub join_frames: u32,
    /// The width and height the picture is brought to before it is joined: a film that holds
    /// clips of two sizes (made ones, and larger ones brought in from a video) gives every piece one.
    pub fit: Option<(u32, u32)>,
}

/// One region of a sound track as it goes into the film.
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    /// The ffmpeg input that holds its file.
    pub input: usize,
    /// Seconds into the file where the region starts, and how long it plays from there.
    pub offset: f64,
    pub length: f64,
    /// How many times that stretch plays, one after another.
    pub loops: u32,
    /// Seconds into the film where it starts. Before 0, what lies before the film is left out.
    pub start: f64,
    /// The region's own gain times its track's volume.
    pub gain: f64,
    /// -1 is all left, 1 all right: the side away from it is turned down.
    pub pan: f64,
    pub fade_in: f64,
    pub fade_out: f64,
}

/// Everything that is heard besides the picture's own joins: the shots' sound as one channel
/// strip, the regions of the tracks that are heard, and the master.
#[derive(Debug, Clone, PartialEq)]
pub struct Mix {
    /// The shots' own sound as a whole: 0 when its strip is muted or another is soloed.
    pub shots_gain: f64,
    pub shots_pan: f64,
    pub sounds: Vec<Sound>,
    pub master: f64,
    /// The stretches of the film in which the shots' own sound is turned down under what is
    /// said, and by how much: 0.6 leaves 0.4 of it.
    pub duck: Vec<(f64, f64)>,
    pub duck_by: f64,
}

impl Mix {
    /// Nothing to mix: the film's sound goes out as it is.
    pub fn is_plain(&self) -> bool {
        self.sounds.is_empty() && self.shots_gain == 1.0 && self.shots_pan == 0.0 && self.master == 1.0 && !self.ducks()
    }

    fn ducks(&self) -> bool {
        self.duck_by > 0.0 && !self.duck.is_empty()
    }
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
    /// The sound tracks and the master. `None` or a plain mix leaves the sound as it is.
    pub mix: Option<Mix>,
    /// Pictures of the subtitles to lay over the film: the ffmpeg input that holds them (one
    /// stream as wide as the finished film) and how far down it sits.
    pub captions: Option<(usize, u32)>,
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
    after.join_frames.min(before.frames / 2).min(after.frames / 2)
}

/// Seconds a piece takes to go to black, or to come up from it, in a join of `join` frames.
fn dip(piece: &Piece, join: u32) -> f64 {
    seconds((join / 2).min(piece.frames / 2))
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
    pieces.iter().enumerate().all(|(i, p)| join_of(pieces, i) == Join::Cut && p.volume == 1.0 && p.has_sound && p.fit.is_none())
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
        let after = pieces.get(i + 1).filter(|next| next.join == Join::Black);
        let (up, down) = (dip(p, p.join_frames), after.map_or(0.0, |next| dip(p, next.join_frames)));
        let goes_down = after.is_some();
        // Pictures: from the trim start, exactly `frames` of them (the last one held if the clip is short).
        let fit =
            p.fit.map_or(String::new(), |(w, h)| format!("scale={w}:{h}:force_original_aspect_ratio=increase:flags=lanczos,crop={w}:{h},"));
        let mut v = format!(
            "[{i}:v]trim=start={:.3},setpts=PTS-STARTPTS,fps=24,tpad=stop=-1:stop_mode=clone,trim=end_frame={},{fit}format=yuv420p,setsar=1,settb=AVTB",
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
            v.push_str(&format!(",fade=t=in:st=0:d={up:.4}"));
            if heard {
                a.push_str(&format!(",afade=t=in:st=0:d={up:.4}"));
            }
        }
        if goes_down {
            v.push_str(&format!(",fade=t=out:st={:.4}:d={down:.4}", length - down));
            if heard {
                a.push_str(&format!(",afade=t=out:st={:.4}:d={down:.4}", length - down));
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
    let mut seen = film.video.clone();
    if finish.scale == 2 && finish.captions.is_none() {
        look.push("scale=iw*2:ih*2:flags=lanczos".into());
    }
    if let Some((input, down)) = finish.captions {
        // The subtitles are drawn for the finished size: over the enlarged picture, under the fades.
        if finish.scale == 2 {
            steps.push(format!("{}scale=iw*2:ih*2:flags=lanczos[big]", pad(&seen)));
            seen = "[big]".into();
        }
        steps.push(format!("{}[{input}:v]overlay=x=0:y={down}:eof_action=pass:format=auto[sub]", pad(&seen)));
        seen = "[sub]".into();
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
        steps.push(format!("{}{}[v]", pad(&seen), look.join(",")));
        out.video = "[v]".into();
    } else {
        out.video = seen;
    }
    let mix = finish.mix.as_ref().filter(|m| !m.is_plain());
    if finish.music.is_none() && mix.is_none() {
        if !hear.is_empty() {
            steps.push(format!("{}{}[a]", pad(&film.audio), hear.join(",")));
            out.audio = "[a]".into();
        }
        out.filter = steps.join(";");
        return out;
    }
    // What is mixed, the shots' own sound first: the mix is as long as it is.
    let mut mixed = vec![pad(&film.audio)];
    if let Some(mix) = mix {
        if mix.shots_gain != 1.0 || mix.shots_pan != 0.0 || mix.ducks() {
            let under = if mix.ducks() { duck(&mix.duck, mix.duck_by) } else { String::new() };
            steps.push(format!("{}{SOUND},volume={:.3}{}{under}[sa]", pad(&film.audio), mix.shots_gain.max(0.0), balance(mix.shots_pan)));
            mixed[0] = "[sa]".into();
        }
    }
    if let Some((input, volume)) = finish.music {
        // The sound file runs under the shots' own sound, as long as the film.
        steps.push(format!("[{input}:a]{SOUND},volume={volume:.2}[m]"));
        mixed.push("[m]".into());
    }
    if let Some(mix) = mix {
        let heard: Vec<&Sound> = mix
            .sounds
            .iter()
            .filter(|s| s.gain > 0.0 && s.length > 0.0 && s.start < total && s.start + s.length * s.loops.max(1) as f64 > 0.0)
            .collect();
        // A file that several regions play is read once and handed to each of them.
        let mut inputs: Vec<usize> = heard.iter().map(|s| s.input).collect();
        inputs.sort_unstable();
        inputs.dedup();
        let uses: Vec<usize> = inputs.iter().map(|input| heard.iter().filter(|s| s.input == *input).count()).collect();
        for (input, uses) in inputs.iter().zip(&uses).filter(|(_, uses)| **uses > 1) {
            let outs: String = (0..*uses).map(|n| format!("[i{input}_{n}]")).collect();
            steps.push(format!("[{input}:a]asplit={uses}{outs}"));
        }
        let mut given = vec![0usize; inputs.len()];
        for (n, s) in heard.iter().enumerate() {
            let at = inputs.binary_search(&s.input).unwrap_or(0);
            let from = if uses[at] > 1 { format!("[i{}_{}]", s.input, given[at]) } else { format!("[{}:a]", s.input) };
            given[at] += 1;
            steps.push(format!("{from}{}[s{n}]", sound_chain(s)));
            mixed.push(format!("[s{n}]"));
        }
    }
    let mut last: Vec<String> = Vec::new();
    if mixed.len() > 1 {
        last.push(format!("amix=inputs={}:duration=first:dropout_transition=0:normalize=0", mixed.len()));
    }
    last.extend(hear);
    if let Some(mix) = mix {
        if mix.master != 1.0 {
            last.push(format!("volume={:.3}", mix.master.max(0.0)));
        }
        // Several sounds on top of each other can be louder than a file can hold: the peaks are held down.
        last.push("alimiter=limit=0.97:level=false:latency=true".into());
    }
    if !last.is_empty() {
        steps.push(format!("{}{}[a]", mixed.concat(), last.join(",")));
        out.audio = "[a]".into();
    }
    out.filter = steps.join(";");
    out
}

/// Turns the shots' sound down while something is said: over a quarter of a second at each end
/// of a stretch, so nothing clicks. `by` 0.6 leaves 0.4 of the sound.
fn duck(spans: &[(f64, f64)], by: f64) -> String {
    let ramp = super::words::DUCK_RAMP;
    let down: Vec<String> = spans.iter().map(|(a, b)| format!("clip(min((t-{a:.3})/{ramp},({b:.3}-t)/{ramp}),0,1)")).collect();
    format!(",volume=volume='1-{:.3}*min(1,{})':eval=frame", by.clamp(0.0, 1.0), down.join("+"))
}

/// Turns down the side a sound is panned away from. Nothing in the middle.
fn balance(pan: f64) -> String {
    let pan = pan.clamp(-1.0, 1.0);
    if pan == 0.0 {
        return String::new();
    }
    format!(",pan=stereo|c0={:.3}*c0|c1={:.3}*c1", (1.0 - pan).min(1.0), (1.0 + pan).min(1.0))
}

/// What is done to the file of one region: its stretch of the file, repeated, faded at its ends,
/// at its loudness and side, set at its place in the film.
fn sound_chain(s: &Sound) -> String {
    let loops = s.loops.max(1);
    let span = s.length * loops as f64;
    let from = s.offset.max(0.0);
    let mut chain = format!(
        "atrim=start={from:.4}:end={:.4},asetpts=PTS-STARTPTS,{SOUND},apad=whole_dur={:.4},atrim=end={:.4}",
        from + s.length,
        s.length,
        s.length
    );
    if loops > 1 {
        chain.push_str(&format!(",aloop=loop={}:size={},asetpts=N/SR/TB", loops - 1, (s.length * RATE).round() as u64));
    }
    let (fade_in, fade_out) = (s.fade_in.clamp(0.0, span), s.fade_out.clamp(0.0, span));
    if fade_in > 0.0 {
        chain.push_str(&format!(",afade=t=in:st=0:d={fade_in:.4}"));
    }
    if fade_out > 0.0 {
        chain.push_str(&format!(",afade=t=out:st={:.4}:d={fade_out:.4}", span - fade_out));
    }
    // A region that starts before the film loses what lies before it.
    if s.start < 0.0 {
        chain.push_str(&format!(",atrim=start={:.4},asetpts=PTS-STARTPTS", -s.start));
    }
    if s.gain != 1.0 {
        chain.push_str(&format!(",volume={:.3}", s.gain));
    }
    chain.push_str(&balance(s.pan));
    if s.start > 0.0 {
        chain.push_str(&format!(",adelay={}:all=1", (s.start * 1000.0).round() as u64));
    }
    chain
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Half a second: what a join lasts unless the shot says otherwise.
    const JOIN_FRAMES: u32 = 12;

    fn piece(frames: u32, join: Join) -> Piece {
        Piece { start: 0.0, frames, join, volume: 1.0, has_sound: true, join_frames: JOIN_FRAMES, fit: None }
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
        let all = Finish { fade_in: true, fade_out: true, scale: 2, music: Some((2, 0.4)), mix: None, captions: None };
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

    #[test]
    fn a_join_is_as_long_as_its_shot_says() {
        let mut slow = piece(96, Join::Dissolve);
        slow.join_frames = 48;
        let mut dark = piece(96, Join::Black);
        dark.join_frames = 24;
        let film = [piece(96, Join::Cut), slow, dark];
        let g = joined(&film);
        // Two seconds of dissolve, then half a second down to black and half a second up.
        assert!(g.filter.contains("xfade=transition=fade:duration=2.0000:offset=1.9980"));
        assert!(g.filter.contains("fade=t=out:st=3.5000:d=0.5000[v1]"));
        assert!(g.filter.contains("fade=t=in:st=0:d=0.5000[v2]"));
        assert_eq!(g.frames, 96 * 3 - 48);
        assert_eq!(film_frames(&film), g.frames);
    }

    fn sound(input: usize, start: f64, length: f64) -> Sound {
        Sound { input, offset: 0.0, length, loops: 1, start, gain: 1.0, pan: 0.0, fade_in: 0.0, fade_out: 0.0 }
    }

    fn mix(sounds: Vec<Sound>) -> Mix {
        Mix { shots_gain: 1.0, shots_pan: 0.0, sounds, master: 1.0, duck: Vec::new(), duck_by: 0.0 }
    }

    const REGION: &str = "asetpts=PTS-STARTPTS,aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo";
    const LIMIT: &str = "alimiter=limit=0.97:level=false:latency=true";

    #[test]
    fn no_tracks_leave_the_film_as_it_was() {
        // A plain mix adds nothing: a film of cuts still needs no graph at all.
        let plain = finished(as_it_is(96), &Finish { mix: Some(mix(vec![])), ..Finish::default() });
        assert_eq!((plain.filter.as_str(), plain.video.as_str(), plain.audio.as_str()), ("", "0:v", "0:a"));
        assert!(mix(vec![]).is_plain() && !mix(vec![sound(1, 0.0, 1.0)]).is_plain());
        // A region with no loudness, or one that lies wholly outside the film, is not read at all.
        let mut silent = sound(1, 0.0, 2.0);
        silent.gain = 0.0;
        let outside =
            finished(as_it_is(96), &Finish { mix: Some(mix(vec![silent, sound(2, 4.0, 2.0), sound(3, -3.0, 2.0)])), ..Finish::default() });
        assert_eq!(outside.filter, format!("[0:a]{LIMIT}[a]"));
        // The master alone turns the film's own sound up or down, held under the limit.
        let quiet = finished(as_it_is(96), &Finish { mix: Some(Mix { master: 0.5, ..mix(vec![]) }), ..Finish::default() });
        assert_eq!(quiet.filter, format!("[0:a]volume=0.500,{LIMIT}[a]"));
    }

    #[test]
    fn regions_are_set_at_their_place() {
        let mut second = sound(2, 1.5, 2.0);
        second.offset = 0.25;
        second.gain = 0.5;
        second.fade_in = 0.2;
        second.fade_out = 0.4;
        let g = finished(as_it_is(96), &Finish { mix: Some(mix(vec![sound(1, 0.0, 1.0), second])), ..Finish::default() });
        let steps: Vec<&str> = g.filter.split(';').collect();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0], format!("[1:a]atrim=start=0.0000:end=1.0000,{REGION},apad=whole_dur=1.0000,atrim=end=1.0000[s0]"));
        assert_eq!(
            steps[1],
            format!(
                "[2:a]atrim=start=0.2500:end=2.2500,{REGION},apad=whole_dur=2.0000,atrim=end=2.0000,\
                 afade=t=in:st=0:d=0.2000,afade=t=out:st=1.6000:d=0.4000,volume=0.500,adelay=1500:all=1[s1]"
            )
        );
        // Regions that overlap are simply both in the mix; the shots' sound is first and gives the length.
        assert_eq!(steps[2], format!("[0:a][s0][s1]amix=inputs=3:duration=first:dropout_transition=0:normalize=0,{LIMIT}[a]"));
        assert_eq!((g.video.as_str(), g.audio.as_str()), ("0:v", "[a]"));
    }

    #[test]
    fn a_region_that_starts_before_the_film_is_cut_at_its_start() {
        let mut early = sound(1, -0.75, 2.0);
        early.fade_in = 0.5;
        let g = finished(as_it_is(96), &Finish { mix: Some(mix(vec![early])), ..Finish::default() });
        // The fade belongs to the region's own start, so it is cut with it; nothing is delayed.
        assert!(g.filter.contains("afade=t=in:st=0:d=0.5000,atrim=start=0.7500,asetpts=PTS-STARTPTS[s0]"));
        assert!(!g.filter.contains("adelay"));
    }

    #[test]
    fn loops_pan_and_one_file_in_several_regions() {
        let mut looped = sound(3, 1.0, 0.5);
        looped.loops = 4;
        looped.fade_out = 0.5;
        looped.pan = -1.0;
        let mut right = sound(3, 0.0, 0.5);
        right.pan = 0.5;
        let film = joined(&[piece(48, Join::Cut), piece(48, Join::Dissolve)]);
        let shots = Mix { shots_gain: 0.8, shots_pan: 1.0, master: 1.2, sounds: vec![looped, right], duck: Vec::new(), duck_by: 0.0 };
        let g = finished(film, &Finish { fade_out: true, music: Some((2, 0.4)), mix: Some(shots), ..Finish::default() });
        let steps: Vec<&str> = g.filter.split(';').collect();
        let tail = &steps[steps.len() - 7..];
        assert_eq!(tail[0], "[xv1]fade=t=out:st=3.0000:d=0.5000[v]");
        assert_eq!(tail[1], format!("[xa1]{SOUND},volume=0.800,pan=stereo|c0=0.000*c0|c1=1.000*c1[sa]"));
        assert_eq!(tail[2], format!("[2:a]{SOUND},volume=0.40[m]"));
        // The file is read once and split for the two regions that play it.
        assert_eq!(tail[3], "[3:a]asplit=2[i3_0][i3_1]");
        assert_eq!(
            tail[4],
            format!(
                "[i3_0]atrim=start=0.0000:end=0.5000,{REGION},apad=whole_dur=0.5000,atrim=end=0.5000,aloop=loop=3:size=24000,asetpts=N/SR/TB,\
                 afade=t=out:st=1.5000:d=0.5000,pan=stereo|c0=1.000*c0|c1=0.000*c1,adelay=1000:all=1[s0]"
            )
        );
        assert!(tail[5].starts_with("[i3_1]") && tail[5].ends_with("atrim=end=0.5000,pan=stereo|c0=0.500*c0|c1=1.000*c1[s1]"));
        assert_eq!(
            tail[6],
            format!("[sa][m][s0][s1]amix=inputs=4:duration=first:dropout_transition=0:normalize=0,afade=t=out:st=3.0000:d=0.5000,volume=1.200,{LIMIT}[a]")
        );
        assert_eq!((g.video.as_str(), g.audio.as_str(), g.frames), ("[v]", "[a]", 84));
    }

    #[test]
    fn many_regions_on_many_tracks() {
        // Six tracks of twenty regions each, every track with its own file.
        let sounds: Vec<Sound> = (0..120).map(|n| sound(1 + n % 6, n as f64 * 0.5, 1.0)).collect();
        let g = finished(as_it_is(24 * 300), &Finish { mix: Some(mix(sounds)), ..Finish::default() });
        assert_eq!(g.filter.matches("asplit=20").count(), 6);
        assert_eq!(g.filter.matches("adelay=").count(), 119);
        assert!(g.filter.contains("amix=inputs=121:"));
        for n in 0..120 {
            assert_eq!(g.filter.matches(&format!("[s{n}]")).count(), 2);
        }
    }

    #[test]
    fn clips_of_two_sizes_are_brought_to_one() {
        let mut large = piece(48, Join::Cut);
        large.fit = Some((1280, 768));
        // A film that holds a clip of another size is never joined as a plain list.
        assert!(!plain(&[piece(48, Join::Cut), large.clone()]));
        let g = joined(&[large]);
        assert!(g
            .filter
            .contains("trim=end_frame=48,scale=1280:768:force_original_aspect_ratio=increase:flags=lanczos,crop=1280:768,format=yuv420p"));
    }

    #[test]
    fn the_shots_sound_goes_down_under_what_is_said() {
        let said = Mix { duck: vec![(0.75, 3.25), (4.75, 6.25)], duck_by: 0.6, ..mix(vec![sound(1, 1.0, 2.0)]) };
        assert!(!Mix { sounds: Vec::new(), ..said.clone() }.is_plain());
        let g = finished(as_it_is(240), &Finish { mix: Some(said.clone()), ..Finish::default() });
        let steps: Vec<&str> = g.filter.split(';').collect();
        assert_eq!(
            steps[0],
            format!("[0:a]{SOUND},volume=1.000,volume=volume='1-0.600*min(1,clip(min((t-0.750)/0.25,(3.250-t)/0.25),0,1)+clip(min((t-4.750)/0.25,(6.250-t)/0.25),0,1))':eval=frame[sa]")
        );
        assert!(steps[2].starts_with("[sa][s0]amix=inputs=2"));
        // No amount, or nothing said: the shots' sound is left alone.
        let none = finished(as_it_is(240), &Finish { mix: Some(Mix { duck_by: 0.0, ..said.clone() }), ..Finish::default() });
        assert!(!none.filter.contains("eval=frame"));
        let silent = finished(as_it_is(240), &Finish { mix: Some(Mix { duck: Vec::new(), ..said }), ..Finish::default() });
        assert!(!silent.filter.contains("eval=frame"));
    }

    #[test]
    fn subtitles_are_laid_over_the_finished_picture() {
        let plain = finished(as_it_is(96), &Finish { captions: Some((1, 292)), ..Finish::default() });
        assert_eq!(plain.filter, "[0:v][1:v]overlay=x=0:y=292:eof_action=pass:format=auto[sub]");
        assert_eq!((plain.video.as_str(), plain.audio.as_str()), ("[sub]", "0:a"));
        // Over the enlarged picture, and under the film's own fades.
        let all = finished(as_it_is(96), &Finish { captions: Some((3, 584)), scale: 2, fade_in: true, ..Finish::default() });
        let steps: Vec<&str> = all.filter.split(';').collect();
        assert_eq!(steps[0], "[0:v]scale=iw*2:ih*2:flags=lanczos[big]");
        assert_eq!(steps[1], "[big][3:v]overlay=x=0:y=584:eof_action=pass:format=auto[sub]");
        assert_eq!(steps[2], "[sub]fade=t=in:st=0:d=0.5000[v]");
        assert_eq!(all.video, "[v]");
    }
}
