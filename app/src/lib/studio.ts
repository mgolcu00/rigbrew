// Studio arithmetic that has no screen in it: how long a shot is, what a move does to the film,
// which part of which clip is on screen at a moment of the film.

import type { StudioShot } from './api.ts';

/** A shot with a clip: made, or made and then changed. */
export const hasClip = (shot: StudioShot) => shot.state === 'done' || shot.state === 'changed';

/** Seconds of a shot: of its clip when it has one (the model rounds lengths), else what was asked for. */
export const shotSeconds = (shot: StudioShot) => ((hasClip(shot) && shot.made_frames) || shot.frames) / 24;

/** "0:07", "2:30". */
export function stamp(seconds: number) {
	const s = Math.max(0, Math.round(seconds));
	return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

/** The list with the item at `from` put at `to`. */
export function reorder<T>(list: readonly T[], from: number, to: number): T[] {
	const out = [...list];
	if (from < 0 || from >= out.length) return out;
	const [item] = out.splice(from, 1);
	out.splice(Math.max(0, Math.min(to, out.length)), 0, item);
	return out;
}

/**
 * The made shots a move sends back to be made again, as places in the new order. A shot that
 * continues another started from that shot's last picture: behind a different shot, or in the
 * first place, that picture is no longer the one before it.
 */
export function remadeByMove(shots: readonly StudioShot[], from: number, to: number): number[] {
	const next = reorder(shots, from, to);
	const out: number[] = [];
	next.forEach((shot, at) => {
		if (shot.state !== 'done' || !shot.continues) return;
		const was = shots.indexOf(shot);
		if (at === 0 || shots[was - 1] !== next[at - 1]) out.push(at);
	});
	return out;
}

/** Seconds a dissolve lasts, and a fade through black from the last picture to the next first one. */
export const JOIN_SECONDS = 0.5;

/** One clip in the film as it plays and as it is exported. */
export interface FilmPart {
	shot: string;
	/** Place of the shot in the film's list. */
	index: number;
	/** Seconds into the clip where it starts and stops (its trims). */
	from: number;
	to: number;
	/** Seconds into the film where it starts. A dissolve starts before the part before it has ended. */
	start: number;
	length: number;
	/** Seconds this part and the one before it are both on screen. */
	overlap: number;
	/** Seconds it comes up from black at its start, and goes down to black at its end. */
	fadeIn: number;
	fadeOut: number;
	/** 0 to 1.5; 0 when muted. */
	volume: number;
}

/**
 * The film out of its made shots: which part of each clip plays and when. The same arithmetic
 * as the export, so the player shows what the file will hold.
 */
export function filmPlan(shots: readonly StudioShot[]): { parts: FilmPart[]; total: number } {
	const parts: FilmPart[] = [];
	shots.forEach((shot, index) => {
		if (!hasClip(shot) || !shot.id) return;
		const whole = shotSeconds(shot);
		const from = Math.min(Math.max(0, shot.trim_start ?? 0), Math.max(0, whole - 0.1));
		const to = Math.max(from + 0.1, whole - Math.max(0, shot.trim_end ?? 0));
		const length = to - from;
		const before = parts[parts.length - 1];
		// A shot that continues the one before it joins it with nothing in between.
		const join = !before || shot.continues ? 'cut' : (shot.join ?? 'cut');
		const overlap = before && join === 'dissolve' ? Math.min(JOIN_SECONDS, before.length / 2, length / 2) : 0;
		const fadeIn = before && join === 'black' ? Math.min(JOIN_SECONDS / 2, length / 2) : 0;
		if (before && join === 'black') before.fadeOut = Math.min(JOIN_SECONDS / 2, before.length / 2);
		const volume = shot.mute ? 0 : Math.min(1.5, Math.max(0, shot.volume ?? 1));
		parts.push({ shot: shot.id, index, from, to, start: before ? before.start + before.length - overlap : 0, length, overlap, fadeIn, fadeOut: 0, volume });
	});
	const last = parts[parts.length - 1];
	return { parts, total: last ? last.start + last.length : 0 };
}

/** The part on screen at a moment of the film (the later one during a dissolve), and the seconds into its clip. */
export function partAt(parts: readonly FilmPart[], seconds: number): { part: number; at: number } | null {
	if (parts.length === 0) return null;
	let part = 0;
	for (let i = 0; i < parts.length; i++) if (parts[i].start <= seconds) part = i;
	const p = parts[part];
	return { part, at: Math.min(p.to, Math.max(p.from, p.from + seconds - p.start)) };
}
