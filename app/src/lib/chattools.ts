// Skills and connectors in the chat: what the chat model is told about them and the tools it
// gets for them. The chat model has room for 8192 tokens, so it is told little: one line for a
// skill, one line for a connector, and never a connector's list of tools. It asks for what it
// needs: `use_skill` reads a skill, `find_tool` gives the three tools that fit a few words, and
// `call_tool` runs one after the person said yes.
//
// Plain functions and data, with no imports, so they can be tested with
// `node --test src/lib/chattools.test.mjs`.

/** A skill as the service lists it (`GET /v1/skills`). */
export interface SkillRow {
	name: string;
	description: string;
	enabled: boolean;
	where: 'any' | 'code' | 'chat';
}

/** A connector as the service lists it (`GET /v1/connectors`). */
export interface ConnectorRow {
	id: string;
	name: string;
	description: string;
	about: string;
	state: string;
	tools: number;
	always_allow: string[];
}

/** What a chat turn is offered beyond pictures, sound and video. */
export interface Extras {
	skills: SkillRow[];
	connectors: ConnectorRow[];
}

export const NO_EXTRAS: Extras = { skills: [], connectors: [] };

export const hasExtras = (extras: Extras) => extras.skills.length > 0 || extras.connectors.length > 0;

/** The most skills a chat is told about, and the room their lines may take. */
export const MAX_CHAT_SKILLS = 12;
export const SKILL_LINES_BUDGET = 1600;
const SKILL_LINE = 200;

const short = (text: string, max: number) => {
	const flat = text.replace(/\s+/g, ' ').trim();
	return [...flat].length <= max ? flat : `${[...flat].slice(0, max - 1).join('').trimEnd()}…`;
};

/**
 * The skills a chat is offered: the ones that are on and not only for Code. An agent with
 * pinned skills is offered those and no others. When there are more than fit, the ones with
 * the shortest descriptions stay.
 */
export function chatSkills(all: readonly SkillRow[], pinned: readonly string[] = [], max = MAX_CHAT_SKILLS, budget = SKILL_LINES_BUDGET): SkillRow[] {
	let usable = all.filter((s) => s.enabled && s.where !== 'code');
	if (pinned.length > 0) usable = usable.filter((s) => pinned.includes(s.name));
	const cost = (s: SkillRow) => s.name.length + Math.min([...s.description].length, SKILL_LINE) + 4;
	if (usable.length <= max && usable.reduce((sum, s) => sum + cost(s), 0) <= budget) return usable;
	const kept = new Set<string>();
	let used = 0;
	for (const s of [...usable].sort((a, b) => [...a.description].length - [...b.description].length)) {
		if (kept.size >= max || used + cost(s) > budget) break;
		kept.add(s.name);
		used += cost(s);
	}
	// In the order the service lists them, so the lines do not jump around between turns.
	return usable.filter((s) => kept.has(s.name));
}

/** The connectors a chat is offered: the running ones that have tools. */
export const chatConnectors = (all: readonly ConnectorRow[]) => all.filter((c) => c.state === 'running' && c.tools > 0);

/** What the chat model is told about the skills and connectors it has on this turn. */
export function extrasPrompt(extras: Extras): string {
	const parts: string[] = [];
	if (extras.skills.length > 0) {
		parts.push(
			'You can read skills: instructions for one kind of work. Only when the request clearly is that kind of work, call use_skill with the name first, then follow what it says. Otherwise answer as usual. The skills:\n' +
				extras.skills.map((s) => `- ${s.name}: ${short(s.description, SKILL_LINE)}`).join('\n')
		);
	}
	if (extras.connectors.length > 0) {
		parts.push(
			'Connectors give you tools from outside. To use one, call find_tool with a few words for what you need, then call_tool with the connector, the tool and its arguments. The user is asked before a tool runs. The connectors:\n' +
				extras.connectors
					.map((c) => `- ${c.id}${c.name.toLowerCase() === c.id ? '' : ` (${short(c.name, 60)})`}: ${short(c.description || c.about || `${c.tools} tools`, 120)}`)
					.join('\n')
		);
	}
	if (parts.length > 0) {
		const tools = [...(extras.skills.length > 0 ? ['use_skill'] : []), ...(extras.connectors.length > 0 ? ['find_tool', 'call_tool'] : [])];
		const named = tools.join(' and ').replace(' and find_tool and', ', find_tool and');
		parts.push(`What ${named} ${tools.length === 1 ? 'gives' : 'give'} back is for you only: the user does not see it. Use it to write your answer.`);
	}
	return parts.join('\n');
}

