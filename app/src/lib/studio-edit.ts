// What an edit does to a film, with no screen in it: the film as the workstation holds it, moving
// and trimming shots and regions, splitting, the undo history, and the arithmetic of the mixer.

import type { StudioExportOptions, StudioMember, StudioProject, StudioSave, StudioShot, StudioStrip } from './api.ts';
import { FPS, snapFrame } from './studio-time.ts';

/** A shot as the person edits it. What the service knows about its clip is kept apart. */
export interface DocShot {
	id: string;
	prompt: string;
	frames: number;
	continues: boolean;
	trim_start: number;
	trim_end: number;
	join: 'cut' | 'dissolve' | 'black';
	join_seconds: number;
	volume: number;
	mute: boolean;
	seed: number | null;
	/** What is said during the shot. */
	lines: DocLine[];
	/** The id of a cast member whose portrait the shot starts from; empty for none. */
	from: string;
}

/** One spoken line: by a cast member (`who` is their id) or, with none, the narrator. */
export interface DocLine {
	id: string;
	who: string;
	text: string;
	/** The same line in other languages, by the language's name. */
	said: Record<string, string>;
}

/** A character: a name, the fixed words for how they look, and the voice that says their lines. */
export interface DocMember {
	id: string;
	name: string;
	look: string;
	voice: number | null;
}

export interface DocRegion {
	id: string;
	file: string;
	start: number;
	offset: number;
	length: number;
	gain: number;
	fade_in: number;
	fade_out: number;
	loops: number;
}

export interface DocTrack {
	id: string;
	name: string;
	/** "voice" for the readings of the film's lines in one language (`lang`); "sound" otherwise. */
	kind: string;
	lang: string;
	color: string;
	volume: number;
	pan: number;
	mute: boolean;
	solo: boolean;
	regions: DocRegion[];
}

/** Everything about a film that an edit changes and an undo brings back. */
export interface Doc {
	name: string;
	size: string;
	auto_export: boolean;
	fade_in: boolean;
	fade_out: boolean;
	master: number;
	export: StudioExportOptions;
	shots: DocShot[];
	shot_track: StudioStrip;
	tracks: DocTrack[];
	/** One line every shot shares: style, colours, light, lens. */
	look: string;
	cast: DocMember[];
	/** A seed every shot without its own is made with. */
	seed: number | null;
	/** Whether the shot writer writes spoken lines, the language they are in, and the narrator's voice. */
	speech: boolean;
	language: string;
	voice: number | null;
	subtitles: { show: boolean; burn: boolean };
	/** How far the shots' own sound goes down while a line is said, 0 to 1. */
	duck: number;
	/** The language that is heard and read; empty for the film's own. */
	dub: string;
	/** The take each shot has in use, so an undo can put the one before back in use. Not saved from here. */
	takes: Record<string, number>;
}

/** A new id as the service takes them: twelve lowercase letters and digits. */
export function newId(random: () => number = Math.random) {
	let id = '';
	for (let i = 0; i < 12; i++) id += '0123456789abcdefghijklmnopqrstuvwxyz'[Math.floor(random() * 36)];
	return id;
}

export const docShot = (s: Partial<StudioShot> & { id: string }): DocShot => ({
	id: s.id,
	prompt: s.prompt ?? '',
	frames: s.frames ?? 121,
	continues: Boolean(s.continues),
	trim_start: s.trim_start ?? 0,
	trim_end: s.trim_end ?? 0,
	join: s.join === 'dissolve' || s.join === 'black' ? s.join : 'cut',
	join_seconds: s.join_seconds ?? 0.5,
	volume: s.volume ?? 1,
	mute: Boolean(s.mute),
	seed: s.seed ?? null,
	lines: (s.lines ?? []).map((l) => ({ id: l.id, who: l.who ?? '', text: l.text, said: { ...l.said } })),
	from: s.from ?? ''
});

