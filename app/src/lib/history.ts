// The history in the sidebar: one list with a tab for each kind of work (chats, Code tasks,
// films), and what a chat is named after. Plain functions, so they can be tested without the app.

export type HistoryTab = 'chat' | 'code' | 'studio';

export const historyTabs: { id: HistoryTab; name: string }[] = [
	{ id: 'chat', name: 'Chat' },
	{ id: 'code', name: 'Code' },
	{ id: 'studio', name: 'Studio' }
];

/** The tab a screen belongs to, or null for a screen that has no history of its own. */
export function tabOf(screen: string): HistoryTab | null {
	return screen === 'chat' || screen === 'code' || screen === 'studio' ? screen : null;
}

/** The tab that was remembered, when it is still one that is shown. */
export function tabFrom(kept: string | null, shown: HistoryTab[]): HistoryTab {
	return shown.find((t) => t === kept) ?? shown[0] ?? 'chat';
}

/** The last part of a path: the folder's own name. */
export function folderName(path: string): string {
	return path.split(/[/\\]/).filter(Boolean).pop() ?? path;
}

interface TaskLike {
	id: string;
	folder: string;
	title: string;
	state: string;
	created_ms: number;
	request_ms?: number;
}

export interface CodeRow {
	id: string;
	folder: string;
	title: string;
	/** The folder's own name, shown faintly next to the title. */
	where: string;
	/** A small dot: the task is working, or it waits for the person. */
	mark: '' | 'working' | 'waiting';
	/** When the person last asked it for something. */
	at: number;
}

/**
 * Every Code task of every folder, newest first. The order goes by when the person last asked
 * for something, not by the last change, so a task that is working does not jump about.
 */
export function codeRows(tasks: TaskLike[]): CodeRow[] {
	return tasks
		.map((t) => ({
			id: t.id,
			folder: t.folder,
			title: t.title.trim() || 'Untitled task',
			where: folderName(t.folder),
			mark: t.state === 'running' ? ('working' as const) : t.state === 'waiting' || t.state === 'asking' ? ('waiting' as const) : ('' as const),
			at: t.request_ms || t.created_ms
		}))
		.sort((a, b) => b.at - a.at || (a.id < b.id ? -1 : 1));
}

interface MessageLike {
	role: string;
	content: string;
	state?: string;
}

/** A first exchange shorter than this says little: the chat is named again a few turns later. */
const THIN = 120;
/** How many things the person must have sent before a thin name is written again. */
export const RETITLE_AFTER = 3;

/**
 * What a chat is named after: what the person wrote first and how the first answer began.
 * With `again`, the person's first few messages and the newest answer. Null when the person has
 * written nothing yet.
 */
export function titleSource(messages: MessageLike[], again = false): { text: string; answer: string; thin: boolean } | null {
	const sent = messages.filter((m) => m.role === 'user' && m.content.trim());
	if (sent.length === 0) return null;
	const answers = messages.filter((m) => m.role === 'assistant' && m.state === 'done' && m.content.trim());
	const text = again
		? sent
				.slice(0, RETITLE_AFTER)
				.map((m) => m.content.trim())
				.join('\n')
		: sent[0].content.trim();
	const answer = (again ? answers.at(-1) : answers[0])?.content.trim() ?? '';
	return { text, answer, thin: !again && text.length + answer.length < THIN };
}

/** Whether a title the model wrote may replace the one the chat has. A name the person typed stays. */
export function mayRetitle(titleBy: string | undefined, retitle: boolean | undefined, sent: number): 'first' | 'again' | null {
	if (titleBy === 'words') return 'first';
	if (titleBy === 'model' && retitle && sent >= RETITLE_AFTER) return 'again';
	return null;
}
