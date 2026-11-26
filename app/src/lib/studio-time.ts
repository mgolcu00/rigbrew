// Studio arithmetic of time on a screen: timecode, the mapping between seconds and pixels at a
// zoom, the marks on the ruler, and what a dragged edge snaps to. No screen in it.

export const FPS = 24;

/** The frame a moment falls on, and the moment of a frame. */
export const frameOf = (seconds: number) => Math.round(seconds * FPS);
export const snapFrame = (seconds: number) => Math.round(seconds * FPS) / FPS;

const two = (n: number) => String(n).padStart(2, '0');

/** "00:01:07:13": hours, minutes, seconds and frames at 24 a second. */
export function timecode(seconds: number) {
	const frames = Math.max(0, frameOf(seconds));
	const s = Math.floor(frames / FPS);
	return `${two(Math.floor(s / 3600))}:${two(Math.floor(s / 60) % 60)}:${two(s % 60)}:${two(frames % FPS)}`;
}

/** A length for a small label: "2:13" is two seconds and thirteen frames, "1:02:13" with minutes. */
export function shortTime(seconds: number) {
	const sign = seconds < 0 ? '-' : '';
	const frames = Math.abs(frameOf(seconds));
	const s = Math.floor(frames / FPS);
	return `${sign}${s >= 60 ? `${Math.floor(s / 60)}:${two(s % 60)}` : s}:${two(frames % FPS)}`;
}

/**
 * Seconds out of what a person types: "1:07:13" (minutes, seconds, frames), "7:13" (seconds
 * and frames), "00:01:07:13", or plain seconds such as "7.5". `null` when it is none of these.
 */
export function parseTime(text: string): number | null {
	const t = text.trim();
	if (!t) return null;
	if (/^\d+([.,]\d+)?$/.test(t)) return Number(t.replace(',', '.'));
	const parts = t.split(':');
	if (parts.length < 2 || parts.length > 4 || parts.some((p) => !/^\d+$/.test(p))) return null;
	const n = parts.map(Number);
	const frames = n.pop()!;
	const s = n.pop() ?? 0;
	const m = n.pop() ?? 0;
	const h = n.pop() ?? 0;
	return h * 3600 + m * 60 + s + frames / FPS;
}

/** A place on the timeline: pixels a second, and how far it is scrolled. */
export interface Zoom {
	pps: number;
	scroll: number;
}

/** One frame is at most this wide, and a whole hour still fits a small screen. */
export const MAX_PPS = FPS * 24;
export const MIN_PPS = 0.25;

export const timeToX = (seconds: number, z: Zoom) => seconds * z.pps - z.scroll;
export const xToTime = (x: number, z: Zoom) => (x + z.scroll) / z.pps;

/** Zooms by `factor` so the moment under `x` stays under it. */
export function zoomAround(z: Zoom, x: number, factor: number, min = MIN_PPS, max = MAX_PPS): Zoom {
	const pps = Math.min(max, Math.max(min, z.pps * factor));
	const at = xToTime(x, z);
	return { pps, scroll: Math.max(0, at * pps - x) };
}

/** The zoom at which `seconds` fill `width` pixels, with a little air at the end. */
export function fitZoom(seconds: number, width: number, min = MIN_PPS, max = MAX_PPS): Zoom {
	const pps = Math.min(max, Math.max(min, (width * 0.96) / Math.max(1, seconds)));
	return { pps, scroll: 0 };
}

/** The zoom slider runs from 0 to 1 and feels even: each step multiplies. */
export const zoomToSlider = (pps: number, min = MIN_PPS, max = MAX_PPS) => Math.log(Math.min(max, Math.max(min, pps)) / min) / Math.log(max / min);
export const sliderToZoom = (at: number, min = MIN_PPS, max = MAX_PPS) => min * Math.pow(max / min, Math.min(1, Math.max(0, at)));

/** The steps between labelled marks on the ruler, in seconds, and how many small marks each holds. */
const STEPS: [number, number][] = [
	[1 / FPS, 1],
	[2 / FPS, 2],
	[4 / FPS, 4],
	[0.25, 6],
	[0.5, 2],
	[1, 4],
	[2, 2],
	[5, 5],
	[10, 2],
	[15, 3],
	[30, 2],
	[60, 4],
	[120, 2],
	[300, 5],
	[600, 2],
	[900, 3],
	[1800, 2],
	[3600, 4]
];

