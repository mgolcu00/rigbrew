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
	{ id: 'auto_edits', name: 'Auto edits', does: 'Changes files in the folder without asking. Still asks before it runs a command.' },
	{ id: 'bypass', name: 'Bypass permissions', does: 'Never asks. It changes files and runs commands on its own.' }
];

const MAX_RECENT = 8;

/** What was remembered, with the folder that is open first when the list does not have it yet. */
function recentFolders(): string[] {
	const kept = remembered.json<unknown>('code.folders', []);
	const list = Array.isArray(kept) ? kept.filter((f): f is string => typeof f === 'string' && f.trim() !== '') : [];
	const open = remembered.get('code.folder');
	return (open && !list.includes(open) ? [open, ...list] : list).slice(0, MAX_RECENT);
}

/** What a command is for, said only when the command line leaves no doubt. */
export function purposeOf(command: string): string {
	const line = command.trim();
	// Two commands in one line, a pipe or a redirect: no guessing.
	if (/[;&|<>`\n]|\$\(/.test(line)) return '';
	const rules: [RegExp, string][] = [
		[/^(cargo|go) test\b/, 'This runs the tests of the project.'],
		[/^(npm|pnpm|yarn|bun) (run )?test\b/, 'This runs the tests of the project.'],
		[/^(python3?|py) -m (pytest|unittest)\b/, 'This runs the tests of the project.'],
		[/^(pytest|jest|vitest( run)?|swift test|make test|\.\/gradlew test|gradle test|mvn test)( |$)/, 'This runs the tests of the project.'],
		[/^(cargo (build|check|clippy)|go (build|vet)|swift build|tsc)( |$)/, 'This builds or checks the project.'],
		[/^make( (build|check|all))?$/, 'This builds or checks the project.'],
		[/^(npm|pnpm|yarn|bun) (run )?(build|check|lint|typecheck)( |$)/, 'This builds or checks the project.'],
		[/^git (status|diff|log|show|branch)( |$)/, 'This only looks at the history and the state of the files.'],
		[/^(ls|pwd|cat|head|tail|wc|grep|rg|find|tree|which|file)( |$)/, 'This only reads.'],
		[/^(npm|pnpm|yarn|bun) (install|i|add)( |$)/, 'This downloads and installs packages.'],
		[/^(pip3?|python3? -m pip|uv pip) install( |$)/, 'This downloads and installs packages.'],
		[/^(rm|rmdir) /, 'This deletes files.']
	];
	// `find` and `git branch` can also change things; then nothing is said.
	if (/^find .*-(delete|exec)/.test(line) || /^git branch .*-[dDmM]\b/.test(line)) return '';
	return rules.find(([pattern]) => pattern.test(line))?.[1] ?? '';
}

class Code {
	/** The folder being worked on, as the person chose it. Empty until they do. */
	folder = $state(remembered.get('code.folder') ?? '');
	mode = $state<AgentMode>(modes.some((m) => m.id === remembered.get('code.mode')) ? (remembered.get('code.mode') as AgentMode) : 'ask');
	/** The folders worked on lately, newest first, at most eight. */
	recent = $state<string[]>(recentFolders());
	/** "Allow web search": off until the person switches it on. */
	web = $state(remembered.get('code.web') === 'on');
	/** "Use connectors": off until the person switches it on. */
	connectors = $state(remembered.get('code.connectors') === 'on');
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

	/** What the model has written of its answer so far, while the open task works. */
	writing = $state('');
	/** The preview next to a task (its running app, the phone's screen) is shown. */
	preview = $state(remembered.get('code.preview') !== 'off');
	/** A look at the page or the phone, asked for from the preview, is under way. */
	looking = $state(false);

	#watching = false;
	#visible = false;
	#listedAt = 0;
	/** The lists are asked for a little longer after the work ends: a task's name may still arrive. */
	#lingerUntil = 0;

	/** The tasks of the chosen folder (the service knows which folder is which, links followed). */
	tasks = $state<AgentTaskRow[]>([]);
	/** How many tasks, in any folder, wait for an answer: shown next to Code in the sidebar. */
	waiting = $derived(this.all.filter((t) => t.state === 'waiting' || t.state === 'asking').length);
	working = $derived(this.all.some((t) => active(t)));

	setMode(mode: AgentMode) {
		this.mode = mode;
		remembered.set('code.mode', mode);
	}

	setWeb(on: boolean) {
		this.web = on;
		remembered.set('code.web', on ? 'on' : 'off');
	}

	setPreview(on: boolean) {
		this.preview = on;
		remembered.set('code.preview', on ? 'on' : 'off');
	}

	setConnectors(on: boolean) {
		this.connectors = on;
		remembered.set('code.connectors', on ? 'on' : 'off');
	}

	async setFolder(path: string) {
		const folder = path.trim().replace(/(.)[/\\]+$/, '$1');
		if (!folder) return;
		this.folder = folder;
		this.task = null;
		this.writing = '';
		this.problem = '';
		remembered.set('code.folder', folder);
		this.recent = [folder, ...this.recent.filter((f) => f !== folder)].slice(0, MAX_RECENT);
		remembered.setJson('code.folders', this.recent);
		await this.refresh();
	}

	/** Takes a folder off the list of recent ones. Nothing on disk changes. */
	forget(folder: string) {
		this.recent = this.recent.filter((f) => f !== folder);
		remembered.setJson('code.folders', this.recent);
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
		const open = this.task;
		if (open && active(open)) {
			// While it works, a small question several times a second: has anything changed, and
			// what has the model written so far. The whole task is asked for only when it changed.
			try {
				const brief = await api.agentTaskBrief(open.id);
				if (this.task?.id !== open.id) return;
				if (brief.updated_ms !== open.updated_ms || brief.state !== open.state) {
					const fresh = await api.agentTask(open.id);
					if (this.task?.id === open.id) this.#show(fresh);
				} else {
					this.writing = brief.writing ?? '';
				}
			} catch {
				// The service is away for a moment; the next round asks again.
			}
		} else if (open && this.all.find((t) => t.id === open.id)?.updated_ms !== open.updated_ms) {
			try {
				const fresh = await api.agentTask(open.id);
				if (this.task?.id === open.id) this.#show(fresh);
			} catch {
				// as above
			}
		}
		if (this.working || active(this.task)) this.#lingerUntil = Date.now() + 30_000;
		// The lists change slowly; once a second is enough for them.
		if (Date.now() - this.#listedAt < 1000) return;
		this.#listedAt = Date.now();
		try {
			this.all = (await api.agentTasks()) ?? this.all;
			if (this.#visible && this.folder) this.tasks = (await api.agentTasks(this.folder)) ?? this.tasks;
		} catch {
			// as above
		}
	}

	#show(task: AgentTask) {
		this.task = task;
		this.writing = active(task) ? (task.writing ?? '') : '';
	}

	/** Asks again and again while the screen is shown or something is working. */
	async #watch() {
		if (this.#watching) return;
		this.#watching = true;
		while (this.#visible || this.working || active(this.task) || Date.now() < this.#lingerUntil) {
			await this.#tick();
			// Quick while the open task is working in front of the person, slow otherwise.
			const quick = this.#visible && active(this.task);
			await new Promise((r) => setTimeout(r, quick ? 150 : this.#visible ? 2500 : 5000));
		}
		this.#watching = false;
	}

	async open(id: string) {
		this.problem = '';
		try {
			this.#show(await api.agentTask(id));
		} catch (e) {
			this.problem = say(e);
		}
	}

	/** Opens a task from anywhere (the sidebar, the search): its folder first, then the task. */
	async show(row: { id: string; folder: string }) {
		if (this.task?.id === row.id) return;
		if (this.folder !== row.folder && !this.tasks.some((t) => t.id === row.id)) await this.setFolder(row.folder);
		await this.open(row.id);
		this.#watch();
	}

	/** The person names a task. The name is theirs from then on. */
	async rename(id: string, title: string) {
		const clean = title.replace(/\s+/g, ' ').trim().slice(0, 80);
		const known = this.all.find((t) => t.id === id);
		if (!clean || clean === (known?.title ?? this.task?.title)) return;
		try {
			const task = await api.renameAgentTask(id, clean);
			if (this.task?.id === id) this.#show(task);
			for (const list of [this.all, this.tasks]) {
				const row = list.find((t) => t.id === id);
				if (row) Object.assign(row, { title: task.title, title_by: 'person', updated_ms: task.updated_ms });
			}
		} catch (e) {
			this.problem = say(e);
		}
	}

	close() {
		this.task = null;
		this.writing = '';
		this.problem = '';
	}

	/** Starts a task, or continues the open one. Returns whether it was accepted. */
	async send(text: string, model?: string): Promise<boolean> {
		const prompt = text.trim();
		if (!prompt || !this.folder || this.busy) return false;
		this.busy = true;
		this.problem = '';
		try {
			this.#show(
				this.task
					? await api.tellAgentTask(this.task.id, { text: prompt, mode: this.mode, web: this.web, connectors: this.connectors })
					: await api.startAgentTask({ folder: this.folder, prompt, mode: this.mode, web: this.web, connectors: this.connectors, ...(model ? { model } : {}) })
			);
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
			this.#show(await api.answerAgentTask(task.id, task.pending.call, allow, always));
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
			this.#show(await api.stopAgentTask(this.task.id));
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
			this.#show(await api.undoAgentTask(task.id));
		} catch (e) {
			this.problem = say(e);
		} finally {
			this.busy = false;
		}
	}

	/** One thing asked for from the preview: it shows that it is going, and its error if it has one. */
	async #fromPreview(work: (task: AgentTask) => Promise<AgentTask | void>) {
		const task = this.task;
		if (!task || this.looking) return;
		this.looking = true;
		this.problem = '';
		try {
			const fresh = (await work(task)) ?? (await api.agentTask(task.id));
			if (this.task?.id === task.id) this.#show(fresh);
		} catch (e) {
			this.problem = say(e);
		} finally {
			this.looking = false;
		}
	}

	/** Stops the app the open task started. */
	stopApp() {
		return this.#fromPreview((task) => api.stopAgentApp(task.id));
	}

	/** Looks at the running app again the way the model does: its errors and a new picture. */
	lookAgain(size: string) {
		return this.#fromPreview(async (task) => void (await api.lookAtAgentApp(task.id, size)));
	}

	/** Takes a new picture of the phone's screen. */
	deviceShot() {
		return this.#fromPreview((task) => api.agentDeviceShot(task.id));
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
