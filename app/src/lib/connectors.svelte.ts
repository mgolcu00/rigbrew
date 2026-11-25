import * as api from './api';
import type { Connector, ConnectorBody, ConnectorTool, ReadyConnector } from './api';
import { formArgs, skeleton, tryArguments } from './connectors';
import { app } from './state.svelte';

const say = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** One environment variable in the form. A saved one shows its name only; its value stays in the service. */
export interface EnvRow {
	name: string;
	value: string;
	saved: boolean;
}

/** What the form is working on. */
export interface ConnectorDraft {
	/** '' for a connector that is not saved yet. */
	id: string;
	name: string;
	description: string;
	kind: 'stdio' | 'http';
	command: string;
	/** One argument on each line. */
	args: string;
	/** What stands in for `<folder>` or `<file>` in the arguments. */
	place: string;
	env: EnvRow[];
	cwd: string;
	url: string;
	token: string;
	hasToken: boolean;
	alwaysAllow: string[];
	/** Set when the form was filled from a ready-made connector. */
	needs: 'node' | 'python' | null;
}

/** A tool being tried by the person. */
export interface Trial {
	tool: string;
	args: string;
	running: boolean;
	result: string;
	ok: boolean;
	problem: string;
}

/**
 * The connectors the person added, as the service keeps and runs them (`/v1/connectors`), and
 * the sheet that lists them. With an older service there are none and the app says so.
 */
class ConnectorsState {
	list = $state<Connector[]>([]);
	gallery = $state<ReadyConnector[]>([]);
	programs = $state({ npx: true, uvx: true });
	support = $state<'unknown' | 'yes' | 'no'>('unknown');
	/** The sheet: closed, the list, the form, or the tools of one connector. */
	sheet = $state<null | 'list' | 'form' | 'tools'>(null);
	draft = $state<ConnectorDraft | null>(null);
	problem = $state('');
	saving = $state(false);
	/** The connector being started or stopped right now. */
	busy = $state('');
	/** The connector whose tools are shown. */
	shown = $state<Connector | null>(null);
	tools = $state<ConnectorTool[]>([]);
	trial = $state<Trial | null>(null);

	/** The running connectors that have tools: what a model can be offered. */
	usable = $derived(this.list.filter((c) => c.state === 'running' && c.tools > 0));

	async load() {
		try {
			const answer = await api.connectors();
			this.support = answer === null ? 'no' : 'yes';
			if (answer) {
				this.list = answer.connectors;
				this.gallery = answer.gallery;
				this.programs = answer.programs;
			}
		} catch {
			// The service is away: the next load fills it in.
		}
	}

	open(from: string) {
		this.problem = '';
		this.draft = null;
		this.sheet = 'list';
		app.track('screen_viewed', { screen: 'connectors', from });
		void this.load();
	}

	close() {
		this.sheet = null;
		this.draft = null;
		this.shown = null;
		this.trial = null;
	}

	back() {
		this.draft = null;
		this.shown = null;
		this.trial = null;
		this.problem = '';
		this.sheet = 'list';
	}

	/** Opens the form, empty or filled from a ready-made connector. Nothing is saved or run yet. */
	startNew(ready?: ReadyConnector) {
		this.problem = '';
		this.draft = {
			id: '',
			name: ready?.name ?? '',
			description: ready?.does ?? '',
			kind: 'stdio',
			command: ready?.command ?? '',
			args: (ready?.args ?? []).join('\n'),
			place: '',
			env: [],
			cwd: '',
			url: '',
			token: '',
			hasToken: false,
			alwaysAllow: [],
			needs: ready?.needs ?? null
		};
		this.sheet = 'form';
		if (ready) app.track('connector_gallery_picked', { entry: ready.id });
	}

	edit(c: Connector) {
		this.problem = '';
		this.draft = {
			id: c.id,
			name: c.name,
			description: c.description,
			kind: c.kind,
			command: c.command,
			args: c.args.join('\n'),
			place: '',
			env: c.env.map((name) => ({ name, value: '', saved: true })),
			cwd: c.cwd,
			url: c.url,
			token: '',
			hasToken: c.has_token,
			alwaysAllow: [...c.always_allow],
			needs: null
		};
		this.sheet = 'form';
	}

