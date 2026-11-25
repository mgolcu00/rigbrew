// Connectors, the parts that are plain functions: the command a connector will run as the
// person is shown it before saving, the form's arguments, and the first arguments for trying
// a tool. The service checks everything again (daemon/src/connectors.rs).
//
// No imports, so it can be tested with `node --test src/lib/connectors.test.mjs`.

/** Where a ready-made connector wants a folder or a file of the person's choice. */
export const PLACEHOLDERS = ['<folder>', '<file>'] as const;

/** One argument as a shell would need it written. For showing only: nothing is run through a shell. */
export function quoteArg(arg: string): string {
	return arg !== '' && /^[A-Za-z0-9\-_./:=@,+%]+$/.test(arg) ? arg : `'${arg.replace(/'/g, `'\\''`)}'`;
}

/** The command line exactly as it will be run. */
export const commandLine = (command: string, args: readonly string[]) => [command.trim(), ...args].filter((part, i) => i > 0 || part).map(quoteArg).join(' ');

/** The arguments of the form: one on each line, empty lines left out, the placeholder replaced by what the person chose. */
export function formArgs(text: string, place = ''): string[] {
	return text
		.split('\n')
		.map((line) => line.trim())
		.filter(Boolean)
		.map((arg) => ((PLACEHOLDERS as readonly string[]).includes(arg) && place.trim() ? place.trim() : arg));
}

/** Which placeholder the arguments still hold, if any. */
export function placeholderIn(text: string): (typeof PLACEHOLDERS)[number] | null {
	const lines = text.split('\n').map((l) => l.trim());
	return PLACEHOLDERS.find((p) => lines.includes(p)) ?? null;
}

/** What a ready-made connector needs, and whether it is there, in a sentence or two. */
export function needsLine(needs: 'node' | 'python', programs: { npx: boolean; uvx: boolean }, machine = 'computer'): { text: string; missing: boolean } {
	if (needs === 'node') {
		return programs.npx
			? { text: `Needs Node.js on this ${machine}. It is installed.`, missing: false }
			: { text: `Needs Node.js on this ${machine}. It was not found: install Node.js from nodejs.org first.`, missing: true };
	}
	return programs.uvx
		? { text: `Needs Python with uv on this ${machine}. It is installed.`, missing: false }
		: { text: `Needs Python with uv on this ${machine}. The uvx program was not found: install uv from docs.astral.sh/uv first.`, missing: true };
}

/** First arguments for trying a tool: its needed arguments with empty values of the right kind. */
export function skeleton(schema: { properties?: Record<string, { type?: string | string[] }>; required?: string[] } | null | undefined): string {
	const out: Record<string, unknown> = {};
	const props = schema?.properties ?? {};
	const wanted = schema?.required?.length ? schema.required : Object.keys(props).slice(0, 3);
	for (const key of wanted) {
		const kind = Array.isArray(props[key]?.type) ? props[key]?.type?.[0] : props[key]?.type;
		out[key] = kind === 'number' || kind === 'integer' ? 0 : kind === 'boolean' ? false : kind === 'array' ? [] : kind === 'object' ? {} : '';
	}
	return JSON.stringify(out, null, 2);
}

/** The arguments typed for a try, as an object; a sentence when they are not one. */
export function tryArguments(text: string): { value: Record<string, unknown> } | { problem: string } {
	if (!text.trim()) return { value: {} };
	try {
		const parsed = JSON.parse(text);
		if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) return { value: parsed };
	} catch {
		// said below
	}
	return { problem: 'The arguments must be JSON in curly brackets, like {"name": "value"}.' };
}

/** The state of a connector in a few words. */
export function stateWords(c: { state: string; tools: number; error: string | null }): string {
	if (c.state === 'running') return c.tools === 1 ? 'Running, 1 tool' : `Running, ${c.tools} tools`;
	if (c.state === 'starting') return 'Starting';
	if (c.state === 'failed') return `Did not start. ${c.error ?? ''}`.trim();
	return 'Stopped';
}
