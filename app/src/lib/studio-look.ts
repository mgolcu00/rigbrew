// The colours of the Studio workstation, for what is drawn on canvas (lanes, ruler, meters).
// The same values stand as CSS variables on the Studio screen (see Studio.svelte).

export const LOOK = {
	bg: '#0c0d0f',
	panel: '#151619',
	raised: '#1c1e22',
	lane: '#18191c',
	laneOn: '#1f2126',
	line: '#2a2c32',
	lineSoft: '#212327',
	edge: '#060607',
	ink: '#e3e5e9',
	muted: '#9a9ea8',
	faint: '#686c76',
	accent: '#5b9df0',
	playhead: '#f4f5f7',
	cycle: '#e3b341',
	ok: '#4fbf8b',
	warn: '#e0a93f',
	fail: '#e5645c',
	meterLow: '#35c46a',
	meterMid: '#e2c443',
	meterHigh: '#ee5546'
} as const;

/** The colour of a shot on the film track, by what state its clip is in. */
export const STATE_COLOR = {
	empty: '#383b43',
	queued: '#4b5060',
	making: '#2c62a6',
	done: '#2a7688',
	changed: '#8a6a26',
	failed: '#8f3834'
} as const;

export const STATE_WORD = {
	empty: 'Not made',
	queued: 'Waiting',
	making: 'Being made',
	done: 'Made',
	changed: 'Changed',
	failed: 'Did not work'
} as const;

const parse = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));

/** A colour moved toward another by `amount` (0 to 1). */
export function blend(hex: string, toward: string, amount: number) {
	const a = parse(hex);
	const b = parse(toward);
	return `#${a.map((v, i) => Math.round(v + (b[i] - v) * amount).toString(16).padStart(2, '0')).join('')}`;
}

export const lighter = (hex: string, amount: number) => blend(hex, '#ffffff', amount);
export const darker = (hex: string, amount: number) => blend(hex, '#000000', amount);

export function alpha(hex: string, a: number) {
	const [r, g, b] = parse(hex);
	return `rgba(${r},${g},${b},${a})`;
}

/** The last part of a path: a file's name. */
export const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

/** The first words of a description, for a label. */
export function firstWords(text: string, most = 6) {
	const words = text.trim().split(/\s+/).filter(Boolean);
	return words.slice(0, most).join(' ') + (words.length > most ? '…' : '');
}
