// Studio's assistant: the conversation about the film that is open. The work runs in the
// service (the same loop as Code, with the film tools), so this is only a view of it and the
// person's answers. Each conversation belongs to its film and is there again after a restart:
// opening a film shows its newest one. While a request is worked on, the workstation follows
// every change the assistant makes, and the whole request is one step of undo (see
// `studio.assistantBegin`).

import * as api from './api';
import type { AgentMode, AgentTask } from './api';
import { changesDone, filmModes, heaviest } from './assistant-view';
import { remembered } from './remember';
import { app } from './state.svelte';
import { studio } from './studio.svelte';

const active = (t: { state: string } | null | undefined) => t?.state === 'running' || t?.state === 'waiting';

class Assistant {
	/** The film whose conversation is shown. */
	film = $state('');
	/** Its newest conversation; null before the first request. */
	task = $state<AgentTask | null>(null);
	/** What the model has written of its answer so far. */
	writing = $state('');
	problem = $state<unknown>(null);
	busy = $state(false);
	mode = $state<AgentMode>(filmModes.some((m) => m.id === remembered.get('studio.assistant.mode')) ? (remembered.get('studio.assistant.mode') as AgentMode) : 'ask');
	/** The model chosen in the menu; without one, the heaviest installed chat model (as in Code). */
	chosen = $state(remembered.get('studio.assistant.model') ?? '');
	model = $derived(app.chatPackages.find((p) => p.id === this.chosen) ?? heaviest(app.chatPackages));
	working = $derived(active(this.task));
	/** Esc was pressed once while it works: the next one stops it. */
	stopping = $state(false);

	/** Film changes of the open conversation already taken into the workstation. */
	#seen = 0;
	#watching = false;
	#forget: ReturnType<typeof setTimeout> | undefined;

	setMode(mode: AgentMode) {
		this.mode = mode;
		remembered.set('studio.assistant.mode', mode);
	}

	chooseModel(id: string) {
		this.chosen = id;
		remembered.set('studio.assistant.model', id);
	}

	/** The film open in Studio: its newest conversation is shown, and followed when it still works. */
	async show(film: string) {
		if (film === this.film) return;
		this.film = film;
		this.task = null;
		this.writing = '';
		this.problem = null;
		this.stopping = false;
		if (!film) return;
		try {
			const newest = (await api.filmTasks(film))[0];
			if (this.film !== film || !newest) return;
			const task = await api.agentTask(newest.id);
			if (this.film !== film) return;
			this.#seen = changesDone(task.steps);
			this.#show(task);
			if (active(task)) {
				await studio.assistantBegin(film);
				this.#watch();
			}
		} catch (e) {
			if (this.film === film) this.problem = e;
		}
	}

	/** A new conversation about the same film; the one before stays with the service. */
	fresh() {
		if (this.working) return;
		this.task = null;
		this.writing = '';
		this.problem = null;
	}

	/** Sends a request, with the shots selected in the tracks. Returns whether it was taken. */
	async send(text: string, selection: string[]): Promise<boolean> {
		const prompt = text.trim();
		const film = this.film;
		if (!prompt || !film || this.busy || this.working) return false;
		this.busy = true;
		this.problem = null;
		try {
			await studio.assistantBegin(film);
			const task = this.task
				? await api.tellAgentTask(this.task.id, { text: prompt, mode: this.mode, web: false, selection })
				: await api.startFilmTask({ film, prompt, mode: this.mode, selection, ...(this.model ? { model: this.model.id } : {}) });
			if (this.film !== film) return true;
			this.#seen = changesDone(task.steps);
			this.#show(task);
			this.#watch();
			app.track('studio_assistant_asked', { follow_up: task.steps.filter((s) => s.kind === 'user').length > 1, selected: selection.length, mode: this.mode });
			return true;
		} catch (e) {
			this.problem = e;
			await studio.assistantEnd(film);
			return false;
		} finally {
			this.busy = false;
		}
	}

	/** The answer to a step that waits: making shots, a dub, an export. */
	async answer(allow: boolean, always = false) {
		const task = this.task;
		if (!task?.pending || this.busy) return;
		this.busy = true;
		try {
			this.#show(await api.answerAgentTask(task.id, task.pending.call, allow, always));
		} catch (e) {
			this.problem = e;
		} finally {
			this.busy = false;
		}
	}

	async stop() {
		this.stopping = false;
		clearTimeout(this.#forget);
		if (!this.task || !this.working) return;
		try {
			this.#show(await api.stopAgentTask(this.task.id));
		} catch (e) {
			this.problem = e;
		}
	}

	/** Esc while it works: the first press says to press again, the second stops it. */
	escape(): boolean {
		if (!this.working) return false;
		clearTimeout(this.#forget);
		if (this.stopping) void this.stop();
		else {
			this.stopping = true;
			this.#forget = setTimeout(() => (this.stopping = false), 3000);
		}
		return true;
	}

	/** The task as the service has it: the workstation follows the changes, and a request that ended is one step of undo. */
	#show(task: AgentTask) {
		this.task = task;
		this.writing = active(task) ? (task.writing ?? '') : '';
		if (!active(task)) this.stopping = false;
		const film = task.film || this.film;
		const done = changesDone(task.steps);
		if (done !== this.#seen) {
			this.#seen = done;
			void studio.assistantFollow(film);
		}
		if (!active(task)) void studio.assistantEnd(film);
	}

	async #tick() {
		const open = this.task;
		if (!open || !active(open)) return;
		try {
			const brief = await api.agentTaskBrief(open.id);
			if (this.task?.id !== open.id) return;
			if (brief.updated_ms !== open.updated_ms || brief.state !== open.state) {
				const fresh = await api.agentTask(open.id);
				if (this.task?.id === open.id) this.#show(fresh);
			} else this.writing = brief.writing ?? '';
		} catch {
			// The service is away for a moment; the next round asks again.
		}
	}

	/** Asks several times a second while a request is worked on. */
	async #watch() {
		if (this.#watching) return;
		this.#watching = true;
		while (active(this.task)) {
			await this.#tick();
			await new Promise((r) => setTimeout(r, 250));
		}
		this.#watching = false;
	}
}

export const assistant = new Assistant();
