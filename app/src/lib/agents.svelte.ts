import * as api from './api';
import { checkAgent, copyName, isValid, newAgentId, tidy, type Agent } from './agents';
import { app } from './state.svelte';

/** What the form is working on: a new agent, or one that is being changed. */
export interface AgentDraft extends Agent {
	/** False for an agent that is not saved yet. */
	saved: boolean;
}

/**
 * The agents the person made, as the service keeps them (`/v1/agents`), and the sheet that
 * lists them. With an older service that cannot keep agents there are none and the app says so.
 */
class AgentsState {
	list = $state<Agent[]>([]);
	/** Whether agents can be kept: unknown until asked, 'no' with an older service. */
	support = $state<'unknown' | 'yes' | 'no'>('unknown');
	/** The sheet: closed, the list, or the form. */
	sheet = $state<null | 'list' | 'form'>(null);
	draft = $state<AgentDraft | null>(null);
	/** Why the last save or delete did not work. */
	problem = $state('');
	saving = $state(false);

	/** Called with a newly made agent, once, when the form was opened from a chat. */
	#then: ((agent: Agent) => void) | null = null;

	byId(id: string | undefined | null): Agent | null {
		return id ? (this.list.find((a) => a.id === id) ?? null) : null;
	}

	/** Reads the agents. Called when the app opens and after the service restarted. */
	async load() {
		try {
			const list = await api.agents();
			this.support = list === null ? 'no' : 'yes';
			if (list) this.list = list;
		} catch {
			// The service is away: the next load fills it in.
		}
	}

	open(from: string) {
		this.problem = '';
		this.draft = null;
		this.sheet = 'list';
		app.track('screen_viewed', { screen: 'agents', from });
		if (this.support !== 'yes') this.load();
	}

	close() {
		this.sheet = null;
		this.draft = null;
		this.#then = null;
	}

	/** Opens the form on an empty agent. `then` gets the agent once it is saved. */
	startNew(from: string, then?: (agent: Agent) => void) {
		this.problem = '';
		this.#then = then ?? null;
		this.draft = { id: '', name: '', instruction: '', tools: [], starter: '', connectors: false, skills: [], saved: false };
		if (this.sheet === null) app.track('screen_viewed', { screen: 'agents', from });
		this.sheet = 'form';
	}

	edit(agent: Agent) {
		this.problem = '';
		this.draft = { connectors: false, skills: [], ...$state.snapshot(agent), saved: true };
		this.sheet = 'form';
	}

	/** Back from the form to the list, without saving. */
	cancel() {
		this.draft = null;
		this.problem = '';
		this.#then = null;
		this.sheet = 'list';
	}

	/** Saves what is in the form. True when it is saved. */
	async save(): Promise<boolean> {
		const draft = this.draft;
		if (!draft || this.saving) return false;
		const agent = tidy({ ...draft, id: draft.saved ? draft.id : newAgentId(draft.name, this.list.map((a) => a.id)) });
		if (!isValid(checkAgent(agent))) return false;
		const stored = await this.#put(agent);
		if (!stored) return false;
		const then = this.#then;
		this.#then = null;
		this.draft = null;
		this.sheet = 'list';
		if (!draft.saved) then?.(stored);
		return true;
	}

	/**
	 * A copy of an agent or of an example, saved under a name of its own. An example keeps its
	 * name when no agent has it yet.
	 */
	async copy(source: Agent, example = false): Promise<Agent | null> {
		const names = this.list.map((a) => a.name);
		const name = example && !names.includes(source.name) ? source.name : copyName(source.name, names);
		const agent = tidy({ ...source, name, id: newAgentId(source.name, this.list.map((a) => a.id)) });
		return this.#put(agent);
	}

	async #put(agent: Agent): Promise<Agent | null> {
		this.saving = true;
		this.problem = '';
		try {
			const stored = await api.saveAgent(agent);
			const at = this.list.findIndex((a) => a.id === stored.id);
			if (at >= 0) this.list[at] = stored;
			else this.list.push(stored);
			this.support = 'yes';
			return stored;
		} catch (e) {
			this.problem = e instanceof api.ApiError && (e.status === 404 || e.status === 405) ? 'This version of the background service cannot keep agents yet.' : e instanceof Error ? e.message : String(e);
			return null;
		} finally {
			this.saving = false;
		}
	}

	/** Deletes an agent. Undo, for a few seconds, saves it again. */
	async remove(id: string) {
		const at = this.list.findIndex((a) => a.id === id);
		if (at < 0) return;
		const agent = $state.snapshot(this.list[at]);
		this.problem = '';
		try {
			await api.deleteAgent(id);
		} catch (e) {
			this.problem = e instanceof Error ? e.message : String(e);
			return;
		}
		this.list.splice(at, 1);
		app.say(`Deleted the agent “${agent.name}”.`, { label: 'Undo', run: () => void this.#put(agent) }, 6000);
	}
}

export const agents = new AgentsState();
