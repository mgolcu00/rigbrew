// How much the interface shows. One setting, three levels, and every component reads the same
// flags from here instead of asking "is this Pro?" on its own. A new install starts Guided;
// the app suggests the next level when real use says the person is ready (see guide.svelte.ts)
// and never switches by itself.

import { remembered } from './remember';

export type Level = 'guided' | 'standard' | 'pro';
export const LEVEL_ORDER: Level[] = ['guided', 'standard', 'pro'];

export interface Flags {
	/** Names lead with what a model does: "Chat (Steep 4B)". */
	plainNames: boolean;
	/** Friendly version names next to model names: "Steep 4B · Balanced". */
	versionNames: boolean;
	/** Raw build ids (q5, int8), install ids and other identifiers. */
	buildIds: boolean;
	/** Speed as a word ("fast"), with the measured number on hover. */
	speedWords: boolean;
	/** Speed as a number (tokens per second) wherever it is known. */
	speedNumbers: boolean;
	/** Models: one row per model, the other versions behind "Other versions". */
	otherVersions: boolean;
	/** Models: the full builds table with measured quality metrics, sorting and filters. */
	buildsTable: boolean;
	/** Key caps on buttons, in menus and in the palette. */
	keyHints: boolean;
	/** The list of shortcuts in the chat's side pane. */
	keysPane: boolean;
	/** Digits next to the title-bar places (Cmd/Ctrl+1 to 3). */
	tabDigits: boolean;
	/** "/ for commands" in the composer, and where an example prompt is routed. */
	slashHints: boolean;
	/** The bay draws the memory budget as a ruler with numbers; otherwise a plain bar and words. */
	ruler: boolean;
	/** What the bay says about usage data waiting to be sent: nothing, a few words, or the raw queue. */
	queue: 'none' | 'words' | 'full';
	/** The side pane shows this chat's figures and today's totals (chats are always there). */
	paneFigures: boolean;
	/** The getting-started checklist in the empty chat. */
	checklist: boolean;
	/** Small "?" marks next to terms, opening the help article about them. */
	hints: boolean;
	/** One-time tips: all of them, only when something new appears, or none. */
	tips: 'all' | 'new' | 'none';
	/** Help is a labelled button in the title bar, not only a "?". */
	helpLabel: boolean;
	/** The empty chat is the plain first screen rather than the full inventory. */
	plainStart: boolean;
	/** The raw message under "Details" on every error, not only on ones without a plain sentence. */
	rawErrors: boolean;
	/** Technical figures: tokens, CPU features, architecture, hardware class, data folder, queue ids. */
	techDetails: boolean;
}

export interface LevelInfo {
	id: Level;
	name: string;
	/** One line for Settings and the palette. */
	line: string;
	/** What switching to this level changes, said once in the notice line. */
	change: string;
	flags: Flags;
}

export const LEVELS: Record<Level, LevelInfo> = {
	guided: {
		id: 'guided',
		name: 'Guided',
		line: 'Plain words and one button per model. For getting started.',
		change: 'Guided: plain names and one row per model.',
		flags: {
			plainNames: true,
			versionNames: false,
			buildIds: false,
			speedWords: false,
			speedNumbers: false,
			otherVersions: false,
			buildsTable: false,
			keyHints: false,
			keysPane: false,
			tabDigits: false,
			slashHints: false,
			ruler: false,
			queue: 'none',
			paneFigures: false,
			checklist: true,
			hints: true,
			tips: 'all',
			helpLabel: true,
			plainStart: true,
			rawErrors: false,
			techDetails: false
		}
	},
	standard: {
		id: 'standard',
		name: 'Standard',
		line: 'Model versions and speeds are shown.',
		change: 'Standard: model versions and speeds are shown.',
		flags: {
			plainNames: false,
			versionNames: true,
			buildIds: false,
			speedWords: true,
			speedNumbers: false,
			otherVersions: true,
			buildsTable: false,
			keyHints: true,
			keysPane: false,
			tabDigits: false,
			slashHints: false,
			ruler: true,
			queue: 'words',
			paneFigures: true,
			checklist: true,
			hints: true,
			tips: 'new',
			helpLabel: false,
			plainStart: false,
			rawErrors: false,
			techDetails: false
		}
	},
	pro: {
		id: 'pro',
		name: 'Pro',
		line: 'Everything: every build with its measurements, raw ids, shortcuts and the queue.',
		change: 'Pro: every build with its measurements, raw ids, shortcuts, slash commands and the queue.',
		flags: {
			plainNames: false,
			versionNames: false,
			buildIds: true,
			speedWords: false,
			speedNumbers: true,
			otherVersions: false,
			buildsTable: true,
			keyHints: true,
			keysPane: true,
			tabDigits: true,
			slashHints: true,
			ruler: true,
			queue: 'full',
			paneFigures: true,
			checklist: false,
			hints: false,
			tips: 'none',
			helpLabel: false,
			plainStart: false,
			rawErrors: true,
			techDetails: true
		}
	}
};

export type TextSize = 'small' | 'default' | 'large' | 'larger';

export const TEXT_SIZES: { id: TextSize; name: string; scale: number }[] = [
	{ id: 'small', name: 'Small', scale: 0.9 },
	{ id: 'default', name: 'Default', scale: 1 },
	{ id: 'large', name: 'Large', scale: 1.12 },
	{ id: 'larger', name: 'Larger', scale: 1.25 }
];

const isLevel = (v: unknown): v is Level => v === 'guided' || v === 'standard' || v === 'pro';
const isSize = (v: unknown): v is TextSize => TEXT_SIZES.some((s) => s.id === v);

class Experience {
	level = $state<Level>(isLevel(remembered.get('level')) ? (remembered.get('level') as Level) : 'guided');
	textSize = $state<TextSize>(isSize(remembered.get('text-size')) ? (remembered.get('text-size') as TextSize) : 'default');

	info = $derived(LEVELS[this.level]);
	/** What every component reads. */
	flags = $derived(LEVELS[this.level].flags);
	scale = $derived(TEXT_SIZES.find((s) => s.id === this.textSize)?.scale ?? 1);

	/** The next level up, or null at Pro. */
	next = $derived(LEVEL_ORDER[LEVEL_ORDER.indexOf(this.level) + 1] ?? null);

	setLevel(level: Level) {
		this.level = level;
		remembered.set('level', level);
	}

	setTextSize(size: TextSize) {
		this.textSize = size;
		remembered.set('text-size', size);
		this.applyTextSize();
	}

	/**
	 * Scales the whole interface. In the app the webview zooms like a browser does (the layout
	 * reflows at the smaller width, so narrow-window rules apply); in a plain browser the page
	 * is zoomed with CSS, which the dev preview is good enough with.
	 */
	async applyTextSize() {
		const scale = this.scale;
		const root = document.documentElement;
		root.dataset.textSize = this.textSize;
		if ('__TAURI_INTERNALS__' in window) {
			try {
				const { getCurrentWebview } = await import('@tauri-apps/api/webview');
				await getCurrentWebview().setZoom(scale);
				root.style.removeProperty('zoom');
				return;
			} catch {
				// an older shell without the permission: fall through to CSS
			}
		}
		if (scale === 1) root.style.removeProperty('zoom');
		else root.style.setProperty('zoom', String(scale));
	}
}

export const ux = new Experience();
