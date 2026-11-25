// Studio arithmetic that has no screen in it: how long a shot is, what a move does to the film.

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
