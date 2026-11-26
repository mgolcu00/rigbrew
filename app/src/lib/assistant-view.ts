// How the steps of Studio's assistant become lines in its panel: one quiet line per thing done
// to the film ("Shot 3: description changed", "Added Ayşe to the cast", "Queued 4 shots"), a run
// of readings as one line, and before and after for a line that is opened. Nothing here touches
// the screen, so it is tested as it is (assistant-view.test.mjs).

import type { AgentMode, AgentStep, AgentToolStep } from './api';
import type { Activity } from './code-words';

/** The permissions of the assistant, for the same menu as Code's. */
export const filmModes: { id: AgentMode; name: string; does: string }[] = [
	{ id: 'ask', name: 'Ask first', does: 'Edits the film right away. Asks before it makes shots, dubs or exports.' },
	{ id: 'read_only', name: 'Read only', does: 'Only reads the film and says what it would change.' },
	{ id: 'bypass', name: 'Bypass permissions', does: 'Never asks. It also makes shots, dubs and exports on its own.' }
];

/** What an empty conversation offers to start with. */
export const firstPrompts = ['Make it a night scene', 'Add a narrator line to every shot', 'What is wrong with the pacing?'];

/** The tools that only read the film. */
const reads = new Set(['read_film', 'read_shot', 'look_at_shot']);
/** The tools that change the film itself: when one of them ends, the workstation reads the film again. */
export const changesFilm = new Set([
	'edit_shot',
	'add_shot',
	'remove_shot',
	'move_shot',
	'set_look',
	'set_language',
	'cast_add',
	'cast_edit',
	'rewrite_shot',
	'subtitles',
	'add_sound_region',
	'make_shots',
	'dub',
	'export'
]);
/** What takes long: it waits for a yes. */
const long = new Set(['make_shots', 'dub', 'export']);

export type FilmRow =
	| { kind: 'user' | 'model' | 'note' | 'question'; at: number; text: string }
	| { kind: 'step'; at: number; step: AgentToolStep }
	| { kind: 'reads'; at: number; steps: AgentToolStep[] };

/** The steps as rows: a run of readings is one row; the plan has its own place, so updating it is none. */
export function foldFilm(steps: readonly AgentStep[]): FilmRow[] {
	const rows: FilmRow[] = [];
	steps.forEach((step, at) => {
		if (step.kind !== 'tool') {
			rows.push({ kind: step.kind, at, text: step.text });
			return;
		}
		if (step.name === 'update_plan' && step.status !== 'failed') return;
		const last = rows.at(-1);
		if (reads.has(step.name) && step.status !== 'waiting') {
			if (last?.kind === 'reads') last.steps.push(step);
			else rows.push({ kind: 'reads', at, steps: [step] });
			return;
		}
		rows.push({ kind: 'step', at, step });
	});
	return rows;
}

const arg = (step: AgentToolStep, key: string) => {
	const v = step.arguments[key];
	return typeof v === 'string' || typeof v === 'number' ? String(v).trim() : '';
};
const firstLine = (text: string | null) => (text ?? '').split('\n')[0].trim().replace(/\.$/, '');

/** What a step was going to do, in a few words: for a step that runs, waits, or did not happen. */
function tried(step: AgentToolStep): string {
	const shot = arg(step, 'shot');
	switch (step.name) {
		case 'edit_shot':
			return shot ? `Shot ${shot}: edit` : 'Edit a shot';
		case 'add_shot':
			return 'Add a shot';
		case 'remove_shot':
			return shot ? `Remove shot ${shot}` : 'Remove a shot';
		case 'move_shot':
			return shot ? `Move shot ${shot}` : 'Move a shot';
		case 'set_look':
			return 'Set the look';
		case 'set_language':
			return arg(step, 'language') ? `Lines in ${arg(step, 'language')}` : 'Set the language';
		case 'cast_add':
			return arg(step, 'name') ? `Add ${arg(step, 'name')} to the cast` : 'Add to the cast';
		case 'cast_edit':
			return arg(step, 'name') ? `Change ${arg(step, 'name')}` : 'Change the cast';
		case 'rewrite_shot':
			return shot ? `Shot ${shot}: write again` : 'Write a shot again';
		case 'subtitles':
			return 'Subtitles';
		case 'add_sound_region':
			return arg(step, 'file') ? `Add ${arg(step, 'file')}` : 'Add a sound';
		case 'make_shots':
			return 'Make shots';
		case 'dub':
			return arg(step, 'language') ? `Dub in ${arg(step, 'language')}` : 'Dub';
		case 'export':
			return 'Export';
		case 'update_plan':
			return 'Plan';
		default:
			return step.name;
	}
}