export const docMember = (m: Partial<StudioMember> & { id: string }): DocMember => ({ id: m.id, name: m.name ?? '', look: m.look ?? '', voice: m.voice ?? null });

/** The film as the service sent it, as the workstation edits it. */
export function docOf(p: StudioProject): Doc {
	return {
		name: p.name,
		size: p.size,
		auto_export: Boolean(p.auto_export),
		fade_in: Boolean(p.fade_in),
		fade_out: Boolean(p.fade_out),
		master: p.master ?? 1,
		export: { ...p.export },
		shots: p.shots.map(docShot),
		shot_track: { volume: 1, pan: 0, mute: false, solo: false, ...p.shot_track },
		look: p.look ?? '',
		cast: (p.cast ?? []).map(docMember),
		seed: p.seed ?? null,
		speech: Boolean(p.speech),
		language: p.language ?? '',
		voice: p.voice ?? null,
		subtitles: { show: true, burn: false, ...p.subtitles },
		duck: p.duck ?? 0.6,
		dub: p.dub ?? '',
		takes: Object.fromEntries(p.shots.filter((s) => s.take).map((s) => [s.id, s.take as number])),
		tracks: (p.tracks ?? []).map((t) => ({
			id: t.id,
			name: t.name,
			kind: t.kind ?? 'sound',
			lang: t.lang ?? '',
			color: t.color,
			volume: t.volume ?? 1,
			pan: t.pan ?? 0,
			mute: Boolean(t.mute),
			solo: Boolean(t.solo),
			regions: t.regions.map((r) => ({
				id: r.id,
				file: r.file,
				start: r.start,
				offset: r.offset ?? 0,
				length: r.length,
				gain: r.gain ?? 1,
				fade_in: r.fade_in ?? 0,
				fade_out: r.fade_out ?? 0,
				loops: r.loops ?? 1
			}))
		}))
	};
}

/** What is sent to the service to save the film. */
export function saveBody(doc: Doc): StudioSave {
	return {
		name: doc.name,
		size: doc.size,
		auto_export: doc.auto_export,
		fade_in: doc.fade_in,
		fade_out: doc.fade_out,
		master: doc.master,
		export: doc.export,
		shot_track: doc.shot_track,
		shots: doc.shots.map((s) => ({ ...s, lines: s.lines.map((l) => ({ ...l, said: { ...l.said } })) })),
		look: doc.look,
		cast: doc.cast.map((m) => ({ ...m })),
		seed: doc.seed,
		speech: doc.speech,
		language: doc.language,
		voice: doc.voice,
		subtitles: { ...doc.subtitles },
		duck: doc.duck,
		dub: doc.dub,
		tracks: doc.tracks.map((t) => ({ ...t, regions: t.regions.map((r) => ({ ...r })) }))
	};
}

// --- shots: a magnetic lane --------------------------------------------------------------------

/**
 * The shots with the ones named in `ids` taken out and put back together, in the order they
 * had, in front of the shot that stood at `to` (the end of the film for `to` past the last).
 * The others close up: nothing in the lane leaves a gap.
 */
export function moveShots<T extends { id: string }>(shots: readonly T[], ids: readonly string[], to: number): T[] {
	const moved = shots.filter((s) => ids.includes(s.id));
	if (moved.length === 0) return [...shots];
	const before = shots.slice(0, Math.max(0, Math.min(to, shots.length))).filter((s) => !ids.includes(s.id)).length;
	const rest = shots.filter((s) => !ids.includes(s.id));
	return [...rest.slice(0, before), ...moved, ...rest.slice(before)];
}

/** Where a dragged shot would land: in front of the first part whose middle is after the pointer. */
export function dropPlace(parts: readonly { start: number; length: number }[], seconds: number): number {
	const at = parts.findIndex((p) => seconds < p.start + p.length / 2);
	return at < 0 ? parts.length : at;
}

/** The least of a clip a trim leaves, as the service holds it. */
export const LEAST_KEPT = 0.5;

