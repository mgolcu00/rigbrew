// Agents the person makes: a saved way to use the chat model. A name, an instruction in plain
// text, the abilities it may use, and a first message to suggest. The service keeps them
// (`/v1/agents`, see daemon/src/agents.rs, which checks the same limits).
//
// Plain functions and data, with no imports, so they can be tested with
// `node --test src/lib/agents.test.mjs`.

/** The chat's tools. An agent is given some of them, or none. */
export type ToolName = 'generate_image' | 'read_aloud' | 'generate_video';

export interface Agent {
	/** 1 to 40 of a-z, 0-9 and '-'. */
	id: string;
	name: string;
	/** What the chat model is told, on top of who it is. */
	instruction: string;
	tools: ToolName[];
	/** A first message to suggest in a new chat; may be empty. */
	starter: string;
	/** "May use connectors": off unless the person switched it on. */
	connectors?: boolean;
	/** Names of the skills pinned to it. With none pinned the chat is offered every skill that is on. */
	skills?: string[];
	created_ms?: number;
	updated_ms?: number;
}

export const MAX_NAME = 60;
export const MAX_INSTRUCTION = 4000;
export const MAX_STARTER = 300;
export const MAX_ID = 40;
export const MAX_PINNED_SKILLS = 12;

/** The abilities, in the words the form uses. */
export const abilities: { tool: ToolName; label: string; does: string }[] = [
	{ tool: 'generate_image', label: 'Make pictures', does: 'It may ask the picture model for an image.' },
	{ tool: 'read_aloud', label: 'Read aloud', does: 'It may speak a text with the voice model.' },
	{ tool: 'generate_video', label: 'Make videos', does: 'It may ask the video model for a short clip.' }
];

const known = abilities.map((a) => a.tool);

/** Three to copy and change. They are not saved until copied. */
export const examples: (Agent & { does: string })[] = [
	{
		id: 'editor',
		name: 'Editor',
		does: 'Tightens a text and keeps its meaning.',
		instruction:
			'You are an editor. Tighten the text the user gives you: cut filler, fix grammar and spelling, and keep the meaning, the facts and the voice of the author. Do not add new ideas. ' +
			'Answer with the edited text first, then one short line that says what you changed. If a passage is unclear, leave it as it is and say so in that line.',
		tools: [],
		starter: 'Tighten this text:'
	},
	{
		id: 'tutor',
		name: 'Tutor',
		does: 'Explains step by step and asks one question back.',
		instruction:
			'You are a patient tutor. Explain the topic step by step in plain words, one idea in each step, with a small example where it helps. Keep each answer short. ' +
			'End every answer with exactly one question that checks whether the user understood, and wait for the reply before you go on.',
		tools: [],
		starter: 'Teach me how'
	},
	{
		id: 'art-director',
		name: 'Art director',
		does: 'Turns an idea into a picture prompt and makes the picture.',
		instruction:
			'You are an art director. When the user gives you an idea, turn it into one clear picture prompt in English that names the subject, the setting, the style, the light and the framing. ' +
			'Then call generate_image with that prompt. When the picture is shown, say in one sentence what you chose and offer one way to change it.',
		tools: ['generate_image'],
		starter: 'A poster for'
	}
];

/** Length in characters, counted the way the service counts them. */
export const chars = (text: string) => [...text].length;

export const validId = (id: string) => /^[a-z0-9-]{1,40}$/.test(id);

export interface AgentProblems {
	id?: string;
	name?: string;
	instruction?: string;
	starter?: string;
	tools?: string;
}

/** What is wrong with an agent, field by field, in a sentence each. Nothing wrong: an empty object. */
export function checkAgent(agent: { id: string; name: string; instruction: string; starter?: string; tools?: readonly string[] }): AgentProblems {
	const problems: AgentProblems = {};
	if (!validId(agent.id)) problems.id = `An agent id is 1 to ${MAX_ID} lowercase letters, digits or dashes.`;
	const name = agent.name.trim();
	if (!name) problems.name = 'Give it a name.';
	else if (chars(name) > MAX_NAME) problems.name = `The name can be ${MAX_NAME} characters at most. This one has ${chars(name)}.`;
	const instruction = agent.instruction.trim();
	if (!instruction) problems.instruction = 'Say what it should do.';
	else if (chars(instruction) > MAX_INSTRUCTION) problems.instruction = `The instruction can be ${MAX_INSTRUCTION} characters at most. This one has ${chars(instruction)}.`;
	if (chars((agent.starter ?? '').trim()) > MAX_STARTER) problems.starter = `The first message can be ${MAX_STARTER} characters at most.`;
	if ((agent.tools ?? []).some((t) => !known.includes(t as ToolName))) problems.tools = 'One of the abilities is not known.';
	return problems;
}

export const isValid = (problems: AgentProblems) => Object.keys(problems).length === 0;

/** The agent as it is saved: trimmed, each ability once, in the usual order. */
export function tidy(agent: Agent): Agent {
	return {
		id: agent.id,
		name: agent.name.trim(),
		instruction: agent.instruction.trim(),
		tools: known.filter((t) => agent.tools.includes(t)),
		starter: (agent.starter ?? '').trim(),
		connectors: agent.connectors === true,
		skills: [...new Set(agent.skills ?? [])].slice(0, MAX_PINNED_SKILLS)
	};
}

/**
 * An id for a new agent: its name in lower case with dashes, and a few random letters so two
 * agents with one name stay apart. Never one that is taken.
 */
export function newAgentId(name: string, taken: readonly string[] = [], random: () => number = Math.random): string {
	const stem =
		name
			.normalize('NFKD')
			.replace(/[̀-ͯ]/g, '')
			.toLowerCase()
			.replace(/ı/g, 'i')
			.replace(/[^a-z0-9]+/g, '-')
			.replace(/^-+|-+$/g, '')
			.slice(0, 28)
			.replace(/-+$/, '') || 'agent';
	for (;;) {
		const tail = Array.from({ length: 5 }, () => 'abcdefghijklmnopqrstuvwxyz0123456789'[Math.floor(random() * 36) % 36]).join('');
		const id = `${stem}-${tail}`;
		if (!taken.includes(id)) return id;
	}
}

/** A name for a copy that is not taken: "Editor copy", "Editor copy 2". */
export function copyName(name: string, taken: readonly string[]): string {
	const base = [...`${name.trim()} copy`].slice(0, MAX_NAME - 4).join('');
	if (!taken.includes(base)) return base;
	for (let n = 2; ; n++) if (!taken.includes(`${base} ${n}`)) return `${base} ${n}`;
}

/** The suggested first message as it goes into the box: an unfinished one gets a space to type on from. */
export const starterDraft = (starter: string) => (/[.?!…]$/.test(starter.trim()) ? starter.trim() : `${starter.trim()} `);

/** Of the tools that can be used right now, the ones this agent may use. Without an agent: all of them. */
export function allowedTools(agent: Pick<Agent, 'tools'> | null | undefined, available: readonly ToolName[]): ToolName[] {
	return agent ? available.filter((t) => agent.tools.includes(t)) : [...available];
}

/** What the chat model is told about the agent it is running as, after the line that says who it is. */
export function agentPrompt(agent: Pick<Agent, 'name' | 'instruction'>): string {
	return `In this chat you work as "${agent.name.trim()}", a role the user set up. Follow these instructions from the user: ${agent.instruction.trim()}`;
}
