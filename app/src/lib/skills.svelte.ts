import * as api from './api';
import type { FoundSkill, Skill, SkillDetail } from './api';
import { app } from './state.svelte';

const say = (e: unknown) => (e instanceof Error ? e.message : String(e));

export const MAX_SKILL_NAME = 64;
export const MAX_SKILL_DESCRIPTION = 300;
export const MAX_SKILL_BODY = 20_000;

/** A name a skill can have, from what the person typed: lower case, dashes for everything else. */
export const skillName = (typed: string) =>
	typed
		.normalize('NFKD')
		.replace(/[̀-ͯ]/g, '')
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '')
		.slice(0, MAX_SKILL_NAME)
		.replace(/-+$/, '');

/** What the form is working on. */
export interface SkillDraft {
	/** The name it is saved under now; '' for a new one. */
	was: string;
	name: string;
	description: string;
	body: string;
	/** A skill that comes with Rigbrew: saving makes the person's own version under the same name. */
	builtIn: boolean;
	/** The skill this is a copy of, whose other files come along. */
	copyOf: string;
}

/** Skills being looked at before they are brought in. */
export interface ImportState {
	from: 'folder' | 'github';
	location: string;
	looking: boolean;
	found: FoundSkill[];
	/** The names ticked to bring in. */
	picked: string[];
	problem: string;
	saving: boolean;
}

/**
 * Every skill there is, as the service keeps them (`/v1/skills`), and the sheet that manages
 * them. With an older service the sheet says so.
 */
class SkillsState {
	list = $state<Skill[]>([]);
	/** Where the person's own skills are kept, for the small print. */
	dir = $state('');
	support = $state<'unknown' | 'yes' | 'no'>('unknown');
	sheet = $state<null | 'list' | 'form' | 'read' | 'import'>(null);
	draft = $state<SkillDraft | null>(null);
	reading = $state<SkillDetail | null>(null);
	bringing = $state<ImportState | null>(null);
	problem = $state('');
	saving = $state(false);
	/** The Code folder whose own skills are listed too. */
	#folder = '';

	async load(folder = this.#folder) {
		this.#folder = folder;
		try {
			const answer = await api.skills(folder || undefined);
			this.support = answer === null ? 'no' : 'yes';
			if (answer) {
				this.list = answer.skills;
				this.dir = answer.dir;
			}
		} catch {
			// The service is away: the next load fills it in.
		}
	}

	open(from: string, folder = '') {
		this.problem = '';
		this.draft = null;
		this.reading = null;
		this.bringing = null;
		this.sheet = 'list';
		app.track('screen_viewed', { screen: 'skills', from });
		void this.load(folder);
	}

	close() {
		this.sheet = null;
		this.draft = null;
		this.reading = null;
		this.bringing = null;
	}

	back() {
		this.draft = null;
		this.reading = null;
		this.bringing = null;
		this.problem = '';
		this.sheet = 'list';
	}

	startNew() {
		this.problem = '';
		this.draft = { was: '', name: '', description: '', body: '', builtIn: false, copyOf: '' };
		this.sheet = 'form';
	}

	async read(skill: Skill) {
		this.problem = '';
		try {
			this.reading = await api.skill(skill.name, skill.source === 'folder' ? this.#folder : undefined);
			this.sheet = 'read';
		} catch (e) {
			this.problem = say(e);
		}
	}

	async edit(skill: Skill) {
		this.problem = '';
		try {
			const full = await api.skill(skill.name);
			this.draft = { was: full.name, name: full.name, description: full.description, body: full.body, builtIn: full.source === 'built-in', copyOf: '' };
			this.sheet = 'form';
		} catch (e) {
			this.problem = say(e);
		}
	}

	/** Opens the form on a copy under a name that is free. Saved only when the person saves. */
	async duplicate(skill: Skill) {
		this.problem = '';
		try {
			const full = await api.skill(skill.name, skill.source === 'folder' ? this.#folder : undefined);
			const taken = this.list.map((s) => s.name);
			const stem = `${full.name}-copy`.slice(0, MAX_SKILL_NAME - 3);
			let name = stem;
			for (let n = 2; taken.includes(name); n++) name = `${stem}-${n}`;
			this.draft = { was: '', name, description: full.description, body: full.body, builtIn: false, copyOf: full.source === 'folder' ? '' : full.name };
			this.sheet = 'form';
		} catch (e) {
			this.problem = say(e);
		}
	}

	async save(): Promise<boolean> {
		const draft = this.draft;
		if (!draft || this.saving) return false;
		this.saving = true;
		this.problem = '';
		const body = { name: draft.name.trim(), description: draft.description.trim(), body: draft.body };
		try {
			if (draft.was) await api.saveSkill(draft.was, body);
			else await api.createSkill({ ...body, ...(draft.copyOf ? { copy_of: draft.copyOf } : {}) });
			await this.load();
			this.draft = null;
			this.sheet = 'list';
			return true;
		} catch (e) {
			this.problem = say(e);
			return false;
		} finally {
			this.saving = false;
		}
	}

	async setEnabled(skill: Skill, enabled: boolean) {
		this.problem = '';
		const before = skill.enabled;
		skill.enabled = enabled;
		try {
			await api.setSkillEnabled(skill.name, enabled);
		} catch (e) {
			skill.enabled = before;
			this.problem = say(e);
		}
	}

	async remove(skill: Skill) {
		this.problem = '';
		try {
			await api.deleteSkill(skill.name);
			await this.load();
			app.say(skill.replaces_built_in ? `Deleted your version of “${skill.name}”. The one that comes with Rigbrew is back.` : `Deleted the skill “${skill.name}”.`);
		} catch (e) {
			this.problem = say(e);
		}
	}

	startImport() {
		this.problem = '';
		this.bringing = { from: 'github', location: '', looking: false, found: [], picked: [], problem: '', saving: false };
		this.sheet = 'import';
	}

	/** Looks at what is at a folder or an address. Nothing is saved. */
	async look(from: 'folder' | 'github', location: string) {
		const b = this.bringing;
		if (!b || b.looking || !location.trim()) return;
		Object.assign(b, { from, location: location.trim(), looking: true, found: [], picked: [], problem: '' });
		try {
			const answer = await api.importSkills(from, b.location);
			b.found = answer.found;
			b.picked = answer.found.filter((f) => !f.problem).map((f) => f.name);
		} catch (e) {
			b.problem = say(e);
		} finally {
			b.looking = false;
		}
	}

	async chooseFolder() {
		try {
			const picked = await api.pickFolder('Choose a skill folder, or a folder of skills');
			if (picked) await this.look('folder', picked);
		} catch (e) {
			if (this.bringing) this.bringing.problem = say(e);
		}
	}

	/** Saves the ticked skills. */
	async bringIn() {
		const b = this.bringing;
		if (!b || b.saving || b.picked.length === 0) return;
		b.saving = true;
		b.problem = '';
		try {
			const answer = await api.importSkills(b.from, b.location, $state.snapshot(b.picked));
			await this.load();
			const failed = answer.found.filter((f) => b.picked.includes(f.name) && !answer.saved.includes(f.name));
			if (failed.length === 0) {
				this.bringing = null;
				this.sheet = 'list';
				app.say(answer.saved.length === 1 ? `Added the skill “${answer.saved[0]}”.` : `Added ${answer.saved.length} skills.`);
			} else {
				b.found = answer.found.filter((f) => !answer.saved.includes(f.name));
				b.picked = [];
			}
		} catch (e) {
			b.problem = say(e);
		} finally {
			b.saving = false;
		}
	}
}

export const skills = new SkillsState();
