// The box to type in on the Code screen, as logic: which menu opens while the person types (`/`
// for a command, `@` for a file), what is in it, and what choosing an entry does to the text.
// Nothing here touches the screen, so it is tested as it is (code-compose.test.mjs).

/** One entry of the `/` menu. `send`: the service knows it and the model does the work. `app`: done here. */
export interface Command {
	name: string;
	hint: string;
	kind: 'send' | 'app';
}

/** The commands there always are, in the order they are listed. Skills follow them. */
export const builtIn: Command[] = [
	{ name: 'init', hint: 'Write AGENTS.md for this folder', kind: 'send' },
	{ name: 'review', hint: 'Review the uncommitted changes', kind: 'send' },
	{ name: 'test', hint: 'Run the checks and fix what fails', kind: 'send' },
	{ name: 'explain', hint: 'Explain this codebase, or what you name', kind: 'send' },
	{ name: 'clear', hint: 'A new task in this folder', kind: 'app' },
	{ name: 'model', hint: 'Choose the model', kind: 'app' },
	{ name: 'mode', hint: 'Choose the permissions', kind: 'app' }
];

/** Every command: the built-in ones, then one per skill. A skill never hides a built-in command. */
export function commands(skills: readonly { name: string; description: string }[]): Command[] {
	const taken = new Set(builtIn.map((c) => c.name));
	const own = skills.filter((s) => !taken.has(s.name)).map((s) => ({ name: s.name, hint: s.description, kind: 'send' as const }));
	return [...builtIn, ...own];
}

/** What the open menu is about: a command being typed, or a file being named. */
export type Typing = { kind: 'slash'; query: string } | { kind: 'at'; query: string; start: number } | null;

/**
 * What is being typed at the caret: `/` and letters at the very start of the box (and nothing
 * after them yet), or `@` and the letters of a path at the start of a word.
 */
export function typing(draft: string, caret: number): Typing {
	const before = draft.slice(0, caret);
	const slash = /^\/([\w-]*)$/.exec(before);
	if (slash && !/\S/.test(draft.slice(caret, caret + 1))) return { kind: 'slash', query: slash[1] };
	const at = /(^|[\s([{"'])@([^\s@]*)$/.exec(before);
	if (at) return { kind: 'at', query: at[2], start: before.length - at[2].length - 1 };
	return null;
}

/** The commands whose name starts with what was typed, then those that have it elsewhere in the name. */
export function matching(list: readonly Command[], query: string): Command[] {
	const q = query.toLowerCase();
	const starts = list.filter((c) => c.name.startsWith(q));
	const within = list.filter((c) => !c.name.startsWith(q) && c.name.includes(q));
	return [...starts, ...within];
}

/** The text with a command chosen: `/name ` and whatever was typed after the command before. */
export function withCommand(draft: string, name: string): string {
	const rest = draft.replace(/^\/[\w-]*\s*/, '');
	return `/${name} ${rest}`;
}

/** The text with a file chosen for the `@` being typed: `@path ` in its place. Returns the text and where the caret goes. */
export function withMention(draft: string, start: number, caret: number, path: string): { text: string; caret: number } {
	const after = draft.slice(caret).replace(/^\S*/, '');
	const inserted = `@${path}${path.endsWith('/') || /^\s/.test(after) ? '' : ' '}`;
	return { text: draft.slice(0, start) + inserted + after, caret: start + inserted.length };
}

/** An app command at the start of the text, with what follows it: `/clear`, `/model`, `/mode`. */
export function appCommand(draft: string): { name: string; rest: string } | null {
	const found = /^\/(clear|model|mode)(?:\s+(.*))?$/s.exec(draft.trim());
	return found ? { name: found[1], rest: (found[2] ?? '').trim() } : null;
}

/** "34% of context", from the engine's count. Empty when there is none yet. */
export function contextShown(context: { used: number; size: number } | null | undefined): string {
	if (!context || !context.size) return '';
	const percent = Math.min(100, Math.max(1, Math.round((context.used / context.size) * 100)));
	return `${percent}% of context`;
}

/** "Thought for 12 s", "Thought for a moment" when it was under a second. */
export function thoughtFor(ms: number): string {
	if (!ms || ms < 1000) return 'Thought for a moment';
	const s = Math.round(ms / 1000);
	return s < 60 ? `Thought for ${s} s` : `Thought for ${Math.floor(s / 60)} min ${s % 60} s`;
}

/** The longest side of a picture that is sent; a larger one is scaled down first. */
export const PICTURE_SIDE = 1600;

/** The size a picture is scaled to so that neither side is longer than `PICTURE_SIDE`. */
export function fitted(width: number, height: number): { width: number; height: number } {
	const scale = Math.min(1, PICTURE_SIDE / Math.max(width, height, 1));
	return { width: Math.max(1, Math.round(width * scale)), height: Math.max(1, Math.round(height * scale)) };
}