/**
 * The trims of a shot after one of its edges is dragged by `delta` seconds (to the right is
 * more): whole frames, never less than nothing cut, never less than half a second kept.
 */
export function trimShot(shot: { trim_start: number; trim_end: number }, clip: number, edge: 'start' | 'end', delta: number): { trim_start: number; trim_end: number } {
	const room = Math.max(0, clip - LEAST_KEPT);
	if (edge === 'start') {
		const trim_start = Math.min(Math.max(0, snapFrame(shot.trim_start + delta)), Math.max(0, snapFrame(room - shot.trim_end)));
		return { trim_start, trim_end: shot.trim_end };
	}
	const trim_end = Math.min(Math.max(0, snapFrame(shot.trim_end - delta)), Math.max(0, snapFrame(room - shot.trim_start)));
	return { trim_start: shot.trim_start, trim_end };
}

/** A join out of where its handle is dragged to: how far from the cut, and in which half of the lane. */
export function joinFromDrag(seconds: number, upper: boolean): { join: DocShot['join']; join_seconds: number } {
	const far = Math.abs(seconds);
	if (far < 0.1) return { join: 'cut', join_seconds: 0.5 };
	return { join: upper ? 'dissolve' : 'black', join_seconds: Math.min(2, Math.max(0.25, snapFrame(far))) };
}

// --- regions: placed freely -------------------------------------------------------------------

/** The shortest a region is: two frames. */
export const LEAST_REGION = 2 / FPS;

export const regionSpan = (r: { length: number; loops: number }) => r.length * Math.max(1, r.loops);
export const regionEnd = (r: { start: number; length: number; loops: number }) => r.start + regionSpan(r);

/** A region moved to start at `start`: never before the film. */
export const moveRegion = (r: DocRegion, start: number): DocRegion => ({ ...r, start: Math.max(0, start) });

/** Holds a region's fades inside it: together they are never longer than the region. */
function withFades(r: DocRegion): DocRegion {
	const span = regionSpan(r);
	const fade_in = Math.min(Math.max(0, r.fade_in), span);
	return { ...r, fade_in, fade_out: Math.min(Math.max(0, r.fade_out), span - fade_in) };
}

/**
 * A region whose left edge is dragged to `start`: its end stays, and it starts later or earlier
 * in its file. It cannot start before its file does, or before the film. A repeated region is
 * left as it is: its left edge is the start of every repeat.
 */
export function trimRegionStart(r: DocRegion, start: number): DocRegion {
	if (r.loops > 1) return r;
	const end = r.start + r.length;
	const from = Math.min(Math.max(start, r.start - r.offset, 0), end - LEAST_REGION);
	return withFades({ ...r, start: from, offset: Math.max(0, r.offset + from - r.start), length: end - from });
}

/**
 * A region whose right edge is dragged to `end`: it plays longer or shorter, to the end of its
 * file at most (`file` seconds; unknown: no limit). A repeated region gets more or fewer repeats.
 */
export function trimRegionEnd(r: DocRegion, end: number, file = Infinity): DocRegion {
	if (r.loops > 1) return withFades({ ...r, loops: Math.max(1, Math.round((end - r.start) / r.length)) });
	const length = Math.min(Math.max(LEAST_REGION, end - r.start), Math.max(LEAST_REGION, file - r.offset));
	return withFades({ ...r, length });
}

/**
 * A region cut in two at `at` seconds of the film. The first part keeps the fade in, the last
 * the fade out. A repeated region is cut into its whole repeats and the one that is cut through.
 * `null` when the moment is not inside the region.
 */