/** The words of a step that is under way. */
function going(step: AgentToolStep): string {
	switch (step.name) {
		case 'make_shots':
			return 'Queueing shots…';
		case 'dub':
			return 'Dubbing…';
		case 'export':
			return 'Exporting…';
		default:
			return `${tried(step)}…`;
	}
}

/** One step as the line the person reads, and what is said after it when it did not simply work. */
export function filmLine(step: AgentToolStep): { text: string; tail: string } {
	switch (step.status) {
		case 'done':
			return { text: firstLine(step.result) || tried(step), tail: '' };
		case 'running':
			return { text: going(step), tail: '' };
		case 'waiting':
			return { text: tried(step), tail: 'waiting for you' };
		case 'denied':
			return { text: tried(step), tail: 'not allowed' };
		case 'stopped':
			return { text: tried(step), tail: 'stopped' };
		default:
			return { text: tried(step), tail: 'did not work' };
	}
}

/** A run of readings as one line: "Read the film", "Read shots 2 and 3", "Looked at shot 4". */
export function readsLine(steps: readonly AgentToolStep[]): string {
	const and = (items: string[]) => (items.length < 2 ? items.join('') : `${items.slice(0, -1).join(', ')} and ${items.at(-1)}`);
	const unique = (name: string) => [...new Set(steps.filter((s) => s.name === name).map((s) => arg(s, 'shot')).filter(Boolean))];
	const parts: string[] = [];
	if (steps.some((s) => s.name === 'read_film')) parts.push('Read the film');
	const shots = unique('read_shot');
	if (shots.length) parts.push(`${parts.length ? 'read' : 'Read'} ${shots.length === 1 ? 'shot' : 'shots'} ${and(shots)}`);
	const looked = unique('look_at_shot');
	if (looked.length) parts.push(`${parts.length ? 'looked at' : 'Looked at'} ${looked.length === 1 ? 'shot' : 'shots'} ${and(looked)}`);
	const running = steps.some((s) => s.status === 'running');
	return (parts.length ? and(parts) : 'Read the film') + (running ? '…' : '');
}

/** Before and after of a change, as rows: "-" what it was, "+" what it is. */
export function diffRows(diff: string | null): { sign: '-' | '+'; text: string }[] {
	return (diff ?? '')
		.split('\n')
		.filter((l) => l.startsWith('- ') || l.startsWith('+ ') || l === '-' || l === '+')
		.map((l) => ({ sign: l[0] as '-' | '+', text: l.slice(2) }));
}

/** What the assistant is doing right now, for the word in the status line. */
export function filmActivity(steps: readonly AgentStep[]): Activity {
	const last = steps.at(-1);
	if (last?.kind !== 'tool' || last.status !== 'running') return 'thinking';
	if (reads.has(last.name)) return 'reading';
	if (long.has(last.name)) return 'running';
	if (changesFilm.has(last.name)) return 'writing';
	return 'thinking';
}

/** How many changes to the film a conversation has finished: when it grows, the film is read again. */
export function changesDone(steps: readonly AgentStep[]): number {
	return steps.filter((s) => s.kind === 'tool' && changesFilm.has(s.name) && s.status !== 'running' && s.status !== 'waiting').length;
}

/** The heaviest installed chat model, the one the work goes to unless another is chosen (as in Code). */
export function heaviest<P extends { installed: readonly string[]; variants: readonly { id: string; disk_mb: number }[] }>(packages: readonly P[]): P | null {
	const weight = (p: P) => Math.max(0, ...p.variants.filter((v) => p.installed.includes(v.id)).map((v) => v.disk_mb));
	return [...packages].sort((a, b) => weight(b) - weight(a))[0] ?? null;
}