	#body(draft: ConnectorDraft): ConnectorBody {
		const base = { name: draft.name.trim(), description: draft.description.trim(), kind: draft.kind, always_allow: draft.alwaysAllow };
		if (draft.kind === 'http') {
			// An empty token field keeps the saved token.
			return { ...base, url: draft.url.trim(), ...(draft.token.trim() || !draft.hasToken ? { token: draft.token.trim() } : {}) };
		}
		const env: Record<string, string | null> = {};
		for (const row of draft.env) {
			if (!row.name.trim()) continue;
			// A saved variable left empty keeps its value.
			env[row.name.trim()] = row.saved && row.value === '' ? null : row.value;
		}
		return { ...base, command: draft.command.trim(), args: formArgs(draft.args, draft.place), env, cwd: draft.cwd.trim() };
	}

	#put(c: Connector) {
		const at = this.list.findIndex((x) => x.id === c.id);
		if (at >= 0) this.list[at] = c;
		else this.list.push(c);
		if (this.shown?.id === c.id) this.shown = c;
	}

	/** Saves what is in the form, and starts it when asked. True when it is saved. */
	async save(start: boolean): Promise<boolean> {
		const draft = this.draft;
		if (!draft || this.saving) return false;
		this.saving = true;
		this.problem = '';
		try {
			const body = this.#body(draft);
			const stored = draft.id ? await api.saveConnector(draft.id, body) : await api.addConnector(body);
			this.#put(stored);
			this.support = 'yes';
			this.draft = null;
			this.sheet = 'list';
			if (start) void this.start(stored.id);
			return true;
		} catch (e) {
			this.problem = say(e);
			return false;
		} finally {
			this.saving = false;
		}
	}

	async start(id: string) {
		if (this.busy) return;
		this.busy = id;
		this.problem = '';
		try {
			this.#put(await api.startConnector(id));
		} catch (e) {
			this.problem = say(e);
		} finally {
			this.busy = '';
		}
	}

	async stop(id: string) {
		if (this.busy) return;
		this.busy = id;
		this.problem = '';
		try {
			this.#put(await api.stopConnector(id));
		} catch (e) {
			this.problem = say(e);
		} finally {
			this.busy = '';
		}
	}

	async remove(id: string) {
		const c = this.list.find((x) => x.id === id);
		if (!c) return;
		this.problem = '';
		try {
			await api.deleteConnector(id);
			this.list = this.list.filter((x) => x.id !== id);
			app.say(`Removed the connector “${c.name}”.`);
		} catch (e) {
			this.problem = say(e);
		}
	}

	async openTools(c: Connector) {
		this.problem = '';
		this.trial = null;
		this.shown = c;
		this.tools = [];
		this.sheet = 'tools';
		try {
			this.tools = await api.connectorTools(c.id);
		} catch (e) {
			this.problem = say(e);
		}
	}

	/** Marks a tool as one that runs without asking, or takes the mark away. */
	async setAlways(id: string, tool: string, always: boolean) {
		try {
			this.#put(await api.allowConnectorTool(id, tool, always));
			const shown = this.tools.find((t) => t.name === tool);
			if (shown && this.shown?.id === id) shown.always = always;
		} catch (e) {
			this.problem = say(e);
		}
	}

	startTrial(tool: ConnectorTool) {
		this.trial = this.trial?.tool === tool.name ? null : { tool: tool.name, args: skeleton(tool.schema), running: false, result: '', ok: true, problem: '' };
	}

	/** Runs the tool being tried with the arguments typed. The person pressed the button: nothing else is asked. */
	async runTrial() {
		const trial = this.trial;
		const c = this.shown;
		if (!trial || !c || trial.running) return;
		const args = tryArguments(trial.args);
		if ('problem' in args) {
			trial.problem = args.problem;
			return;
		}
		trial.problem = '';
		trial.running = true;
		try {
			const answer = await api.callConnector(c.id, trial.tool, args.value);
			trial.result = answer.text;
			trial.ok = answer.ok;
		} catch (e) {
			trial.result = say(e);
			trial.ok = false;
		} finally {
			trial.running = false;
		}
	}
}

export const connectors = new ConnectorsState();