/**
 * The marks of the ruler at a zoom: labelled ones at least `room` pixels apart, small ones
 * between them. Frames are shown in the labels once a mark is less than a second.
 */
export function rulerSteps(pps: number, room = 84): { major: number; minor: number; frames: boolean } {
	const [major, parts] = STEPS.find(([step]) => step * pps >= room) ?? STEPS[STEPS.length - 1];
	// Small marks closer than five pixels only blur.
	const minor = (major / parts) * pps >= 5 ? major / parts : major;
	return { major, minor, frames: major < 1 };
}

/** The label of a ruler mark: "0:07", "1:02:05", and with frames "0:07:12". */
export function rulerLabel(seconds: number, frames: boolean) {
	const f = frameOf(seconds);
	const s = Math.floor(f / FPS);
	const base = s >= 3600 ? `${Math.floor(s / 3600)}:${two(Math.floor(s / 60) % 60)}:${two(s % 60)}` : `${Math.floor(s / 60)}:${two(s % 60)}`;
	return frames ? `${base}:${two(f % FPS)}` : base;
}

/**
 * Where a dragged moment lands: on the nearest of `marks` (region edges, the playhead) when one
 * is within `reach` pixels, else on the nearest frame. With snapping off it stays where it is.
 */
export function snapTime(seconds: number, marks: readonly number[], pps: number, on = true, reach = 7): { time: number; mark: number | null } {
	if (!on) return { time: Math.max(0, seconds), mark: null };
	let best: number | null = null;
	for (const m of marks) if (Math.abs(m - seconds) * pps <= reach && (best === null || Math.abs(m - seconds) < Math.abs(best - seconds))) best = m;
	return best === null ? { time: Math.max(0, snapFrame(seconds)), mark: null } : { time: best, mark: best };
}

/** Past this share of the window's width, the window turns a page while the film plays. */
export const PAGE_AT = 0.92;
/** Where the playhead is after a page is turned: near the left edge. */
const PAGE_FROM = 0.08;

/**
 * How far to scroll so a moment stays in view while it moves: nothing while it is in the
 * window; when it reaches the right edge (`PAGE_AT` of the width) the window turns a page, so
 * the playhead is near the left edge again, as a pro app pages. The lanes are drawn again once
 * a page, not on every frame. A moment behind the window is brought back near the left edge.
 */
export function followScroll(seconds: number, z: Zoom, width: number): number {
	const x = timeToX(seconds, z);
	if (x > width * PAGE_AT || x < 0) return Math.max(0, seconds * z.pps - width * PAGE_FROM);
	return z.scroll;
}

/**
 * The stretch a magnified view shows: `base` as it is at zoom 1, a part of it at more. The
 * part is held inside `base` and starts at `from` where it can.
 */
export function zoomedSpan(base: { from: number; to: number }, zoom: number, from: number): { from: number; to: number } {
	const whole = Math.max(0.001, base.to - base.from);
	const shown = whole / Math.max(1, zoom);
	const start = Math.min(base.to - shown, Math.max(base.from, from));
	return { from: start, to: start + shown };
}

/** A zoom changed so that the moment under `x` (0 to 1 across the view) stays under it: the new zoom and where the view starts. */
export function zoomSpanAround(base: { from: number; to: number }, view: { from: number; to: number }, factor: number, x: number, most = 64): { zoom: number; from: number } {
	const whole = Math.max(0.001, base.to - base.from);
	const was = Math.max(0.001, view.to - view.from);
	const zoom = Math.min(most, Math.max(1, (whole / was) * factor));
	const at = view.from + was * Math.min(1, Math.max(0, x));
	return { zoom, from: zoomedSpan(base, zoom, at - (whole / zoom) * Math.min(1, Math.max(0, x))).from };
}

/** Pixels to scroll this frame while something is dragged near an edge of the window: faster the closer. */
export function edgeScroll(x: number, width: number, margin = 36, top = 22): number {
	if (x < margin) return -Math.ceil(((margin - Math.max(0, x)) / margin) * top);
	if (x > width - margin) return Math.ceil(((Math.min(width, x) - (width - margin)) / margin) * top);
	return 0;
}