export function splitRegion(r: DocRegion, at: number, id: () => string = newId): DocRegion[] | null {
	const span = regionSpan(r);
	const into = at - r.start;
	if (into < LEAST_REGION / 2 || into > span - LEAST_REGION / 2) return null;
	const whole = Math.floor(into / r.length + 1e-6);
	const rest = into - whole * r.length;
	const out: DocRegion[] = [];
	const piece = (start: number, offset: number, length: number, loops: number) => out.push({ ...r, id: id(), start, offset, length, loops, fade_in: 0, fade_out: 0 });
	if (whole > 0) piece(r.start, r.offset, r.length, whole);
	if (rest > 1e-4) {
		piece(r.start + whole * r.length, r.offset, rest, 1);
		piece(at, r.offset + rest, r.length - rest, 1);
		if (r.loops - whole - 1 > 0) piece(r.start + (whole + 1) * r.length, r.offset, r.length, r.loops - whole - 1);
	} else {
		piece(at, r.offset, r.length, r.loops - whole);
	}
	out[0].id = r.id;
	out[0] = withFades({ ...out[0], fade_in: r.fade_in });
	out[out.length - 1] = withFades({ ...out[out.length - 1], fade_out: r.fade_out });
	return out;
}

/** The ids of the regions of a track that lie over another region of it. */
export function overlapping(regions: readonly DocRegion[]): Set<string> {
	const sorted = [...regions].sort((a, b) => a.start - b.start);
	const out = new Set<string>();
	let reach = -Infinity;
	let holder = '';
	for (const r of sorted) {
		if (r.start < reach - 1e-6) {
			out.add(r.id);
			out.add(holder);
		}
		if (regionEnd(r) > reach) {
			reach = regionEnd(r);
			holder = r.id;
		}
	}
	return out;
}

/** Copies of regions set down so the earliest starts at `at`, with ids of their own. */
export function pasteRegions(regions: readonly DocRegion[], at: number, id: () => string = newId): DocRegion[] {
	if (regions.length === 0) return [];
	const first = Math.min(...regions.map((r) => r.start));
	return regions.map((r) => ({ ...r, id: id(), start: Math.max(0, r.start - first + at) }));
}

/** How loud a region is at a moment of itself, from its fades alone: 0 to 1. */
export function fadeLevel(r: { length: number; loops: number; fade_in: number; fade_out: number }, into: number) {
	const span = regionSpan(r);
	if (into < 0 || into > span) return 0;
	let level = 1;
	if (r.fade_in > 0) level = Math.min(level, into / r.fade_in);
	if (r.fade_out > 0) level = Math.min(level, (span - into) / r.fade_out);
	return Math.max(0, Math.min(1, level));
}

// --- undo ------------------------------------------------------------------------------------------

/**
 * What a film was before each edit, newest last. An edit that carries the same `key` as the
 * one before it, within a moment of it, belongs to that one (typing a description is one step).
 */
export class History<T> {
	#past: { state: T; label: string; key: string; at: number }[] = [];
	#future: { state: T; label: string }[] = [];
	limit: number;

	constructor(limit = 200) {
		this.limit = limit;
	}

