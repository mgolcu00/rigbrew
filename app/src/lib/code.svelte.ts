// Code: a model on this computer works on a folder. The work itself runs in the service, so
// this is only a view of it: which folder, its tasks, the one that is open, and the answers
// the person gives. Leaving the screen does not stop a task; the list keeps being asked for
// while something is working, so the sidebar can say when a task waits for an answer.

import * as api from './api';
import type { AgentMode, AgentSkill, AgentTask, AgentTaskRow } from './api';
import { remembered } from './remember';

const say = (e: unknown) => (e instanceof Error ? e.message : String(e));
const active = (t: { state: string } | null | undefined) => t?.state === 'running' || t?.state === 'waiting';

export const modes: { id: AgentMode; name: string; does: string }[] = [
	{ id: 'ask', name: 'Ask first', does: 'Asks you before it changes a file or runs a command.' },
	{ id: 'read_only', name: 'Read only', does: 'Only reads. It cannot change files or run commands.' },
	{ id: 'auto_edits', name: 'Auto edits', does: 'Changes files in the folder without asking. Still asks before it runs a command.' }
];

class Code {
	/** The folder being worked on, as the person chose it. Empty until they do. */
	folder = $state(remembered.get('code.folder') ?? '');
	mode = $state<AgentMode>(modes.some((m) => m.id === remembered.get('code.mode')) ? (remembered.get('code.mode') as AgentMode) : 'ask');
	/** "Allow web search": off until the person switches it on. */
	web = $state(remembered.get('code.web') === 'on');
	/** Every task the service knows, newest first. */
	all = $state<AgentTaskRow[]>([]);
	/** The open task, or null on the folder's start page. */
	task = $state<AgentTask | null>(null);
	skills = $state<AgentSkill[]>([]);
	skillsDir = $state('');
	problem = $state('');
	/** False when the service is too old to have Code. */
	available = $state(true);
	busy = $state(false);

	#watching = false;
	#visible = false;

	/** The tasks of the chosen folder (the service knows which folder is which, links followed). */
	tasks = $state<AgentTaskRow[]>([]);
	/** How many tasks, in any folder, wait for an answer: shown next to Code in the sidebar. */
	waiting = $derived(this.all.filter((t) => t.state === 'waiting').length);
	working = $derived(this.all.some((t) => active(t)));

	setMode(mode: AgentMode) {
		this.mode = mode;
		remembered.set('code.mode', mode);
	}

	setWeb(on: boolean) {
		this.web = on;
		remembered.set('code.web', on ? 'on' : 'off');
	}

	async setFolder(path: string) {
		const folder = path.trim().replace(/(.)[/\\]+$/, '$1');
		if (!folder) return;
		this.folder = folder;
		this.task = null;
		this.problem = '';
		remembered.set('code.folder', folder);
		await this.refresh();
	}

	async choose() {
		try {
			const picked = await api.pickFolder();
			if (picked) await this.setFolder(picked);
		} catch (e) {
			this.problem = say(e);
		}
	}

	/** The screen is shown: load what it needs and keep it fresh until `leave`. */
	enter() {
		this.#visible = true;
		this.refresh();
		this.#watch();
	}

	leave() {
		this.#visible = false;
	}

	/** When the window opens: follow what is already working in the service. */
	async wake() {
		await this.refresh();
		this.#watch();
	}

	async refresh() {
		try {
			const rows = await api.agentTasks();
			this.available = rows !== null;
			this.all = rows ?? [];
			this.tasks = this.folder ? ((await api.agentTasks(this.folder)) ?? []) : [];
			if (this.folder) {
				const found = await api.agentSkills(this.folder);
				this.skills = found?.skills ?? [];
				this.skillsDir = found?.dir ?? '';
			}
		} catch (e) {
			this.problem = say(e);
		}
	}

	async #tick() {
		if (this.task && (active(this.task) || this.all.find((t) => t.id === this.task?.id)?.updated_ms !== this.task.updated_ms)) {
			const id = this.task.id;
			try {
				const fresh = await api.agentTask(id);
				if (this.task?.id === id) this.task = fresh;
			} catch {
				// The service is away for a moment; the next round asks again.
			}
		}
		try {
			this.all = (await api.agentTasks()) ?? this.all;
			if (this.#visible && this.folder) this.tasks = (await api.agentTasks(this.folder)) ?? this.tasks;
		} catch {
			// as above
		}
	}

	/** Asks again and again while the screen is shown or something is working. */
	async #watch() {
		if (this.#watching) return;
		this.#watching = true;
		while (this.#visible || this.working || active(this.task)) {
			await this.#tick();
			// Quick while the open task is working in front of the person, slow otherwise.
			const quick = this.#visible && active(this.task);
			await new Promise((r) => setTimeout(r, quick ? 700 : this.#visible ? 2500 : 5000));
		}
		this.#watching = false;
	}

	async open(id: string) {
		this.problem = '';
		try {
			this.task = await api.agentTask(id);
		} catch (e) {
			this.problem = say(e);
		}
	}

	close() {
		this.task = null;
		this.problem = '';
	}

	/** Starts a task, or continues the open one. Returns whether it was accepted. */
	async send(text: string, model?: string): Promise<boolean> {
		const prompt = text.trim();
		if (!prompt || !this.folder || this.busy) return false;
		this.busy = true;
		this.problem = '';
		try {
			this.task = this.task
				? await api.tellAgentTask(this.task.id, { text: prompt, mode: this.mode, web: this.web })
				: await api.startAgentTask({ folder: this.folder, prompt, mode: this.mode, web: this.web, ...(model ? { model } : {}) });
			this.#watch();
			return true;
		} catch (e) {
			this.problem = say(e);
			return false;
		} finally {
			this.busy = false;
		}
	}

	async answer(allow: boolean, always = false) {
		const task = this.task;
		if (!task?.pending || this.busy) return;
		this.busy = true;
		try {
			this.task = await api.answerAgentTask(task.id, task.pending.call, allow, always);
		} catch (e) {
			this.problem = say(e);
			await this.open(task.id);
		} finally {
			this.busy = false;
		}
	}

	async stop() {
		if (!this.task) return;
		try {
			this.task = await api.stopAgentTask(this.task.id);
		} catch (e) {
			this.problem = say(e);
		}
	}

	/** Takes back what the newest request changed in the folder. */
	async undo() {
		const task = this.task;
		if (!task || this.busy) return;
		this.busy = true;
		this.problem = '';
		try {
			this.task = await api.undoAgentTask(task.id);
		} catch (e) {
			this.problem = say(e);
		} finally {
			this.busy = false;
		}
	}

	async remove(id: string) {
		try {
			await api.removeAgentTask(id);
			if (this.task?.id === id) this.task = null;
			this.all = this.all.filter((t) => t.id !== id);
			this.tasks = this.tasks.filter((t) => t.id !== id);
		} catch (e) {
			this.problem = say(e);
		}
	}
}

export const isActive = active;
export const code = new Code();
