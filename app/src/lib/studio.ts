// Studio arithmetic that has no screen in it: how long a shot is, what a move does to the film,
// which part of which clip is on screen at a moment of the film.

import type { StudioQuality, StudioShot } from './api.ts';

export type Shape = 'landscape' | 'portrait' | 'square';

/** The shape of a "WxH" size. */
export function shapeOf(size: string): Shape {
	const [w, h] = size.split('x').map(Number);
	return w > h ? 'landscape' : w < h ? 'portrait' : 'square';
}

/** The quality a "WxH" size is one of. */
export const qualityOf = (qualities: readonly StudioQuality[] | undefined, size: string) =>
	(qualities ?? []).find((q) => Object.values(q.sizes).some((s) => s && `${s[0]}x${s[1]}` === size));

/** The "WxH" of a quality for a shape. */
export function sizeAt(quality: StudioQuality | undefined, shape: Shape): string | undefined {
	const s = quality?.sizes[shape];
	return s ? `${s[0]}x${s[1]}` : undefined;
}

/**
 * Decibels that bring a shot whose sound measured `lufs` toward the loudness of the others, as
 * the export does it: toward -24 LUFS, at most 12 dB either way, nothing for silence.
 */
export function shotLevel(lufs: number | null | undefined): number {
	if (lufs == null || !Number.isFinite(lufs) || lufs <= -70) return 0;
	return Math.min(12, Math.max(-12, -24 - lufs));
}

/** A shot with a clip: made, made and then changed, or being made again while the clip before is still there. */
export const hasClip = (shot: StudioShot) =>
	shot.state === 'done' || shot.state === 'changed' || ((shot.state === 'making' || shot.state === 'queued') && Boolean(shot.made_frames));

/** Seconds of a shot: of its clip when it has one (the model rounds lengths), else what was asked for. */
export const shotSeconds = (shot: StudioShot) => ((hasClip(shot) && shot.made_frames) || shot.frames) / 24;

/** "0:07", "2:30". */
export function stamp(seconds: number) {
	const s = Math.max(0, Math.round(seconds));
	return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

/** A length of time in plain words: "about a minute", "about 25 minutes", "about 2 hours". */
export function roughly(seconds: number) {
	if (seconds < 45) return 'less than a minute';
	if (seconds < 90) return 'about a minute';
	const minutes = Math.round(seconds / 60);
	if (minutes < 90) return `about ${minutes} minutes`;
	const hours = Math.round(seconds / 1800) / 2;
	return `about ${hours} ${hours === 1 ? 'hour' : 'hours'}`;
}

/**
 * Seconds a film of `shots` shots of `frames` frames takes to make, before its shots are written.
 * Scenes are about three shots: the first is plain, the others continue it when the model can.
 */
export function filmWork(shots: number, frames: number, pace: { frames: number; plain: number; continuing: number }, canContinue: boolean) {
	const scale = frames / pace.frames;
	const plain = canContinue ? Math.ceil(shots / 3) : shots;
	return (plain * pace.plain + (shots - plain) * pace.continuing) * scale;
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

/** Seconds a dissolve lasts, and a fade through black from the last picture to the next first one, unless the shot says otherwise. */
export const JOIN_SECONDS = 0.5;
export const FPS = 24;

/** One clip in the film as it plays and as it is exported. */
export interface FilmPart {
	shot: string;
	/** Place of the shot in the film's list. */
	index: number;
	/** Whether the shot has a clip. A shot without one is only in a plan of every shot. */
	made: boolean;
	/** Seconds into the clip where it starts and stops (its trims). */
	from: number;
	to: number;
	/** Seconds into the film where it starts. A dissolve starts before the part before it has ended. */
	start: number;
	length: number;
	/** How it joins the part before it, and the seconds that join was asked to last. */
	join: 'cut' | 'dissolve' | 'black';
	joinSeconds: number;
	/** Seconds this part and the one before it are both on screen. */
	overlap: number;
	/** Seconds it comes up from black at its start, and goes down to black at its end. */
	fadeIn: number;
	fadeOut: number;
	/** The shot's volume (0 to 1.5; 0 when muted) times what brings its sound toward the others' loudness. */
	volume: number;
}

/** The seconds a join into `shot` lasts, as the service holds them: a quarter of a second to two. */
export const joinSeconds = (shot: StudioShot) => Math.min(2, Math.max(0.25, shot.join_seconds ?? JOIN_SECONDS));

/**
 * The film out of its made shots: which part of each clip plays and when. The same arithmetic
 * as the export, counted in frames, so the player shows what the file will hold. With `all`,
 * the shots that have no clip yet are in it too, at the length that was asked for: the film as
 * it stands on the timeline.
 */
export function filmPlan(shots: readonly StudioShot[], all = false): { parts: FilmPart[]; total: number } {
	const parts: FilmPart[] = [];
	shots.forEach((shot, index) => {
		const made = hasClip(shot) && Boolean(shot.id);
		if (!made && !all) return;
		const whole = shotSeconds(shot);
		const from = made ? Math.min(Math.max(0, shot.trim_start ?? 0), Math.max(0, whole - 0.1)) : 0;
		const to = made ? Math.max(from + 0.1, whole - Math.max(0, shot.trim_end ?? 0)) : whole;
		const frames = Math.max(1, Math.round((to - from) * FPS));
		const length = frames / FPS;
		const before = parts[parts.length - 1];
		const beforeFrames = before ? Math.round(before.length * FPS) : 0;
		// A shot that continues the one before it joins it with nothing in between.
		const join = !before || shot.continues ? 'cut' : shot.join || 'cut';
		const asked = Math.round(joinSeconds(shot) * FPS);
		const overlap = join === 'dissolve' ? Math.min(asked, Math.floor(beforeFrames / 2), Math.floor(frames / 2)) / FPS : 0;
		const dip = Math.floor(asked / 2);
		const fadeIn = join === 'black' ? Math.min(dip, Math.floor(frames / 2)) / FPS : 0;
		if (before && join === 'black') before.fadeOut = Math.min(dip, Math.floor(beforeFrames / 2)) / FPS;
		const lufs = shot.takes?.find((t) => t.n === shot.take)?.lufs;
		const volume = shot.mute ? 0 : Math.min(1.5, Math.max(0, shot.volume ?? 1)) * 10 ** (shotLevel(lufs) / 20);
		parts.push({
			shot: shot.id,
			index,
			made,
			from,
			to: from + length,
			start: before ? before.start + before.length - overlap : 0,
			length,
			join,
			joinSeconds: joinSeconds(shot),
			overlap,
			fadeIn,
			fadeOut: 0,
			volume
		});
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

/**
 * How much of a part is seen at a moment of the film, 0 to 1: its fades through black and the
 * film's own fade at its start and end (`edge` seconds each, 0 for none).
 */
export function partLight(p: FilmPart, film: number, total: number, edgeIn = 0, edgeOut = 0) {
	const at = film - p.start;
	let level = 1;
	if (p.fadeIn > 0) level = Math.min(level, at / p.fadeIn);
	if (p.fadeOut > 0) level = Math.min(level, (p.length - at) / p.fadeOut);
	if (edgeIn > 0) level = Math.min(level, film / edgeIn);
	if (edgeOut > 0) level = Math.min(level, (total - film) / edgeOut);
	return Math.max(0, Math.min(1, level));
}
