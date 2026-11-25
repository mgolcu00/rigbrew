// How the steps of a Code task become the lines on the screen. One compact line per thing
// done: a change and a command each have their own, and a run of look-ups (reading, listing,
// searching, the web) is one line that opens to show them. Nothing here touches the screen, so
// it is tested as it is (code-view.test.mjs).

import type { AgentStep, AgentToolStep } from './api';
import type { Activity } from './code-words';

type Group = 'read' | 'search' | 'list' | 'web' | 'page';

/** The tools that only look something up. Connector tools are not among them: they act outside the folder. */
const lookups: Record<string, Group> = {
	read_file: 'read',
	outline: 'read',
	use_skill: 'read',
	read_skill_file: 'read',
	search: 'search',
	list_files: 'list',
	web_search: 'web',
	fetch_url: 'page'
};

export const isLookup = (step: AgentStep): step is AgentToolStep => step.kind === 'tool' && step.name in lookups && step.status !== 'waiting';

export type Row =
	| { kind: 'user' | 'model' | 'note' | 'question'; at: number; text: string }
	| { kind: 'step'; at: number; step: AgentToolStep }
	| { kind: 'lookups'; at: number; steps: AgentToolStep[] };

/**
 * The steps as rows. A run of look-ups is one row; it ends when a change, a command, a
 * question or an answer comes. The plan has its own place on the screen, so a step that only
 * updated it is no row and does not end a run.
 */
export function fold(steps: readonly AgentStep[]): Row[] {
	const rows: Row[] = [];
	steps.forEach((step, at) => {
		if (step.kind !== 'tool') {
			rows.push({ kind: step.kind, at, text: step.text });
			return;
		}
		if (step.name === 'update_plan' && step.status !== 'failed') return;
		const last = rows.at(-1);
		if (isLookup(step)) {
			if (last?.kind === 'lookups') last.steps.push(step);
			else rows.push({ kind: 'lookups', at, steps: [step] });
			return;
		}
		rows.push({ kind: 'step', at, step });
	});
	return rows;
}

const arg = (step: AgentToolStep, key: string) => (typeof step.arguments[key] === 'string' ? (step.arguments[key] as string) : '');
const many = (n: number, one: string, more = `${one}s`) => `${n} ${n === 1 ? one : more}`;
const times = (n: number) => (n > 1 ? many(n, 'time') : '');

export interface Summary {
	/** True while one of the look-ups is still going. */
	running: boolean;
	/** While it runs: "Searching…". Afterwards empty; see `parts`. */
	label: string;
	/** Afterwards: "Read" "4 files", "Searched" "2 times", "Searched the web" "". */
	parts: { label: string; count: string }[];
	/** How many of them did not work. */
	failed: number;
}

const doing: Record<Group, string> = { read: 'Reading…', search: 'Searching…', list: 'Searching…', web: 'Searching the web…', page: 'Opening a page…' };

/** A run of look-ups as one line: what is going on, or what was done and how often. Never what was searched for. */
export function summary(steps: readonly AgentToolStep[]): Summary {
	const running = steps.find((s) => s.status === 'running');
	const failed = steps.filter((s) => s.status === 'failed').length;
	const counts = new Map<Group, number>();
	const files = new Set<string>();
	for (const step of steps) {
		const group = lookups[step.name];
		counts.set(group, (counts.get(group) ?? 0) + 1);
		if (group === 'read') files.add(`${arg(step, 'name')}/${arg(step, 'path')}`);
	}
	const parts = [...counts].map(([group, n]) => {
		switch (group) {
			case 'read':
				return { label: 'Read', count: many(files.size, 'file') };
			case 'search':
				return { label: 'Searched', count: times(n) };
			case 'list':
				return { label: 'Listed files', count: times(n) };
			case 'web':
				return { label: 'Searched the web', count: times(n) };
			default:
				return { label: 'Opened', count: many(n, 'page') };
		}
	});
	return { running: Boolean(running), label: running ? doing[lookups[running.name]] : '', parts, failed };
}

export interface Line {
	/** A short label: "Read", "Edit", "Run". */
	verb: string;
	/** What it was done to, shown in the code font: a path, a command, a query. */
	code: string;
	/** Lines added and removed, for a change with a diff. */
	plus?: number;
	minus?: number;
}

/** The lines a unified diff adds and removes. */
export function diffCount(diff: string): { plus: number; minus: number } {
	let plus = 0;
	let minus = 0;
	for (const l of diff.split('\n')) {
		if (l.startsWith('+') && !l.startsWith('+++')) plus++;
		else if (l.startsWith('-') && !l.startsWith('---')) minus++;
	}
	return { plus, minus };
}

/** One step as a label and the thing it was about. */
export function lineOf(step: AgentToolStep): Line {
	const path = arg(step, 'path');
	const change = (verb: string): Line => ({ verb, code: path, ...(step.diff ? diffCount(step.diff) : {}) });
	switch (step.name) {
		case 'read_file': {
			const from = step.arguments.from_line;
			return { verb: 'Read', code: from ? `${path}:${from}` : path };
		}
		case 'outline':
			return { verb: 'Outline', code: path || '.' };
		case 'list_files':
			return { verb: 'List', code: path || '.' };
		case 'search':
			return { verb: 'Search', code: path ? `${arg(step, 'pattern')}  in ${path}` : arg(step, 'pattern') };
		case 'web_search':
			return { verb: 'Web', code: arg(step, 'query') };
		case 'fetch_url':
			return { verb: 'Open', code: arg(step, 'url').replace(/^https?:\/\//, '') };
		case 'use_skill':
			return { verb: 'Skill', code: arg(step, 'name') };
		case 'read_skill_file':
			return { verb: 'Skill', code: `${arg(step, 'name')}/${path}` };
		case 'edit_file':
			return change('Edit');
		case 'write_file':
			return change('Write');
		case 'append_file':
			return change('Append');
		case 'run_command':
			return { verb: 'Run', code: arg(step, 'command') };
		case 'update_plan':
			return { verb: 'Plan', code: '' };
		case 'find_tool':
			return { verb: 'Find tool', code: arg(step, 'query') };
		case 'call_tool':
			return { verb: 'Use', code: `${arg(step, 'connector')} ${arg(step, 'tool')}`.trim() };
		default:
			return { verb: step.name, code: '' };
	}
}

/** What a step that is not simply done says at its end. */
export const tails: Record<AgentToolStep['status'], string> = {
	running: '',
	waiting: 'waiting',
	done: '',
	failed: 'failed',
	denied: 'denied',
	stopped: 'stopped'
};

/** What is happening right now, for the word above the box to type in. */
export function activityOf(steps: readonly AgentStep[]): Activity {
	const last = steps.at(-1);
	if (last?.kind !== 'tool' || last.status !== 'running') return 'thinking';
	switch (last.name) {
		case 'search':
		case 'list_files':
		case 'find_tool':
			return 'searching';
		case 'read_file':
		case 'outline':
		case 'use_skill':
		case 'read_skill_file':
			return 'reading';
		case 'edit_file':
		case 'write_file':
		case 'append_file':
			return 'writing';
		case 'run_command':
			return 'running';
		case 'web_search':
		case 'fetch_url':
			return 'web';
		default:
			return 'thinking';
	}
}
