// What went wrong, in plain words, with the one thing to do about it. The service's own
// message stays available under "Details" (always in Pro; elsewhere when no plain sentence
// matched, so nothing is hidden that could help).

import { LOST } from './api';
import { machine } from './words';

/** What the person can do next. The screen that shows the error wires it up. */
export type Fix = 'retry' | 'models' | 'help-fits' | 'help-disk' | 'help-first' | 'privacy' | 'none';

export interface Plain {
	/** A short class for counting errors without their text: disk, fit, lost… */
	id: string;
	text: string;
	/** Two or three words, for a table cell: "Not enough disk space". */
	short: string;
	fix: Fix;
	/** The button's words for `fix`. */
	act: string;
	raw: string;
	/** Whether a plain sentence was found; otherwise the raw message is the only detail there is. */
	known: boolean;
}

const rules: { id: string; test: RegExp; text: string; short: string; fix: Fix; act: string }[] = [
	{
		id: 'lost',
		test: /stopped answering|bringing it back|no running service|failed to fetch|load failed|networkerror when attempting/i,
		text: 'Lost the connection to Rigbrew’s background service. It is being restarted; nothing you made is lost.',
		short: 'Connection lost',
		fix: 'retry',
		act: 'Try again'
	},
	{
		id: 'video-busy',
		test: /a video is (already )?being made/i,
		text: 'A video clip is still being made, and only one model fits in memory at a time. Wait for the clip to finish, or press Stop on its card, then try again.',
		short: 'A clip is being made',
		fix: 'none',
		act: ''
	},
	{
		id: 'video-closed',
		test: /rigbrew was closed while this clip/i,
		text: 'Rigbrew was closed while this clip was being made, so it was not finished. Make it again to get it.',
		short: 'Interrupted',
		fix: 'none',
		act: ''
	},
	{
		id: 'engine',
		test: /engine not installed|could not start the engine/i,
		text: `The program that runs this model is missing on this ${machine}. Installing Rigbrew again puts it back.`,
		short: 'Engine missing',
		fix: 'none',
		act: ''
	},
	{
		id: 'disk',
		test: /not enough disk space|no space left/i,
		text: 'There is not enough free disk space for this download. Remove a model you do not use, or free some space, then try again.',
		short: 'Not enough disk space',
		fix: 'help-disk',
		act: 'Free up space'
	},
	{
		id: 'fit',
		test: /no variant that fits|does not fit|too heavy|over budget/i,
		text: `This model needs more memory than this ${machine} can give it.`,
		short: `Too big for this ${machine}`,
		fix: 'help-fits',
		act: 'What fits?'
	},
	{
		id: 'start',
		test: /exited while loading|not ready after|out of memory|failed to allocate/i,
		text: 'The model could not start. This happens when memory is short: close some apps and try again.',
		short: 'Could not start',
		fix: 'retry',
		act: 'Try again'
	},
	{
		id: 'not_installed',
		test: /is not installed|not installed any more|no (chat |image )?package/i,
		text: 'This model is not installed. Install it again in Models.',
		short: 'Not installed',
		fix: 'models',
		act: 'Open Models'
	},
	{
		id: 'damaged',
		test: /failed verification|sha256/i,
		text: 'The download was damaged on the way and was thrown away. Try again.',
		short: 'Download damaged',
		fix: 'retry',
		act: 'Try again'
	},
	{
		id: 'network',
		test: /network error|error sending request|dns|connection (refused|reset|closed)|timed out|answered HTTP 5\d\d|answered HTTP 4\d\d/i,
		text: 'The download server could not be reached. Check the internet connection and try again; the download continues where it stopped.',
		short: 'Server not reachable',
		fix: 'retry',
		act: 'Try again'
	},
	{
		id: 'server',
		test: /server cannot be reached|could not be reached|api\.rigbrew\.dev/i,
		text: 'The Rigbrew server could not be reached. Nothing is lost; Rigbrew tries again later.',
		short: 'Server not reachable',
		fix: 'none',
		act: ''
	},
	{
		id: 'model',
		test: /engine answered HTTP|engine request failed|returned no image/i,
		text: 'The model stopped with an error before it was done. Trying again usually works.',
		short: 'Model error',
		fix: 'retry',
		act: 'Try again'
	}
];

/** Turns any error (or message) into a plain sentence and a next step. */
export function plain(error: unknown): Plain {
	const raw = error instanceof Error ? error.message : String(error ?? '');
	if (raw === LOST) return { id: 'lost', text: rules[0].text, short: rules[0].short, fix: 'retry', act: 'Try again', raw, known: true };
	for (const rule of rules) {
		if (rule.test.test(raw)) return { id: rule.id, text: rule.text, short: rule.short, fix: rule.fix, act: rule.act, raw, known: true };
	}
	return { id: 'unknown', text: 'Something went wrong. Trying again usually works.', short: 'It did not work', fix: 'retry', act: 'Try again', raw, known: false };
}
