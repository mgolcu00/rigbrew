//! How a film is joined: the filter graph ffmpeg is given for dissolves, fades through black,
//! per-shot volume, the film's own fades, the sound file under it and the sound tracks mixed
//! into it. Nothing here runs anything.
//!
//! Everything is counted in frames at 24 a second, so the pictures and the sound of every shot
//! are exactly as long as each other and a long film does not drift.
//!
//! The sound is finished as well as it is joined (measured on a 24-shot film of the video model,
//! whose shots came out between -56 and -10 LUFS and clicked at 13 of 23 cuts):
//! - every sound is brought to 48 kHz with a long resampling filter, flat to 15 kHz;
//! - every shot is brought toward one loudness, at most 12 dB either way ([`level`]);
//! - a hard cut fades the sound out and in over 6 ms on each side, so the step between two
//!   clips is not heard as a click; the picture is not touched and nothing drifts;
//! - the whole film is measured once it is mixed and brought to -16 LUFS, the loudness of
//!   films and podcasts played on phones and laptops, under a limiter that keeps the true peak
//!   below -1.5 dBTP once it is AAC ([`MEASURE`], [`master`]).

const FPS: f64 = 24.0;
/// Samples a second of everything that is mixed.
const RATE: f64 = 48000.0;
/// The film's own fade in and fade out.
const EDGE_SECONDS: f64 = 0.5;
/// To 48 kHz with a filter four times the usual length: a 15 kHz tone loses 0.0 dB instead of 1.3.
const SOUND: &str = "aresample=48000:filter_size=128:kaiser_beta=12,aformat=sample_fmts=fltp:channel_layouts=stereo";
/// Seconds of the fade on each side of a hard cut.
const CUT_FADE: f64 = 0.006;
/// The loudness every shot is brought toward, and how far at most.
pub const SHOT_LUFS: f64 = -24.0;
const SHOT_REACH_DB: f64 = 12.0;
/// The loudness of the finished film.
pub const FILM_LUFS: f64 = -16.0;
/// The limiter's ceiling on samples: lower than the -1.5 dBTP the film may peak at, because the
/// peaks between samples and the AAC encoder add about 0.5 dB (measured: -2 dBFS gave -1.4 dBTP).
const LIMIT_DBFS: f64 = -2.5;
/// The first pass over the mixed film: what its loudness is.
pub const MEASURE: &str = "loudnorm=I=-16:TP=-1.5:LRA=11:print_format=json";

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
    /// Decibels that bring the clip's sound toward the loudness of every other shot ([`level`]).
    pub level: f32,
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
    /// Nothing to mix: the film's sound goes out as it is (the master is set at the end, see [`master`]).
    pub fn is_plain(&self) -> bool {
        self.sounds.is_empty() && self.shots_gain == 1.0 && self.shots_pan == 0.0 && !self.ducks()
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

/// Decibels that bring a shot whose sound measured `lufs` toward [`SHOT_LUFS`], at most 12 dB
/// either way; nothing for a shot that was not measured or is all but silent.
pub fn level(lufs: Option<f32>) -> f32 {
    match lufs {
        Some(l) if l.is_finite() && l > -70.0 => (SHOT_LUFS - l as f64).clamp(-SHOT_REACH_DB, SHOT_REACH_DB) as f32,
        _ => 0.0,
    }
}

/// The integrated loudness in ffmpeg's `ebur128` summary, in LUFS: none for silence.
pub fn loudness(log: &str) -> Option<f32> {
    let summary = &log[log.rfind("Summary:")?..];
    let value = summary.split("I:").nth(1)?.split("LUFS").next()?.trim().parse::<f32>().ok()?;
    (value.is_finite() && value > -70.0).then_some(value)
}

/// The integrated loudness of the mixed film as the first pass ([`MEASURE`]) wrote it in its log.
pub fn measured(log: &str) -> Option<f64> {
    let json = log.get(log.rfind('{')?..=log.rfind('}')?)?;
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v["input_i"].as_str()?.trim().parse::<f64>().ok().filter(|i| i.is_finite() && *i > -70.0)
}

/// What is done last to the sound of the mixed film: it is brought from the loudness it measured
/// to [`FILM_LUFS`] (moved by the master: 2 is 6 dB louder) in one step, and a limiter holds the
/// peaks. One gain for the whole film keeps its shots where the mix put them; a film that could
/// not be measured keeps its loudness.
pub fn master(measured: Option<f64>, fader: f64) -> String {
    if fader <= 0.0 {
        return "volume=0".into();
    }
    let offset = 20.0 * fader.min(2.0).log10();
    let gain = measured.map_or(offset, |i| (FILM_LUFS + offset - i).clamp(-30.0, 30.0));
    format!("volume={gain:.2}dB,alimiter=limit={:.3}:attack=5:release=50:level=false:latency=true", 10f64.powf(LIMIT_DBFS / 20.0))
}

/// Frames of the whole film: every piece, less what dissolves share.
#[cfg(test)]
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
        let gain = p.volume as f64 * 10f64.powf(p.level as f64 / 20.0);
        let mut a = if heard {
            format!("[{i}:a]atrim=start={:.3},asetpts=PTS-STARTPTS,{SOUND},volume={gain:.2}", p.start)
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
        // A hard cut on either side: the sound fades out and in over a few milliseconds there, so
        // the step from one clip's sound to the next is not a click. The film's start and end count as cuts.
        let mut edges = String::new();
        if heard {
            let next_cut = pieces.get(i + 1).is_none_or(|next| next.join == Join::Cut);
            if join_of(pieces, i) == Join::Cut {
                edges.push_str(&format!(",afade=t=in:st=0:d={CUT_FADE}:curve=qsin"));
            }
            if next_cut {
                edges.push_str(&format!(",afade=t=out:st={:.4}:d={CUT_FADE}:curve=qsin", length - CUT_FADE));
            }
        }
        steps.push(format!("{v}[v{i}]"));
        steps.push(format!("{a},apad=whole_dur={length:.4},atrim=end={length:.4}{edges}[a{i}]"));
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
#[cfg(test)]
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
    // Several sounds on top of each other can be louder than a file can hold: the mix is kept in
    // floating point, and its loudness, the master and the peaks are set once, at the end ([`master`]).
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
        Piece { start: 0.0, frames, join, volume: 1.0, level: 0.0, has_sound: true, join_frames: JOIN_FRAMES, fit: None }
    }

    const PICTURE: &str = "setpts=PTS-STARTPTS,fps=24,tpad=stop=-1:stop_mode=clone";
    const LOOK: &str = "format=yuv420p,setsar=1,settb=AVTB";

    #[test]
    fn the_sound_fades_for_a_few_milliseconds_at_every_cut() {
        let film = vec![piece(48, Join::Cut), piece(48, Join::Cut), piece(120, Join::Dissolve), piece(48, Join::Black)];
        let g = joined(&film);
        let steps: Vec<&str> = g.filter.split(';').collect();
        let (cut_in, cut_out) = (",afade=t=in:st=0:d=0.006:curve=qsin", ",afade=t=out:st=1.9940:d=0.006:curve=qsin");
        // The film's start and the cut after the first shot.
        assert!(steps[1].ends_with(&format!("atrim=end=2.0000{cut_in}{cut_out}[a0]")), "{}", steps[1]);
        // A cut in, a dissolve out: the dissolve's own crossfade is the join there.
        assert!(steps[3].ends_with(&format!("atrim=end=2.0000{cut_in}[a1]")), "{}", steps[3]);
        // Dissolved in, and gone through black: the fades of those joins, nothing more.
        assert!(steps[5].ends_with("atrim=end=5.0000[a2]") && steps[5].contains("afade=t=out:st=4.7500:d=0.2500"), "{}", steps[5]);
        // Up from black, and the end of the film.
        assert!(steps[7].ends_with(",atrim=end=2.0000,afade=t=out:st=1.9940:d=0.006:curve=qsin[a3]"), "{}", steps[7]);
        assert_eq!(film_frames(&film), g.frames);
        // A muted shot is silence: nothing to fade.
        let mut quiet = film.clone();
        quiet[1].volume = 0.0;
        assert!(joined(&quiet)
            .filter
            .split(';')
            .nth(3)
            .unwrap()
            .starts_with("anullsrc=r=48000:cl=stereo,apad=whole_dur=2.0000,atrim=end=2.0000[a1]"));
        // Nothing to do to the whole film: the joined clips go to the file as they are.
        let whole = finished(as_it_is(216), &Finish::default());
        assert_eq!((whole.filter.as_str(), whole.video.as_str(), whole.audio.as_str()), ("", "0:v", "0:a"));
    }

    #[test]
    fn one_shot() {
        let mut only = piece(56, Join::Dissolve);
        only.start = 0.5;
        only.volume = 1.5;
        let g = joined(&[only.clone()]);
        assert_eq!(
            g.filter,
            format!(
                "[0:v]trim=start=0.500,{PICTURE},trim=end_frame=56,{LOOK}[v0];\
                 [0:a]atrim=start=0.500,asetpts=PTS-STARTPTS,{SOUND},volume=1.50,apad=whole_dur=2.3333,atrim=end=2.3333,\
                 afade=t=in:st=0:d=0.006:curve=qsin,afade=t=out:st=2.3273:d=0.006:curve=qsin[a0]"
            )
        );
        assert_eq!((g.video.as_str(), g.audio.as_str(), g.frames), ("[v0]", "[a0]", 56));
        // A shot brought up 6 dB toward the others: its volume times two.
        only.level = 6.0206;
        assert!(joined(&[only]).filter.contains(",volume=3.00,apad="));
    }

    #[test]
    fn every_shot_is_brought_toward_one_loudness_but_not_too_far() {
        assert_eq!(level(Some(-24.0)), 0.0);
        assert_eq!(level(Some(-30.5)), 6.5);
        assert_eq!(level(Some(-10.5)), -12.0);
        assert_eq!(level(Some(-56.1)), 12.0);
        // Silence, and a clip that was not measured, are left as they are.
        assert_eq!((level(Some(-75.0)), level(Some(f32::NEG_INFINITY)), level(None)), (0.0, 0.0, 0.0));
    }

    #[test]
    fn the_loudness_is_read_from_ffmpeg() {
        let ebur128 = "[Parsed_ebur128_0 @ 0x1] Summary:\n\n  Integrated loudness:\n    I:         -36.7 LUFS\n    Threshold: -47.0 LUFS\n\n  Loudness range:\n    LRA:         3.1 LU\n";
        assert_eq!(loudness(ebur128), Some(-36.7));
        assert_eq!(loudness("[Parsed_ebur128_0 @ 0x1] Summary:\n    I:         -70.0 LUFS\n"), None);
        assert_eq!(loudness("no summary here"), None);
        let first_pass =
            "[Parsed_loudnorm_0 @ 0x2] \n{\n\t\"input_i\" : \"-24.66\",\n\t\"input_tp\" : \"3.93\",\n\t\"input_lra\" : \"12.70\",\n\
                          \t\"input_thresh\" : \"-35.17\",\n\t\"target_offset\" : \"0.12\"\n}\n[out#0/null @ 0x3] video:0KiB audio:...";
        assert_eq!(measured(first_pass), Some(-24.66));
        assert_eq!(measured("{\"input_i\" : \"-inf\"}"), None);
        assert_eq!(measured("nothing"), None);
    }

    #[test]
    fn the_film_is_brought_to_its_loudness_under_a_limiter() {
        let limit = "alimiter=limit=0.750:attack=5:release=50:level=false:latency=true";
        // 8.66 dB up from where it measured, to -16 LUFS.
        assert_eq!(master(Some(-24.66), 1.0), format!("volume=8.66dB,{limit}"));
        // The master moves the film's loudness: twice is 6 dB louder, a half 6 dB quieter.
        assert_eq!(master(Some(-24.66), 2.0), format!("volume=14.68dB,{limit}"));
        assert_eq!(master(Some(-14.2), 0.5), format!("volume=-7.82dB,{limit}"));
        // A film that could not be measured keeps its loudness; a master at nothing is silence.
        assert_eq!(master(None, 1.0), format!("volume=0.00dB,{limit}"));
        assert_eq!(master(Some(-20.0), 0.0), "volume=0");
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
        assert_eq!(g.filter.matches("fade=t=in:st=0:d=0.2500").count(), 2 * (1..60).filter(|i| i % 5 == 0 && i % 3 != 0).count());
        // The sound fades in after every cut and out before every one, and at the film's two ends.
        let cut = |i: usize| i == 0 || (!i.is_multiple_of(3) && !i.is_multiple_of(5));
        let (ins, outs) = ((0..60).filter(|i| cut(*i)).count(), (0..60).filter(|i| *i == 59 || cut(i + 1)).count());
        assert_eq!(g.filter.matches("afade=t=in:st=0:d=0.006").count(), ins);
        assert_eq!(g.filter.matches(":d=0.006:curve=qsin").count(), ins + outs);
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

    const REGION: &str =
        "asetpts=PTS-STARTPTS,aresample=48000:filter_size=128:kaiser_beta=12,aformat=sample_fmts=fltp:channel_layouts=stereo";

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
        assert_eq!((outside.filter.as_str(), outside.audio.as_str()), ("", "0:a"));
        // The master is set with the film's loudness, at the end (see `master`).
        let quiet = finished(as_it_is(96), &Finish { mix: Some(Mix { master: 0.5, ..mix(vec![]) }), ..Finish::default() });
        assert_eq!(quiet.filter, "");
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
        assert_eq!(steps[2], "[0:a][s0][s1]amix=inputs=3:duration=first:dropout_transition=0:normalize=0[a]");
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
            "[sa][m][s0][s1]amix=inputs=4:duration=first:dropout_transition=0:normalize=0,afade=t=out:st=3.0000:d=0.5000[a]"
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
