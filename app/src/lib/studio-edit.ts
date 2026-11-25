// What an edit does to a film, with no screen in it: the film as the workstation holds it, moving
// and trimming shots and regions, splitting, the undo history, and the arithmetic of the mixer.

import type { StudioExportOptions, StudioProject, StudioSave, StudioShot, StudioStrip } from './api.ts';
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
	seed: s.seed ?? null
});

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
		takes: Object.fromEntries(p.shots.filter((s) => s.take).map((s) => [s.id, s.take as number])),
		tracks: (p.tracks ?? []).map((t) => ({
			id: t.id,
			name: t.name,
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
		shots: doc.shots.map((s) => ({ ...s })),
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
