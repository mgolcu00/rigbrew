// Subtitles as pictures. The packaged film tool cannot draw text, so an export that holds its
// subtitles in the frames gets each one from here: drawn on a canvas as wide as the exported
// film, white with a dark edge, and handed to the service as plain points.

import { captionRows } from './studio-edit.ts';

/** How a subtitle is set in a picture of this height: the size of its letters, the room under it. */
export function captionLook(height: number) {
	const size = Math.max(12, Math.round(height * 0.052));
	return { size, line: Math.round(size * 1.3), margin: Math.round(height * 0.05), font: `600 ${size}px "Instrument Sans", -apple-system, "Segoe UI", sans-serif` };
}

/** The height of the strip a subtitle is drawn in, for a film of this height: three rows and the room under them, an even number. */
export function captionHeight(height: number) {
	const look = captionLook(height);
	return Math.min(height, Math.ceil((look.line * 3 + look.margin + look.size * 0.6) / 2) * 2);
}

const base64 = (bytes: Uint8ClampedArray) => {
	let text = '';
	for (let at = 0; at < bytes.length; at += 0x8000) text += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
	return btoa(text);
};

/**
 * One subtitle as a picture: `width` wide and as high as `captionHeight` says, clear but for the
 * words, which sit at its bottom. `null` where there is no canvas to draw on.
 */
export function drawCaption(text: string, width: number, filmHeight: number): { width: number; height: number; rgba: string } | null {
	if (typeof document === 'undefined') return null;
	const height = captionHeight(filmHeight);
	const canvas = document.createElement('canvas');
	canvas.width = width;
	canvas.height = height;
	const ctx = canvas.getContext('2d', { willReadFrequently: true });
	if (!ctx) return null;
	const look = captionLook(filmHeight);
	ctx.font = look.font;
	ctx.textAlign = 'center';
	ctx.textBaseline = 'alphabetic';
	ctx.lineJoin = 'round';
	const rows = captionRows(text, width * 0.86, (t) => ctx.measureText(t).width);
	rows.forEach((row, i) => {
		const y = height - look.margin - (rows.length - 1 - i) * look.line;
		ctx.strokeStyle = 'rgba(0, 0, 0, 0.85)';
		ctx.lineWidth = Math.max(2, look.size * 0.22);
		ctx.strokeText(row, width / 2, y);
		ctx.fillStyle = '#fff';
		ctx.fillText(row, width / 2, y);
	});
	return { width, height, rgba: base64(ctx.getImageData(0, 0, width, height).data) };
}