	/** Notes the film as it was before an edit named `label`. */
	push(before: T, label: string, key = '', now = Date.now()) {
		const last = this.#past[this.#past.length - 1];
		this.#future = [];
		if (key && last && last.key === key && now - last.at < 1500) {
			last.at = now;
			return;
		}
		this.#past.push({ state: before, label, key, at: now });
		if (this.#past.length > this.limit) this.#past.splice(0, this.#past.length - this.limit);
	}

	/** The film before the last edit, given the film as it is now (kept for redo). `null` with nothing to undo. */
	undo(current: T): { state: T; label: string } | null {
		const last = this.#past.pop();
		if (!last) return null;
		this.#future.push({ state: current, label: last.label });
		return { state: last.state, label: last.label };
	}

	redo(current: T): { state: T; label: string } | null {
		const next = this.#future.pop();
		if (!next) return null;
		this.#past.push({ state: current, label: next.label, key: '', at: 0 });
		return next;
	}

	get undoLabel() {
		return this.#past[this.#past.length - 1]?.label ?? '';
	}

	get redoLabel() {
		return this.#future[this.#future.length - 1]?.label ?? '';
	}

	get steps() {
		return this.#past.length;
	}
}

// --- the mixer --------------------------------------------------------------------------------------

/** Decibels of a gain, and back. Silence is minus infinity. */
export const dbOf = (gain: number) => (gain <= 0 ? -Infinity : 20 * Math.log10(gain));
export const gainOf = (db: number) => (db === -Infinity ? 0 : Math.pow(10, db / 20));

/** "0.0", "+3.5", "-12.0", and "-∞" for silence. */
export function dbLabel(gain: number) {
	const db = dbOf(gain);
	if (db === -Infinity || db < -99) return '-∞';
	const text = Math.abs(db) < 0.05 ? '0.0' : db.toFixed(1);
	return db > 0.05 ? `+${text}` : text;
}

/** Where the marks of a fader stand: its travel from 0 (bottom) to 1 (top), and the decibels there. */
export const FADER_MARKS: [number, number][] = [
	[1, 6],
	[0.76, 0],
	[0.62, -6],
	[0.48, -12],
	[0.3, -24],
	[0.15, -40],
	[0.04, -60],
	[0, -Infinity]
];

/** The gain at a place of a fader's travel: even in decibels between its marks, silent at the bottom. */
export function faderToGain(at: number, top = 2) {
	const x = Math.min(1, Math.max(0, at));
	for (let i = 0; i < FADER_MARKS.length - 1; i++) {
		const [hi, hiDb] = FADER_MARKS[i];
		const [lo, loDb] = FADER_MARKS[i + 1];
		if (x >= lo) {
			if (loDb === -Infinity) return x <= 0 ? 0 : Math.min(top, gainOf(hiDb) * (x / hi));
			return Math.min(top, gainOf(loDb + ((x - lo) / (hi - lo)) * (hiDb - loDb)));
		}
	}
	return 0;
}

/** The place on a fader's travel for a gain. */
export function gainToFader(gain: number) {
	if (gain <= 0) return 0;
	const db = dbOf(gain);
	for (let i = 0; i < FADER_MARKS.length - 1; i++) {
		const [hi, hiDb] = FADER_MARKS[i];
		const [lo, loDb] = FADER_MARKS[i + 1];
		if (db >= loDb) {
			if (loDb === -Infinity) return Math.min(hi, (gain / gainOf(hiDb)) * hi);
			return Math.min(1, lo + ((db - loDb) / (hiDb - loDb)) * (hi - lo));
		}
	}
	return 0;
}

/** How much of a meter a level fills, 0 to 1, on the same travel as the faders. */
export const meterFill = (level: number) => gainToFader(Math.min(2, level));

/** How loud the left and the right side of a strip are at a pan from -1 to 1: the side away from it is turned down. */
export function balance(pan: number): [number, number] {
	const p = Math.min(1, Math.max(-1, pan));
	return [Math.min(1, 1 - p), Math.min(1, 1 + p)];
}

/** "C", "L32", "R100". */
export function panLabel(pan: number) {
	const n = Math.round(Math.abs(pan) * 100);
	return n === 0 ? 'C' : `${pan < 0 ? 'L' : 'R'}${n}`;
}

/**
 * Which strips are heard: a muted one never; when any strip is soloed, only the soloed ones.
 * The same rule as the export.
 */
export function heard(strips: readonly { id: string; mute: boolean; solo: boolean }[]): Record<string, boolean> {
	const soloed = strips.some((s) => s.solo);
	return Object.fromEntries(strips.map((s) => [s.id, !s.mute && (!soloed || s.solo)]));
}

/** The gain of a strip as it is mixed: its volume when it is heard, nothing when it is not. */
export const stripGain = (volume: number, on: boolean) => (on ? Math.min(2, Math.max(0, volume)) : 0);

// --- the cast and what is said ------------------------------------------------------------------

/** Whether `name` stands in `text` as a word of its own, whatever the case: "Mira" is in "mira's boat", not in "admiral". */
export function names(text: string, name: string) {
	const [t, n] = [text.toLowerCase(), name.trim().toLowerCase()];
	if (!n) return false;
	const letter = /[\p{L}\p{N}]/u;
	for (let at = t.indexOf(n); at >= 0; at = t.indexOf(n, at + 1)) {
		const before = at > 0 ? t[at - 1] : '';
		const after = t[at + n.length] ?? '';
		if (!letter.test(before) && !letter.test(after)) return true;
	}
	return false;
}

/**
 * What the video model is told for a shot, as the service puts it together: the description,
 * then the look of every character it names, word for word, then the look of the film.
 */
export function told(look: string, cast: readonly DocMember[], prompt: string) {
	const text = prompt.trim();
	const film = look.trim().replace(/\.+$/, '');
	const seen = cast.filter((m) => names(text, m.name) && m.look.trim());
	if (!text || (seen.length === 0 && !film)) return text;
	let out = /[.!?]$/.test(text) ? text : `${text}.`;
	for (const m of seen) out += ` ${m.name}: ${m.look.trim().replace(/\.+$/, '')}.`;
	if (film) out += ` ${film}.`;
	return out;
}

/** The subtitle on screen at a moment of the film, if any. */
export function cueAt<T extends { start: number; end: number }>(cues: readonly T[], seconds: number): T | undefined {
	return cues.find((c) => seconds >= c.start && seconds < c.end);
}

/** A subtitle broken into rows that fit `width`, as `measure` says how wide a text is. At most `rows` rows; the last takes what is left. */
export function captionRows(text: string, width: number, measure: (text: string) => number, rows = 3): string[] {
	const out: string[] = [];
	for (const given of text.split('\n')) {
		let row = '';
		for (const word of given.split(/\s+/).filter(Boolean)) {
			const longer = row ? `${row} ${word}` : word;
			if (row && measure(longer) > width && out.length < rows - 1) {
				out.push(row);
				row = word;
			} else row = longer;
		}
		if (row) out.push(row);
	}
	return out.length > rows ? [...out.slice(0, rows - 1), out.slice(rows - 1).join(' ')] : out;
}

/**
 * How loud the shots' own sound is under what is said, at a moment of the film: 1 away from any
 * line, `1 - by` while one is said, with a quarter of a second to go down and to come back. The
 * same arithmetic as the export.
 */
export function duckLevel(said: readonly [number, number][], by: number, seconds: number, ramp = 0.25) {
	let down = 0;
	for (const [from, to] of said) {
		if (to <= from) continue;
		down += Math.min(1, Math.max(0, Math.min((seconds - (from - ramp)) / ramp, (to + ramp - seconds) / ramp)));
	}
	return 1 - Math.min(1, Math.max(0, by)) * Math.min(1, down);
}

/**
 * What the service did to the film (a video brought in, a dub, clips listened to) put into the
 * film as it stands here, for when it was edited while the service worked: only what that work
 * makes is taken from the service's film, everything else stays as it was edited.
 */
export function takeWork(kind: 'import' | 'dub' | 'listen', mine: Doc, theirs: Doc): Doc {
	const out: Doc = JSON.parse(JSON.stringify(mine));
	if (kind === 'import') {
		// An empty film took the shape of the video.
		if (mine.shots.length === 0) [out.size, out.export] = [theirs.size, { ...theirs.export }];
		for (const s of theirs.shots) if (!out.shots.some((x) => x.id === s.id)) out.shots.push(JSON.parse(JSON.stringify(s)));
		out.takes = { ...theirs.takes, ...out.takes };
	} else if (kind === 'dub') {
		// The voice tracks are the service's; which one is heard follows the language it set.
		for (const t of theirs.tracks.filter((x) => x.kind === 'voice')) {
			const at = out.tracks.findIndex((x) => x.id === t.id);
			if (at >= 0) out.tracks[at] = JSON.parse(JSON.stringify(t));
			else out.tracks.push(JSON.parse(JSON.stringify(t)));
		}
		out.dub = theirs.dub;
		for (const s of out.shots) {
			const there = theirs.shots.find((x) => x.id === s.id);
			for (const l of s.lines) l.said = { ...there?.lines.find((x) => x.id === l.id)?.said, ...l.said };
		}
	} else {
		for (const s of out.shots) {
			const there = theirs.shots.find((x) => x.id === s.id);
			if (s.lines.length === 0 && there) s.lines = JSON.parse(JSON.stringify(there.lines));
		}
		out.language ||= theirs.language;
	}
	return out;
}

// --- the assistant's work -----------------------------------------------------------------------

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
const copy = <T>(x: T): T => JSON.parse(JSON.stringify(x));

/** Things with an id, put together: theirs in their order, with what was changed, added or taken out here kept. */
function mergeById<T extends { id: string }>(base: readonly T[], mine: readonly T[], theirs: readonly T[]): T[] {
	const find = (list: readonly T[], id: string) => list.find((x) => x.id === id);
	const out: T[] = [];
	for (const t of theirs) {
		const [m, b] = [find(mine, t.id), find(base, t.id)];
		// Taken out here.
		if (b && !m) continue;
		out.push(copy(m && b && !same(m, b) ? m : t));
	}
	// Added here: after the one it follows here, or first.
	mine.forEach((m, i) => {
		if (find(base, m.id) || find(theirs, m.id)) return;
		const before = mine
			.slice(0, i)
			.reverse()
			.find((x) => out.some((o) => o.id === x.id));
		out.splice(before ? out.findIndex((o) => o.id === before.id) + 1 : 0, 0, copy(m));
	});
	return out;
}

/**
 * The film as the service has it (`theirs`) with what was edited here since `base` (`mine`) kept:
 * a field edited here stays as it is here, every other one comes from the service. Shots, the
 * cast and the tracks are put together one by one, by id.
 */
export function mergeDocs(base: Doc, mine: Doc, theirs: Doc): Doc {
	const out = copy(theirs);
	for (const key of Object.keys(theirs) as (keyof Doc)[]) {
		if (key === 'shots' || key === 'cast' || key === 'tracks') continue;
		if (!same(mine[key], base[key])) (out as unknown as Record<string, unknown>)[key] = copy(mine[key]);
	}
	out.shots = mergeById(base.shots, mine.shots, theirs.shots);
	out.cast = mergeById(base.cast, mine.cast, theirs.cast);
	out.tracks = mergeById(base.tracks, mine.tracks, theirs.tracks);
	return out;
}

/** What one request of the assistant is called in the undo history. */
export const ASSISTANT_STEP = 'What the assistant did';

/**
 * One request of the assistant on a film. The film before it is what one undo brings back,
 * however many steps the assistant took; `base` is the film as the service had it last.
 */
export class AssistTurn {
	readonly film: string;
	readonly before: Doc;
	base: Doc;

	constructor(film: string, doc: Doc) {
		this.film = film;
		this.before = copy(doc);
		this.base = copy(doc);
	}

	/**
	 * The film as the service has it now: what the workstation shows next. What was edited
	 * here meanwhile (`edited`) is kept. `null` when the service has nothing new.
	 */
	follow(mine: Doc, theirs: Doc, edited: boolean): Doc | null {
		if (same(theirs, this.base)) return null;
		const next = edited ? mergeDocs(this.base, mine, theirs) : copy(theirs);
		this.base = copy(theirs);
		return next;
	}

	/** The request ended with the film as `now`: one step in the history, when anything changed. */
	end(history: History<Doc>, now: Doc): boolean {
		if (same(this.before, now)) return false;
		history.push(copy(this.before), ASSISTANT_STEP);
		return true;
	}
}