const text = (description: string) => ({ type: 'string', description });
const fn = (name: string, description: string, properties: Record<string, unknown>, required: string[]) => ({
	type: 'function',
	function: { name, description, parameters: { type: 'object', properties, required } }
});

const useSkillTool = fn('use_skill', 'Get the instructions of a skill by name.', { name: text('Name of the skill') }, ['name']);
const readSkillFileTool = fn(
	'read_skill_file',
	'Read one of the other files of a skill, by the path use_skill listed.',
	{ name: text('Name of the skill'), path: text('Path of the file inside the skill') },
	['name', 'path']
);
const findTool = fn('find_tool', 'Find tools of the connectors by a few words. Gives the three that fit best, with their arguments.', { query: text('What you need, in a few words, like "add a note"') }, ['query']);
const callTool = fn(
	'call_tool',
	'Run one tool of a connector. Use the names find_tool gave.',
	{ connector: text('Name of the connector'), tool: text('Name of the tool'), arguments: { type: 'object', description: "The tool's arguments, by name" } },
	['connector', 'tool']
);

export type ExtraToolName = 'use_skill' | 'read_skill_file' | 'find_tool' | 'call_tool';
const extraNames: readonly string[] = ['use_skill', 'read_skill_file', 'find_tool', 'call_tool'];
export const isExtraTool = (name: string): name is ExtraToolName => extraNames.includes(name);

/** The tools for the skills and connectors of this turn. */
export function extraTools(extras: Extras): unknown[] {
	const out: unknown[] = [];
	if (extras.skills.length > 0) out.push(useSkillTool, readSkillFileTool);
	if (extras.connectors.length > 0) out.push(findTool, callTool);
	return out;
}

/** The arguments of a call as an object. A small model sometimes writes them badly; then: nothing. */
export function callArguments(raw: string): Record<string, unknown> {
	try {
		const parsed = JSON.parse(raw || '{}');
		return parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : {};
	} catch {
		return {};
	}
}

/** The connector a model named, by its id or by its name. */
export function findConnector(connectors: readonly ConnectorRow[], wanted: unknown): ConnectorRow | null {
	if (typeof wanted !== 'string') return null;
	const name = wanted.trim().toLowerCase();
	return connectors.find((c) => c.id === name) ?? connectors.find((c) => c.name.toLowerCase() === name) ?? null;
}

/** The arguments of a tool call as the person is shown them before saying yes. */
export function shownArguments(value: unknown, max = 1200): string {
	let parsed = value;
	if (typeof value === 'string') {
		try {
			parsed = JSON.parse(value);
		} catch {
			parsed = value;
		}
	}
	const out = parsed === undefined || parsed === null ? '{}' : typeof parsed === 'string' ? parsed : JSON.stringify(parsed, null, 2);
	return out.length > max ? `${out.slice(0, max)}\n…` : out;
}

/**
 * The result of an earlier call as the model sees it on a later turn: a skill that was read is
 * only named, and other results are cut. The results of the turn being answered stay whole.
 */
export function olderResult(tool: string, args: string, result: string): string {
	if (tool === 'use_skill') {
		const name = callArguments(args).name;
		return result.startsWith('Skill ') ? `The skill ${typeof name === 'string' ? name : ''} was read here earlier. Call use_skill again if you need it.`.replace('  ', ' ') : short(result, 300);
	}
	return [...result].length > 400 ? `${[...result].slice(0, 400).join('')}\n[the rest was left out to save space]` : result;
}
